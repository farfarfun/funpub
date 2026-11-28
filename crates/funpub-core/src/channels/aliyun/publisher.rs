//! 阿里云 Packages Generic 制品仓库发布器实现。
//!
//! 凭据解析顺序：显式构造参数 > `funsecret-core`（按 `funpub / aliyun / <repo_type> / <repo_name> / <field>` 查找）。

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use funsecret_core::SecretManage;
use serde_json::Value;

use crate::base::{
    ensure_parent_dir, get_filepath, DownloadTarget, PublishResult, Publisher, UploadOptions,
};
use crate::channels::aliyun::client::{
    AliyunClient, DEFAULT_CHUNK_SIZE, DEFAULT_DOWNLOAD_CHUNK_SIZE, DEFAULT_MERGE_POLL_INTERVAL,
    DEFAULT_MERGE_TIMEOUT,
};
use crate::error::{FunPubError, Result};
use crate::http::TimeoutConfig;

/// 默认仓库类型。
pub const DEFAULT_REPO_TYPE: &str = "generic";

/// 阿里云发布器配置。
#[derive(Debug, Clone)]
pub struct AliyunConfig {
    pub repo_name: Option<String>,
    pub repo_type: String,
    pub repo_url: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub chunk_size: usize,
    pub chunk_threshold: Option<usize>,
    pub timeout: TimeoutConfig,
    /// 可选覆盖 `funsecret` 密钥目录（默认为 `~/.secret` 或 `FUN_SECRET_PATH`）。
    pub secret_dir: Option<String>,
}

impl Default for AliyunConfig {
    fn default() -> Self {
        Self {
            repo_name: None,
            repo_type: DEFAULT_REPO_TYPE.to_string(),
            repo_url: None,
            username: None,
            password: None,
            chunk_size: DEFAULT_CHUNK_SIZE,
            chunk_threshold: None,
            timeout: TimeoutConfig::default(),
            secret_dir: None,
        }
    }
}

/// 基于阿里云 Packages Generic 仓库实现的发布器。
#[derive(Debug, Clone)]
pub struct AliyunPublisher {
    client: AliyunClient,
    chunk_size: usize,
    chunk_threshold: usize,
}

impl AliyunPublisher {
    /// 构建阿里云发布器；当显式参数缺失且提供了 `repo_name` 时自动从 `funsecret-core` 读取凭据。
    pub fn new(config: AliyunConfig) -> Result<Self> {
        if config.chunk_size == 0 {
            return Err(FunPubError::InvalidArgument(
                "chunk_size 必须大于 0".to_string(),
            ));
        }

        let repo_type = if config.repo_type.is_empty() {
            DEFAULT_REPO_TYPE.to_string()
        } else {
            config.repo_type
        };
        let repo_name = config.repo_name.filter(|s| !s.is_empty());
        let mut repo_url = config.repo_url.filter(|s| !s.is_empty());
        let mut username = config.username.filter(|s| !s.is_empty());
        let mut password = config.password.filter(|s| !s.is_empty());

        let needs_secret = repo_url.is_none() || username.is_none() || password.is_none();
        if needs_secret {
            if let Some(ref name) = repo_name {
                let manage = SecretManage::new(config.secret_dir.as_deref(), None, None)
                    .map_err(|e| FunPubError::SecretError(e.to_string()))?;
                let read_field = |field: &str| -> Result<Option<String>> {
                    manage
                        .read(
                            "funpub", "aliyun", &repo_type, name, field, None, false, true, None,
                        )
                        .map(|opt| opt.filter(|s| !s.is_empty()))
                        .map_err(|e| FunPubError::SecretError(e.to_string()))
                };

                if repo_url.is_none() {
                    repo_url = read_field("repo_url")?;
                }
                if username.is_none() {
                    username = read_field("username")?;
                }
                if password.is_none() {
                    password = read_field("password")?;
                }
            }
        }

        let Some(repo_url) = repo_url else {
            return Err(FunPubError::ConfigError(format!(
                "必须提供 repo_url，或提供 repo_name 并预先通过 funsecret 配置：\
                 funsecret write {{repo_url}} funpub aliyun {repo_type} <repo_name> repo_url"
            )));
        };

        let (Some(username), Some(password)) = (username, password) else {
            let target = repo_name.as_deref().unwrap_or("<repo_name>");
            return Err(FunPubError::AuthenticationError(format!(
                "缺少认证凭据 username/password，请通过参数传入或通过 funsecret 配置：\
                 funsecret write {{value}} funpub aliyun {repo_type} {target} username|password"
            )));
        };

        let chunk_threshold = config.chunk_threshold.unwrap_or(config.chunk_size);
        let client = AliyunClient::new(&repo_url, &username, &password, config.timeout)?;

        Ok(Self {
            client,
            chunk_size: config.chunk_size,
            chunk_threshold,
        })
    }

