//! 阿里云 Packages 制品仓库（generic 类型）REST 客户端。
//!
//! 覆盖两套能力：
//! * 单次 API 上传 / 流式原子下载 / 存在性检查 / 临时免密下载地址；
//! * 大文件分块上传：创建会话 -> PATCH 分块 -> 触发异步合并 -> 轮询合并状态（含 MD5 校验） -> 关联仓库路径。

use std::fmt;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use reqwest::blocking::multipart::{Form, Part};
use reqwest::blocking::Response;
use reqwest::header::{AUTHORIZATION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE};
use reqwest::Method;
use serde_json::{Map, Value};
use tempfile::Builder as TempFileBuilder;
use url::Url;

use crate::checksum::file_md5;
use crate::error::{FunPubError, Result};
use crate::http::{headers_from_pairs, HttpClient, HttpConfig, TimeoutConfig};

/// 默认分块大小：100 MiB（与阿里云官方脚本默认值保持一致）。
pub const DEFAULT_CHUNK_SIZE: usize = 100 * 1024 * 1024;
/// 默认合并状态轮询间隔：3 秒。
pub const DEFAULT_MERGE_POLL_INTERVAL: Duration = Duration::from_secs(3);
/// 默认合并状态轮询超时：20 分钟。
pub const DEFAULT_MERGE_TIMEOUT: Duration = Duration::from_secs(20 * 60);
/// 默认流式下载缓冲区：8 MiB。
pub const DEFAULT_DOWNLOAD_CHUNK_SIZE: usize = 8 * 1024 * 1024;

/// 对齐 Python `urllib.parse.quote(path, safe='/')` 的编码集合：
/// 保留 RFC 3986 unreserved (`A-Z a-z 0-9 - _ . ~`) 与 `/`。
const QUOTE_PATH_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~')
    .remove(b'/');

/// 解析后的阿里云 Generic 仓库 URL 元信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRepoUrl {
    pub repo_url: String,
    pub scheme: String,
    pub host: String,
    pub org_id: String,
    pub repo_id: String,
}

/// 解析阿里云 Generic 仓库地址，支持经典格式与专属域名格式：
///
/// 1. 经典格式：`https://packages.aliyun.com/api/protocol/{org_id}/generic/{repo}`
/// 2. 专属域名格式：`https://{组织标识}-{地域标识}.devops.aliyuncs.com/packages/api/protocol/{repo_type}/{repo}`
///    （专属域名格式下 `org_id` 与 `repo_id` 均取 `{repo}` 段）
pub fn parse_repo_url(repo_url: &str) -> Result<ParsedRepoUrl> {
    if repo_url.is_empty() {
        return Err(FunPubError::ConfigError("repo_url 不能为空".to_string()));
    }

    let trimmed = repo_url.trim_end_matches('/');
    if trimmed.contains('?') || trimmed.contains('#') {
        return Err(invalid_repo_url_error(repo_url));
    }

    let parsed = Url::parse(trimmed).map_err(|_| invalid_repo_url_error(repo_url))?;
    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(invalid_repo_url_error(repo_url));
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(invalid_repo_url_error(repo_url));
    }

    let host_str = parsed
        .host_str()
        .filter(|h| !h.is_empty())
        .ok_or_else(|| invalid_repo_url_error(repo_url))?;
    let host = match parsed.port() {
        Some(port) => format!("{host_str}:{port}"),
        None => host_str.to_string(),
    };

    let raw_path = parsed.path().strip_prefix('/').unwrap_or(parsed.path());
    let segments: Vec<&str> = raw_path.split('/').collect();
    if segments.iter().any(|s| s.is_empty()) {
        return Err(invalid_repo_url_error(repo_url));
    }

    // 经典格式: /api/protocol/{org_id}/{repo_type}/{repo}
    if segments.len() == 5 && segments[0] == "api" && segments[1] == "protocol" {
        return Ok(ParsedRepoUrl {
            repo_url: trimmed.to_string(),
            scheme: scheme.to_string(),
            host,
            org_id: segments[2].to_string(),
            repo_id: segments[4].to_string(),
        });
    }

    // 专属域名格式: https://{org}-{region}.devops.aliyuncs.com/packages/api/protocol/{repo_type}/{repo}
    if segments.len() == 5
        && segments[0] == "packages"
        && segments[1] == "api"
        && segments[2] == "protocol"
        && host_str.ends_with(".devops.aliyuncs.com")
        && host_str.len() > ".devops.aliyuncs.com".len()
    {
        let repo_id = segments[4].to_string();
        return Ok(ParsedRepoUrl {
            repo_url: trimmed.to_string(),
            scheme: scheme.to_string(),
            host,
            org_id: repo_id.clone(),
            repo_id,
        });
    }

    Err(invalid_repo_url_error(repo_url))
}

fn invalid_repo_url_error(repo_url: &str) -> FunPubError {
    FunPubError::ConfigError(format!(
        "无法解析 repo_url: {repo_url:?}，期望形如 \
         https://packages.aliyun.com/api/protocol/{{org_id}}/generic/{{repo}} 或 \
         https://{{组织标识}}-{{地域标识}}.devops.aliyuncs.com/packages/api/protocol/{{repo_type}}/{{repo}}"
    ))
}