    /// 返回底层阿里云 REST 客户端引用。
    pub fn client(&self) -> &AliyunClient {
        &self.client
    }

    pub fn chunk_size(&self) -> usize {
        self.chunk_size
    }

    pub fn chunk_threshold(&self) -> usize {
        self.chunk_threshold
    }
}

impl Publisher for AliyunPublisher {
    fn upload_file(
        &self,
        filepath: &Path,
        path: &str,
        version: &str,
        options: &UploadOptions,
    ) -> Result<PublishResult> {
        if !filepath.is_file() {
            return Err(FunPubError::FileNotFound(filepath.to_path_buf()));
        }

        if !options.overwrite && self.exist(path, version)? {
            return Err(FunPubError::FileExists(format!(
                "{path}@{version} 已存在，如需覆盖请传 overwrite=True"
            )));
        }

        let filename = options
            .filename
            .clone()
            .filter(|s| !s.is_empty())
            .or_else(|| {
                filepath
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(ToOwned::to_owned)
            });

        let file_size = std::fs::metadata(filepath)?.len();
        let use_chunked = options
            .chunked
            .unwrap_or(file_size > self.chunk_threshold as u64);

        log::info!(
            "上传 {} -> {path}@{version} ({}上传, {file_size} bytes)",
            filepath.display(),
            if use_chunked { "分块" } else { "简单" }
        );

        let obj = if use_chunked {
            self.client.chunked_upload(
                filepath,
                path,
                version,
                filename.as_deref(),
                options.description.as_deref(),
                self.chunk_size,
                options.poll_interval.unwrap_or(DEFAULT_MERGE_POLL_INTERVAL),
                options.poll_timeout.unwrap_or(DEFAULT_MERGE_TIMEOUT),
            )?
        } else {
            self.client.simple_upload(
                filepath,
                path,
                version,
                filename.as_deref(),
                options.description.as_deref(),
            )?
        };

        let mut result = PublishResult::new(path, version);
        result.url = obj
            .get("url")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        result.size = obj
            .get("fileSize")
            .and_then(Value::as_u64)
            .or(Some(file_size));
        result.md5 = obj
            .get("fileMd5")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        result.sha1 = obj
            .get("fileSha1")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        result.sha256 = obj
            .get("fileSha256")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        result.filename = filename;

        Ok(result)
    }

    fn download_file(&self, path: &str, version: &str, target: &DownloadTarget) -> Result<PathBuf> {
        let fallback_name = remote_basename(path);
        let effective_filename = target
            .filename
            .as_deref()
            .filter(|s| !s.is_empty())
            .or(fallback_name);

        let dest = get_filepath(
            target.save_dir.as_deref(),
            effective_filename,
            target.filepath.as_deref(),
        )?;

        if dest.is_dir() {
            return Err(FunPubError::IsADirectory(dest));
        }
        if dest.exists() && !target.overwrite {
            return Err(FunPubError::FileExists(format!(
                "{} 已存在，如需覆盖请传 overwrite=True",
                dest.display()
            )));
        }

        ensure_parent_dir(&dest)?;
        log::info!("下载 {path}@{version} -> {}", dest.display());
        self.client
            .download(path, version, &dest, DEFAULT_DOWNLOAD_CHUNK_SIZE)?;
        Ok(dest)
    }

    fn exist(&self, path: &str, version: &str) -> Result<bool> {
        self.client.exist(path, version)
    }

    fn get_download_url(
        &self,
        path: &str,
        version: &str,
        expiration_ms: Option<u64>,
    ) -> Result<String> {
        let exp = expiration_ms.unwrap_or_else(|| default_expiration_ms(Duration::from_secs(3600)));
        self.client.get_signed_download_url(path, version, exp)
    }
}

/// 从远端路径提取默认文件名（忽略末尾 `/`）。
pub(crate) fn remote_basename(path: &str) -> Option<&str> {
    let trimmed = path.trim_end_matches('/');
    trimmed.rsplit('/').next().filter(|s| !s.is_empty())
}

fn default_expiration_ms(ttl: Duration) -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    (now + ttl).as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPO_URL: &str = "https://packages.aliyun.com/api/protocol/x/generic/y";

    #[test]
    fn loads_credentials_from_funsecret_core() {
        let dir = tempfile::tempdir().unwrap();
        let secret_dir = dir.path().to_str().unwrap();
        let manage = SecretManage::new(Some(secret_dir), None, None).unwrap();
        manage
            .write(
                REPO_URL,
                "funpub",
                "aliyun",
                "generic",
                "funpackage",
                "repo_url",
                true,
                None,
            )
            .unwrap();
        manage
            .write(
                "alice",
                "funpub",
                "aliyun",
                "generic",
                "funpackage",
                "username",
                true,
                None,
            )
            .unwrap();
        manage
            .write(
                "secret-pw",
                "funpub",
                "aliyun",
                "generic",
                "funpackage",
                "password",
                true,
                None,
            )
            .unwrap();

        let pub_inst = AliyunPublisher::new(AliyunConfig {
            repo_name: Some("funpackage".to_string()),
            secret_dir: Some(secret_dir.to_string()),
            ..AliyunConfig::default()
        })
        .unwrap();

        assert_eq!(pub_inst.client().repo_url(), REPO_URL);
        assert_eq!(pub_inst.client().username(), "alice");
    }

    #[test]
    fn missing_repo_url_or_creds_return_expected_errors() {
        let dir = tempfile::tempdir().unwrap();
        let secret_dir = dir.path().to_str().unwrap();

        let err_url = AliyunPublisher::new(AliyunConfig {
            username: Some("u".to_string()),
            password: Some("p".to_string()),
            secret_dir: Some(secret_dir.to_string()),
            ..AliyunConfig::default()
        })
        .unwrap_err();
        assert!(matches!(err_url, FunPubError::ConfigError(_)));

        let err_auth = AliyunPublisher::new(AliyunConfig {
            repo_url: Some(REPO_URL.to_string()),
            repo_name: Some("myrepo".to_string()),
            secret_dir: Some(secret_dir.to_string()),
            ..AliyunConfig::default()
        })
        .unwrap_err();
        assert!(matches!(err_auth, FunPubError::AuthenticationError(_)));
        assert!(err_auth.to_string().contains("funsecret"));
    }

    #[test]
    fn validates_chunk_size_and_threshold() {
        let err = AliyunPublisher::new(AliyunConfig {
            repo_url: Some(REPO_URL.to_string()),
            username: Some("u".to_string()),
            password: Some("p".to_string()),
            chunk_size: 0,
            ..AliyunConfig::default()
        })
        .unwrap_err();
        assert!(matches!(err, FunPubError::InvalidArgument(_)));

        let ok = AliyunPublisher::new(AliyunConfig {
            repo_url: Some(REPO_URL.to_string()),
            username: Some("u".to_string()),
            password: Some("p".to_string()),
            chunk_size: 1024,
            chunk_threshold: Some(0),
            ..AliyunConfig::default()
        })
        .unwrap();
        assert_eq!(ok.chunk_size(), 1024);
        assert_eq!(ok.chunk_threshold(), 0);
    }
}