fn basic_auth_header(username: &str, password: &str) -> String {
    let encoded = BASE64_STANDARD.encode(format!("{username}:{password}"));
    format!("Basic {encoded}")
}

fn truncate_text(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn safe_json_from_text(text: &str) -> Map<String, Value> {
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

fn safe_object(payload: &Map<String, Value>) -> Map<String, Value> {
    match payload.get("object") {
        Some(Value::Object(obj)) => obj.clone(),
        _ => Map::new(),
    }
}

fn is_successful(payload: &Map<String, Value>) -> bool {
    match payload.get("successful") {
        Some(Value::Bool(b)) => *b,
        Some(Value::Null) | None => true,
        Some(Value::Number(n)) => n.as_i64() != Some(0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// 阿里云 Packages Generic 仓库 REST 客户端。
#[derive(Clone)]
pub struct AliyunClient {
    repo_url: String,
    username: String,
    scheme: String,
    host: String,
    org_id: String,
    repo_id: String,
    http: HttpClient,
}

impl fmt::Debug for AliyunClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AliyunClient")
            .field("repo_url", &self.repo_url)
            .field("username", &self.username)
            .finish()
    }
}

impl AliyunClient {
    /// 构造阿里云客户端并校验 `repo_url` 及 Basic Auth 凭据。
    pub fn new(
        repo_url: &str,
        username: &str,
        password: &str,
        timeout: TimeoutConfig,
    ) -> Result<Self> {
        if repo_url.is_empty() {
            return Err(FunPubError::ConfigError("repo_url 不能为空".to_string()));
        }
        if username.is_empty() || password.is_empty() {
            return Err(FunPubError::AuthenticationError(
                "username/password 不能为空".to_string(),
            ));
        }

        let parsed = parse_repo_url(repo_url)?;
        let default_headers = headers_from_pairs([(
            AUTHORIZATION.as_str(),
            basic_auth_header(username, password),
        )])?;

        let http = HttpClient::new(HttpConfig {
            timeout,
            default_headers,
            ..HttpConfig::default()
        })?;

        Ok(Self {
            repo_url: parsed.repo_url,
            username: username.to_string(),
            scheme: parsed.scheme,
            host: parsed.host,
            org_id: parsed.org_id,
            repo_id: parsed.repo_id,
            http,
        })
    }

    pub fn repo_url(&self) -> &str {
        &self.repo_url
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn org_id(&self) -> &str {
        &self.org_id
    }

    pub fn repo_id(&self) -> &str {
        &self.repo_id
    }

    fn files_url(&self, path: &str) -> Result<String> {
        let clean = path.trim_start_matches('/');
        if clean.is_empty() {
            return Err(FunPubError::InvalidArgument(
                "远端制品路径 path 不能为空".to_string(),
            ));
        }
        let quoted = utf8_percent_encode(clean, QUOTE_PATH_SET);
        Ok(format!("{}/files/{quoted}", self.repo_url))
    }

    fn blob_uploads_url(&self, suffix: &str) -> String {
        format!(
            "{}://{}/api/external/repo/{}/GENERIC/repos/{}/blobs/uploads{}",
            self.scheme, self.host, self.org_id, self.repo_id, suffix
        )
    }

    // ------------------------------------------------------------------
    // 简单上传 / 下载 / 存在性检查 / 签名链接
    // ------------------------------------------------------------------

    /// 一次性 multipart 上传整个文件（适合小文件）。
    pub fn simple_upload(
        &self,
        filepath: &Path,
        path: &str,
        version: &str,
        filename: Option<&str>,
        description: Option<&str>,
    ) -> Result<Map<String, Value>> {
        let url = self.files_url(path)?;
        let mut params: Vec<(&str, &str)> = vec![("version", version)];
        if let Some(name) = filename.filter(|s| !s.is_empty()) {
            params.push(("fileName", name));
        }
        if let Some(desc) = description.filter(|s| !s.is_empty()) {
            params.push(("versionDescription", desc));
        }

        let upload_name = filename
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| {
                filepath
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_else(|| "file".to_string());

        let file = File::open(filepath)?;
        let file_len = file.metadata()?.len();
        let part = Part::reader_with_length(file, file_len).file_name(upload_name);
        let form = Form::new().part("file", part);

        let resp = self
            .http
            .inner()
            .post(&url)
            .query(&params)
            .multipart(form)
            .send()
            .map_err(|e| FunPubError::UploadError(format!("上传请求失败: {e}")))?;

        let status = resp.status().as_u16();
        let body = resp.text().unwrap_or_default();
        check_status(status, &body, "上传", ErrorKind::Upload)?;

        let payload = safe_json_from_text(&body);
        if !is_successful(&payload) {
            return Err(FunPubError::UploadError(format!(
                "上传失败: {}",
                Value::Object(payload)
            )));
        }
        Ok(safe_object(&payload))
    }

    /// 流式下载文件到 `dest_path`，通过同目录临时文件原子落盘，失败自动清理。
    pub fn download(
        &self,
        path: &str,
        version: &str,
        dest_path: &Path,
        chunk_size: usize,
    ) -> Result<()> {
        if chunk_size == 0 {
            return Err(FunPubError::InvalidArgument(
                "chunk_size 必须大于 0".to_string(),
            ));
        }
        let url = self.files_url(path)?;
        let parent_dir = dest_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));

        let mut resp = self
            .http
            .send_with_retry(&Method::GET, |c| c.get(&url).query(&[("version", version)]))?;

        let status = resp.status().as_u16();
        if status >= 400 {
            let body = resp.text().unwrap_or_default();
            return Err(status_error(status, &body, "下载", ErrorKind::Download));
        }

        let mut tmp_file = TempFileBuilder::new()
            .prefix(".funpub-")
            .suffix(".part")
            .tempfile_in(parent_dir)?;

        let expected_len = resp.content_length();
        let buf_len = chunk_size.min(DEFAULT_DOWNLOAD_CHUNK_SIZE);
        let mut buf = vec![0u8; buf_len];
        let mut total_written: u64 = 0;

        loop {
            let n = resp
                .read(&mut buf)
                .map_err(|e| FunPubError::DownloadError(format!("下载读取流失败: {e}")))?;
            if n == 0 {
                break;
            }
            tmp_file.write_all(&buf[..n])?;
            total_written += n as u64;
        }
        if let Some(expected) = expected_len {
            if total_written < expected {
                return Err(FunPubError::DownloadError(format!(
                    "下载不完整: 预期 {expected} 字节，实际接收 {total_written} 字节"
                )));
            }
        }
        tmp_file.as_file_mut().flush()?;

        tmp_file
            .persist(dest_path)
            .map_err(|e| FunPubError::Io(e.error))?;
        Ok(())
    }

    /// 检查指定版本的远端制品是否存在。
    pub fn exist(&self, path: &str, version: &str) -> Result<bool> {
        let url = self.files_url(path)?;
        let resp = self.http.send_with_retry(&Method::HEAD, |c| {
            c.head(&url).query(&[("version", version)])
        })?;
        let status = resp.status().as_u16();
        if status == 404 {
            return Ok(false);
        }
        if status >= 400 {
            let body = resp.text().unwrap_or_default();
            return Err(status_error(status, &body, "查询", ErrorKind::Http));
        }
        Ok(true)
    }

    /// 获取临时免密下载地址（`expiration_ms` 为毫秒级 Unix 时间戳）。
    pub fn get_signed_download_url(
        &self,
        path: &str,
        version: &str,
        expiration_ms: u64,
    ) -> Result<String> {
        let url = self.files_url(path)?;
        let exp_str = expiration_ms.to_string();
        let resp = self.http.send_with_retry(&Method::HEAD, |c| {
            c.head(&url).query(&[
                ("version", version),
                ("signUrl", "true"),
                ("expiration", exp_str.as_str()),
            ])
        })?;
        let status = resp.status().as_u16();
        if status >= 400 {
            let body = resp.text().unwrap_or_default();
            return Err(status_error(
                status,
                &body,
                "生成免密下载地址",
                ErrorKind::Http,
            ));
        }

        let signed = resp
            .headers()
            .get("x-artlab-generic-sign-url")
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                FunPubError::http(Some(status), "响应中没有 x-artlab-generic-sign-url header")
            })?;
        Ok(signed.to_string())
    }

    // ------------------------------------------------------------------
    // 大文件分块上传（5 步协议）
    // ------------------------------------------------------------------

    /// 分块上传大文件：创建会话 -> PATCH 分块 -> 异步合并 -> 轮询合并状态 -> 关联仓库。
    #[allow(clippy::too_many_arguments)]
    pub fn chunked_upload(
        &self,
        filepath: &Path,
        path: &str,
        version: &str,
        filename: Option<&str>,
        description: Option<&str>,
        chunk_size: usize,
        poll_interval: Duration,
        poll_timeout: Duration,
    ) -> Result<Map<String, Value>> {
        if chunk_size == 0 {
            return Err(FunPubError::InvalidArgument(
                "chunk_size 必须大于 0".to_string(),
            ));
        }
        if poll_interval.is_zero() {
            return Err(FunPubError::InvalidArgument(
                "poll_interval 必须大于 0".to_string(),
            ));
        }
        // 预先校验远端 path 非空
        let _ = self.files_url(path)?;

        let file_size = std::fs::metadata(filepath)?.len();
        let digest_md5 = file_md5(filepath)?;
        log::info!(
            "开始分块上传 {} ({file_size} bytes, md5={digest_md5})",
            filepath.display()
        );

        let (upload_uuid, location) = self.start_blob_upload()?;
        self.upload_chunks(&location, filepath, file_size, chunk_size)?;
        self.async_commit(&upload_uuid, &digest_md5)?;
        self.wait_for_merge(&upload_uuid, &digest_md5, poll_interval, poll_timeout)?;
        self.associate(path, version, filename, description, &digest_md5)
    }

    fn start_blob_upload(&self) -> Result<(String, String)> {
        let url = self.blob_uploads_url("");
        let resp = self
            .http
            .inner()
            .post(&url)
            .header(CONTENT_LENGTH, "0")
            .send()
            .map_err(|e| FunPubError::UploadError(format!("创建分块上传会话失败: {e}")))?;

        let status = resp.status().as_u16();
        if status == 401 {
            return Err(FunPubError::AuthenticationError(
                "创建分块上传会话失败: 认证失败(401)".to_string(),
            ));
        }
        if status != 201 && status != 202 {
            let body = resp.text().unwrap_or_default();
            return Err(FunPubError::UploadError(format!(
                "创建分块上传会话失败: {status} {}",
                truncate_text(&body, 500)
            )));
        }

        let location_raw = header_str(&resp, "location");
        let upload_uuid = header_str(&resp, "package-upload-uuid");
        let (Some(mut location), Some(upload_uuid)) = (location_raw, upload_uuid) else {
            return Err(FunPubError::UploadError(
                "创建分块上传会话响应缺少 location/package-upload-uuid".to_string(),
            ));
        };
        if location.starts_with('/') {
            location = format!("{}://{}{}", self.scheme, self.host, location);
        }
        Ok((upload_uuid, location))
    }

    fn upload_chunks(
        &self,
        location: &str,
        filepath: &Path,
        file_size: u64,
        chunk_size: usize,
    ) -> Result<()> {
        let mut file = File::open(filepath)?;
        let mut offset: u64 = 0;

        while offset < file_size {
            let to_read = (file_size - offset).min(chunk_size as u64) as usize;
            let mut chunk = vec![0u8; to_read];
            let mut filled = 0usize;
            while filled < to_read {
                let n = file.read(&mut chunk[filled..])?;
                if n == 0 {
                    break;
                }
                filled += n;
            }
            chunk.truncate(filled);
            if chunk.is_empty() {
                break;
            }

            let start = offset;
            let end = offset + chunk.len() as u64 - 1;
            let content_len = chunk.len().to_string();
            let resp = self
                .http
                .inner()
                .patch(location)
                .header(CONTENT_TYPE, "application/octet-stream")
                .header(CONTENT_RANGE, format!("{start}-{end}"))
                .header(CONTENT_LENGTH, content_len)
                .body(chunk)
                .send()
                .map_err(|e| {
                    FunPubError::UploadError(format!("分块上传失败 [{start}-{end}]: {e}"))
                })?;

            let status = resp.status().as_u16();
            if status == 401 {
                return Err(FunPubError::AuthenticationError(format!(
                    "分块上传失败 [{start}-{end}]: 认证失败(401)"
                )));
            }
            if !matches!(status, 200 | 201 | 202 | 204) {
                let body = resp.text().unwrap_or_default();
                return Err(FunPubError::UploadError(format!(
                    "分块上传失败 [{start}-{end}]: {status} {}",
                    truncate_text(&body, 500)
                )));
            }

            offset = end + 1;
            let pct = (offset as f64) * 100.0 / (file_size as f64);
            log::info!("已上传 {offset}/{file_size} bytes ({pct:.1}%)");
        }

        Ok(())
    }

    fn async_commit(&self, upload_uuid: &str, file_md5: &str) -> Result<()> {
        let url = self.blob_uploads_url(&format!("/{upload_uuid}/async-commit"));
        let digest = format!("md5:{file_md5}");
        let resp = self
            .http
            .inner()
            .post(&url)
            .query(&[("digest", digest.as_str())])
            .header(CONTENT_LENGTH, "0")
            .send()
            .map_err(|e| FunPubError::UploadError(format!("触发异步合并失败: {e}")))?;

        let status = resp.status().as_u16();
        if status == 401 {
            return Err(FunPubError::AuthenticationError(
                "触发异步合并失败: 认证失败(401)".to_string(),
            ));
        }
        if status != 202 {
            let body = resp.text().unwrap_or_default();
            return Err(FunPubError::UploadError(format!(
                "触发异步合并失败: {status} {}",
                truncate_text(&body, 500)
            )));
        }
        Ok(())
    }

    /// 轮询合并状态并在完成后校验远端 MD5。
    pub(crate) fn wait_for_merge(
        &self,
        upload_uuid: &str,
        expected_md5: &str,
        poll_interval: Duration,
        poll_timeout: Duration,
    ) -> Result<()> {
        let deadline = Instant::now() + poll_timeout;
        let url = self.blob_uploads_url(&format!("/{upload_uuid}/merge-status"));

        loop {
            let resp = self
                .http
                .inner()
                .get(&url)
                .send()
                .map_err(|e| FunPubError::UploadError(format!("查询合并状态失败: {e}")))?;

            let status = resp.status().as_u16();
            match status {
                200 => {
                    let header_md5 = header_str(&resp, "x-blob-checksum-md5");
                    let body = resp.text().unwrap_or_default();
                    let merged_md5 = header_md5.or_else(|| {
                        let payload = safe_json_from_text(&body);
                        let obj = safe_object(&payload);
                        obj.get("md5")
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                            .map(ToOwned::to_owned)
                    });
                    if let Some(remote_md5) = merged_md5 {
                        if !remote_md5.eq_ignore_ascii_case(expected_md5) {
                            return Err(FunPubError::ChecksumMismatchError(format!(
                                "合并后 md5 不一致: 本地={expected_md5} 远端={remote_md5}"
                            )));
                        }
                    }
                    log::info!("分块合并完成");
                    return Ok(());
                }
                202 => {
                    let body = resp.text().unwrap_or_default();
                    let payload = safe_json_from_text(&body);
                    let obj = safe_object(&payload);
                    let merge_status = obj
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("PENDING");
                    if merge_status == "FAILED" {
                        return Err(FunPubError::UploadError("分块合并失败".to_string()));
                    }
                }
                401 => {
                    return Err(FunPubError::AuthenticationError(
                        "查询合并状态失败: 认证失败(401)".to_string(),
                    ));
                }
                404 => {
                    return Err(FunPubError::NotFoundError("分块上传会话不存在".to_string()));
                }
                500 => {
                    let body = resp.text().unwrap_or_default();
                    let payload = safe_json_from_text(&body);
                    let obj = safe_object(&payload);
                    let message = obj
                        .get("errorMessage")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .or_else(|| {
                            payload
                                .get("errorMessage")
                                .and_then(Value::as_str)
                                .filter(|s| !s.is_empty())
                        });
                    let detail = match message {
                        Some(msg) => msg.to_string(),
                        None => truncate_text(&body, 500),
                    };
                    return Err(FunPubError::UploadError(format!("分块合并失败: {detail}")));
                }
                _ => {
                    let body = resp.text().unwrap_or_default();
                    return Err(FunPubError::UploadError(format!(
                        "查询合并状态失败: {status} {}",
                        truncate_text(&body, 500)
                    )));
                }
            }

            let now = Instant::now();
            if now >= deadline {
                return Err(FunPubError::MergeTimeoutError(format!(
                    "分块合并轮询超时（{}s）",
                    poll_timeout.as_secs_f64()
                )));
            }
            let remaining = deadline.saturating_duration_since(now);
            thread::sleep(poll_interval.min(remaining));
        }
    }

    pub(crate) fn associate(
        &self,
        path: &str,
        version: &str,
        filename: Option<&str>,
        description: Option<&str>,
        digest_md5: &str,
    ) -> Result<Map<String, Value>> {
        let url = self.files_url(path)?;
        let mut params: Vec<(&str, &str)> = vec![("version", version)];
        if let Some(name) = filename.filter(|s| !s.is_empty()) {
            params.push(("fileName", name));
        }
        if let Some(desc) = description.filter(|s| !s.is_empty()) {
            params.push(("versionDescription", desc));
        }

        let resp = self
            .http
            .inner()
            .post(&url)
            .query(&params)
            .header("x-artlab-checksum-upload", "true")
            .header("x-artlab-checksum-md5", digest_md5)
            .header(CONTENT_LENGTH, "0")
            .send()
            .map_err(|e| FunPubError::UploadError(format!("关联制品到仓库失败: {e}")))?;

        let status = resp.status().as_u16();
        let body = resp.text().unwrap_or_default();
        if status == 401 {
            return Err(FunPubError::AuthenticationError(
                "关联制品到仓库失败: 认证失败(401)".to_string(),
            ));
        }
        if status == 404 {
            return Err(FunPubError::NotFoundError(
                "关联制品到仓库失败: 远端制品不存在(404)".to_string(),
            ));
        }
        if status != 200 && status != 201 {
            return Err(FunPubError::UploadError(format!(
                "关联制品到仓库失败: {status} {}",
                truncate_text(&body, 500)
            )));
        }

        let payload = safe_json_from_text(&body);
        if !is_successful(&payload) {
            return Err(FunPubError::UploadError(format!(
                "关联制品到仓库失败: {}",
                Value::Object(payload)
            )));
        }
        let obj = safe_object(&payload);
        if obj.is_empty() {
            let mut fallback = Map::new();
            fallback.insert("fileMd5".to_string(), Value::String(digest_md5.to_string()));
            Ok(fallback)
        } else {
            Ok(obj)
        }
    }
}

fn header_str(resp: &Response, name: &str) -> Option<String> {
    resp.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

#[derive(Clone, Copy)]
enum ErrorKind {
    Upload,
    Download,
    Http,
}

fn status_error(status: u16, body: &str, action: &str, kind: ErrorKind) -> FunPubError {
    if status == 401 {
        return FunPubError::AuthenticationError(format!("{action}失败: 认证失败(401)"));
    }
    if status == 404 {
        return FunPubError::NotFoundError(format!("{action}失败: 远端制品不存在(404)"));
    }
    let msg = format!("{action}失败: {status} {}", truncate_text(body, 500));
    match kind {
        ErrorKind::Upload => FunPubError::UploadError(msg),
        ErrorKind::Download => FunPubError::DownloadError(msg),
        ErrorKind::Http => FunPubError::http(Some(status), msg),
    }
}

fn check_status(status: u16, body: &str, action: &str, kind: ErrorKind) -> Result<()> {
    if status >= 400 {
        return Err(status_error(status, body, action, kind));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};

    const CLASSIC_URL: &str =
        "https://packages.aliyun.com/api/protocol/5fc5eb115dbd287006145e5f/generic/funpackage";
    const DEVOPS_URL: &str = "https://farfarfun-cn-hangzhou.devops.aliyuncs.com/packages/api/protocol/generic/funpackage";

    #[derive(Debug, Clone)]
    struct RecordedRequest {
        method: String,
        target: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    }

    struct MockResponse {
        status: u16,
        reason: &'static str,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
        truncate_body: Option<usize>,
    }

    impl MockResponse {
        fn ok_json(body: &str) -> Self {
            Self {
                status: 200,
                reason: "OK",
                headers: vec![("Content-Type".into(), "application/json".into())],
                body: body.as_bytes().to_vec(),
                truncate_body: None,
            }
        }

        fn with_status(status: u16, reason: &'static str, body: &str) -> Self {
            Self {
                status,
                reason,
                headers: vec![("Content-Type".into(), "application/json".into())],
                body: body.as_bytes().to_vec(),
                truncate_body: None,
            }
        }
    }

    fn read_http_request(stream: &mut TcpStream) -> RecordedRequest {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut request_line = String::new();
        reader.read_line(&mut request_line).unwrap();
        let parts: Vec<&str> = request_line.split_whitespace().collect();
        let method = parts[0].to_string();
        let target = parts[1].to_string();

        let mut headers = Vec::new();
        let mut content_length: usize = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if trimmed.is_empty() {
                break;
            }
            if let Some((k, v)) = trimmed.split_once(':') {
                let key_lower = k.trim().to_ascii_lowercase();
                let val = v.trim().to_string();
                if key_lower == "content-length" {
                    content_length = val.parse().unwrap_or(0);
                }
                headers.push((key_lower, val));
            }
        }

        let mut body = vec![0u8; content_length];
        if content_length > 0 {
            reader.read_exact(&mut body).unwrap();
        }
        RecordedRequest {
            method,
            target,
            headers,
            body,
        }
    }

    fn spawn_mock_server<F>(
        expected_requests: usize,
        handler: F,
    ) -> (
        String,
        Arc<Mutex<Vec<RecordedRequest>>>,
        thread::JoinHandle<()>,
    )
    where
        F: Fn(usize, &RecordedRequest) -> MockResponse + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let recorded_clone = Arc::clone(&recorded);

        let handle = thread::spawn(move || {
            for idx in 0..expected_requests {
                let (mut stream, _) = listener.accept().unwrap();
                let req = read_http_request(&mut stream);
                let resp = handler(idx, &req);
                recorded_clone.lock().unwrap().push(req);

                let declared_len = resp.body.len();
                let mut head = format!(
                    "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n",
                    resp.status, resp.reason, declared_len
                );
                for (k, v) in &resp.headers {
                    head.push_str(&format!("{k}: {v}\r\n"));
                }
                head.push_str("\r\n");
                let _ = stream.write_all(head.as_bytes());
                if let Some(limit) = resp.truncate_body {
                    let _ = stream.write_all(&resp.body[..limit.min(resp.body.len())]);
                } else {
                    let _ = stream.write_all(&resp.body);
                }
                let _ = stream.flush();
            }
        });

        let repo_url = format!("http://{addr}/api/protocol/org123/generic/myrepo");
        (repo_url, recorded, handle)
    }

    #[test]
    fn parses_classic_and_devops_repo_urls() {
        let classic = parse_repo_url(CLASSIC_URL).unwrap();
        assert_eq!(classic.scheme, "https");
        assert_eq!(classic.host, "packages.aliyun.com");
        assert_eq!(classic.org_id, "5fc5eb115dbd287006145e5f");
        assert_eq!(classic.repo_id, "funpackage");

        let devops = parse_repo_url(DEVOPS_URL).unwrap();
        assert_eq!(devops.host, "farfarfun-cn-hangzhou.devops.aliyuncs.com");
        assert_eq!(devops.org_id, "funpackage");
        assert_eq!(devops.repo_id, "funpackage");

        // 末尾斜杠允许，多余路径段或查询串拒绝
        assert!(parse_repo_url(&format!("{CLASSIC_URL}/")).is_ok());
        for bad in [
            "",
            "https://example.com/not-aliyun",
            &format!("{CLASSIC_URL}/extra"),
            &format!("{CLASSIC_URL}?foo=bar"),
            &format!("{DEVOPS_URL}/extra"),
        ] {
            assert!(matches!(
                parse_repo_url(bad),
                Err(FunPubError::ConfigError(_))
            ));
        }
    }

    #[test]
    fn debug_does_not_leak_password_and_empty_creds_fail() {
        let client = AliyunClient::new(
            CLASSIC_URL,
            "alice",
            "super-secret",
            TimeoutConfig::default(),
        )
        .unwrap();
        let dbg = format!("{client:?}");
        assert!(dbg.contains("alice"));
        assert!(!dbg.contains("super-secret"));

        assert!(matches!(
            AliyunClient::new(CLASSIC_URL, "", "", TimeoutConfig::default()),
            Err(FunPubError::AuthenticationError(_))
        ));
    }

    #[test]
    fn simple_upload_and_error_cases() {
        let (repo_url, recorded, handle) = spawn_mock_server(3, |idx, _req| match idx {
            0 => MockResponse::ok_json(
                r#"{"successful": true, "object": {"fileMd5": "abc", "fileSize": 4, "url": "https://x/y"}}"#,
            ),
            1 => MockResponse::ok_json(r#"{"successful": false, "errorMessage": "bad"}"#),
            _ => MockResponse::ok_json(r#"{"successful": true, "object": null}"#),
        });

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, b"data").unwrap();

        let client = AliyunClient::new(&repo_url, "u", "p", TimeoutConfig::default()).unwrap();
        let obj = client
            .simple_upload(&file, "a/b/c", "1.0.0", Some("out.txt"), Some("desc"))
            .unwrap();
        assert_eq!(obj.get("fileMd5").and_then(Value::as_str), Some("abc"));

        let err = client.simple_upload(&file, "a/b", "1.0.0", None, None);
        assert!(matches!(err, Err(FunPubError::UploadError(_))));

        let empty = client
            .simple_upload(&file, "a/b", "1.0.0", None, None)
            .unwrap();
        assert!(empty.is_empty());

        handle.join().unwrap();
        let reqs = recorded.lock().unwrap();
        assert_eq!(reqs[0].method, "POST");
        assert!(reqs[0].target.contains("/files/a/b/c"));
        assert!(reqs[0].target.contains("version=1.0.0"));
        assert!(reqs[0].target.contains("fileName=out.txt"));
        assert!(reqs[0].target.contains("versionDescription=desc"));
    }

    #[test]
    fn exist_and_signed_url_and_empty_path() {
        let (repo_url, _recorded, handle) = spawn_mock_server(4, |idx, _req| match idx {
            0 => MockResponse::with_status(200, "OK", ""),
            1 => MockResponse::with_status(404, "Not Found", ""),
            2 => MockResponse {
                status: 200,
                reason: "OK",
                headers: vec![("x-artlab-generic-sign-url".into(), "https://signed".into())],
                body: Vec::new(),
                truncate_body: None,
            },
            _ => MockResponse::with_status(200, "OK", ""),
        });

        let client = AliyunClient::new(&repo_url, "u", "p", TimeoutConfig::default()).unwrap();
        assert!(client.exist("a/b", "1.0.0").unwrap());
        assert!(!client.exist("a/b", "1.0.0").unwrap());
        assert_eq!(
            client
                .get_signed_download_url("a/b", "1.0.0", 1234567890)
                .unwrap(),
            "https://signed"
        );
        assert!(matches!(
            client.get_signed_download_url("a/b", "1.0.0", 1234567890),
            Err(FunPubError::HttpError { .. })
        ));
        assert!(matches!(
            client.exist("///", "1.0.0"),
            Err(FunPubError::InvalidArgument(_))
        ));
        handle.join().unwrap();
    }

    #[test]
    fn download_atomic_and_cleans_up_on_truncated_stream() {
        let (repo_url, _recorded, handle) = spawn_mock_server(2, |idx, _req| match idx {
            0 => MockResponse {
                status: 200,
                reason: "OK",
                headers: vec![],
                body: b"helloworld".to_vec(),
                truncate_body: None,
            },
            _ => MockResponse {
                status: 200,
                reason: "OK",
                headers: vec![],
                body: b"0123456789abcdef".to_vec(),
                truncate_body: Some(4),
            },
        });

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("download.bin");
        let client = AliyunClient::new(&repo_url, "u", "p", TimeoutConfig::default()).unwrap();

        client.download("a/b", "1.0.0", &dest, 4).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"helloworld");

        // 截断响应不应破坏原文件，也不应遗留 .part 文件
        let err = client.download("a/b", "1.0.0", &dest, 4);
        assert!(err.is_err());
        assert_eq!(std::fs::read(&dest).unwrap(), b"helloworld");
        let parts: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".part"))
            .collect();
        assert!(parts.is_empty());

        handle.join().unwrap();
    }

    #[test]
    fn chunked_upload_full_five_step_flow() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("big.bin");
        std::fs::write(&file, b"abcdefghij").unwrap(); // 10 bytes -> 4 + 4 + 2
        let expected_md5 = file_md5(&file).unwrap();
        let md5_for_server = expected_md5.clone();

        // 1 start + 3 patch + 1 commit + 2 merge-status (202 -> 200) + 1 associate = 8 requests
        let (repo_url, recorded, handle) = spawn_mock_server(8, move |idx, _req| match idx {
            0 => MockResponse {
                status: 201,
                reason: "Created",
                headers: vec![
                    ("location".into(), "/uploads/xyz".into()),
                    ("package-upload-uuid".into(), "xyz".into()),
                ],
                body: Vec::new(),
                truncate_body: None,
            },
            1..=3 => MockResponse::with_status(202, "Accepted", ""),
            4 => MockResponse::with_status(202, "Accepted", ""),
            5 => MockResponse::with_status(202, "Accepted", r#"{"object":{"status":"PENDING"}}"#),
            6 => MockResponse {
                status: 200,
                reason: "OK",
                headers: vec![("x-blob-checksum-md5".into(), md5_for_server.clone())],
                body: b"{}".to_vec(),
                truncate_body: None,
            },
            _ => MockResponse::ok_json(&format!(
                r#"{{"successful":true,"object":{{"fileMd5":"{}","url":"https://dl/big"}}}}"#,
                md5_for_server
            )),
        });

        let client = AliyunClient::new(&repo_url, "u", "p", TimeoutConfig::default()).unwrap();
        let obj = client
            .chunked_upload(
                &file,
                "a/b",
                "1.0.0",
                Some("big.bin"),
                None,
                4,
                Duration::from_millis(10),
                Duration::from_secs(5),
            )
            .unwrap();
        assert_eq!(
            obj.get("fileMd5").and_then(Value::as_str),
            Some(expected_md5.as_str())
        );

        handle.join().unwrap();
        let reqs = recorded.lock().unwrap();
        assert_eq!(reqs.len(), 8);
        assert_eq!(reqs[1].method, "PATCH");
        assert_eq!(reqs[1].body, b"abcd");
        assert_eq!(reqs[2].method, "PATCH");
        assert_eq!(reqs[2].body, b"efgh");
        assert_eq!(reqs[3].method, "PATCH");
        assert_eq!(reqs[3].body, b"ij");
        assert!(reqs[1]
            .headers
            .iter()
            .any(|(k, v)| k == "content-range" && v == "0-3"));
        assert!(reqs[2]
            .headers
            .iter()
            .any(|(k, v)| k == "content-range" && v == "4-7"));
        assert!(reqs[3]
            .headers
            .iter()
            .any(|(k, v)| k == "content-range" && v == "8-9"));
        assert_eq!(reqs[7].method, "POST");
        assert!(reqs[7]
            .headers
            .iter()
            .any(|(k, v)| k == "x-artlab-checksum-upload" && v == "true"));
    }

    #[test]
    fn merge_polling_handles_mismatch_failed_404_500_and_timeout() {
        let (repo_url, _recorded, handle) = spawn_mock_server(6, |idx, _req| match idx {
            0 => MockResponse::with_status(202, "Accepted", r#"{"object":{"status":"PENDING"}}"#),
            1 => MockResponse {
                status: 200,
                reason: "OK",
                headers: vec![("x-blob-checksum-md5".into(), "wrong".into())],
                body: b"{}".to_vec(),
                truncate_body: None,
            },
            2 => MockResponse::with_status(202, "Accepted", r#"{"object":{"status":"FAILED"}}"#),
            3 => MockResponse::with_status(404, "Not Found", ""),
            4 => MockResponse::with_status(
                500,
                "Internal Server Error",
                r#"{"object":null,"errorMessage":"disk full"}"#,
            ),
            _ => MockResponse::with_status(202, "Accepted", r#"{"object":{"status":"PENDING"}}"#),
        });

        let client = AliyunClient::new(&repo_url, "u", "p", TimeoutConfig::default()).unwrap();
        assert!(matches!(
            client.wait_for_merge(
                "uid",
                "expected",
                Duration::from_millis(5),
                Duration::from_secs(5)
            ),
            Err(FunPubError::ChecksumMismatchError(_))
        ));
        assert!(matches!(
            client.wait_for_merge(
                "uid",
                "expected",
                Duration::from_millis(5),
                Duration::from_secs(5)
            ),
            Err(FunPubError::UploadError(_))
        ));
        assert!(matches!(
            client.wait_for_merge(
                "uid",
                "expected",
                Duration::from_millis(5),
                Duration::from_secs(5)
            ),
            Err(FunPubError::NotFoundError(_))
        ));
        let err500 = client
            .wait_for_merge(
                "uid",
                "expected",
                Duration::from_millis(5),
                Duration::from_secs(5),
            )
            .unwrap_err();
        assert!(err500.to_string().contains("disk full"));
        assert!(matches!(
            client.wait_for_merge("uid", "expected", Duration::from_millis(5), Duration::ZERO),
            Err(FunPubError::MergeTimeoutError(_))
        ));

        handle.join().unwrap();
    }

    #[test]
    fn get_retries_on_503_and_succeeds() {
        let (repo_url, recorded, handle) = spawn_mock_server(2, |idx, _req| match idx {
            0 => MockResponse {
                status: 503,
                reason: "Service Unavailable",
                headers: vec![("Retry-After".into(), "0".into())],
                body: b"busy".to_vec(),
                truncate_body: None,
            },
            _ => MockResponse::with_status(200, "OK", ""),
        });

        let client = AliyunClient::new(&repo_url, "u", "p", TimeoutConfig::default()).unwrap();
        assert!(client.exist("a/b", "1.0.0").unwrap());
        handle.join().unwrap();
        assert_eq!(recorded.lock().unwrap().len(), 2);
    }
}
