//! 发布渠道统一抽象、上传结果与本地路径工具。

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{FunPubError, Result};

/// 归一化路径为绝对路径，不要求目标文件已存在（对齐 Python `Path.resolve()` 语义）。
fn normalize_absolute(path: &Path) -> Result<PathBuf> {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };

    // 如果路径或父目录已存在，尽量与系统 canonicalize 对齐（例如 macOS /var -> /private/var）
    if let Ok(canon) = std::fs::canonicalize(&abs) {
        return Ok(canon);
    }
    if let (Some(parent), Some(name)) = (abs.parent(), abs.file_name()) {
        if let Ok(canon_parent) = std::fs::canonicalize(parent) {
            return Ok(canon_parent.join(name));
        }
    }

    let mut normalized = PathBuf::new();
    for component in abs.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    Ok(normalized)
}

/// 根据 `(filedir, filename)` 或 `filepath` 计算完整的本地文件路径。
///
/// 当同时传入 `filepath` 与 `(filedir, filename)` 时优先使用 `filepath`。
pub fn get_filepath(
    filedir: Option<&Path>,
    filename: Option<&str>,
    filepath: Option<&Path>,
) -> Result<PathBuf> {
    if let Some(fp) = filepath.filter(|p| !p.as_os_str().is_empty()) {
        return normalize_absolute(fp);
    }
    if let (Some(dir), Some(name)) = (
        filedir.filter(|p| !p.as_os_str().is_empty()),
        filename.filter(|s| !s.is_empty()),
    ) {
        return normalize_absolute(&dir.join(name));
    }
    Err(FunPubError::InvalidArgument(
        "Either filepath or (filedir and filename) must be provided".to_string(),
    ))
}

/// 确保 `path` 的父目录存在，并返回 `path` 本身。
pub fn ensure_parent_dir(path: &Path) -> Result<PathBuf> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    Ok(path.to_path_buf())
}

/// 一次上传操作的统一结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublishResult {
    pub path: String,
    pub version: String,
    pub url: Option<String>,
    pub size: Option<u64>,
    pub md5: Option<String>,
    pub sha1: Option<String>,
    pub sha256: Option<String>,
    pub filename: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ext: BTreeMap<String, Value>,
}

impl PublishResult {
    /// 创建一个只填必填字段 `path` 和 `version` 的结果对象。
    pub fn new(path: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            version: version.into(),
            url: None,
            size: None,
            md5: None,
            sha1: None,
            sha256: None,
            filename: None,
            ext: BTreeMap::new(),
        }
    }
}

/// 上传可选参数。
#[derive(Debug, Clone, Default)]
pub struct UploadOptions {
    pub filename: Option<String>,
    pub description: Option<String>,
    pub overwrite: bool,
    pub chunked: Option<bool>,
    pub poll_interval: Option<Duration>,
    pub poll_timeout: Option<Duration>,
}

/// 下载目标与覆盖选项。
#[derive(Debug, Clone, Default)]
pub struct DownloadTarget {
    pub save_dir: Option<PathBuf>,
    pub filename: Option<String>,
    pub filepath: Option<PathBuf>,
    pub overwrite: bool,
}

/// 所有发布渠道实现的统一接口。
pub trait Publisher: Send + Sync {
    /// 上传制品文件并返回统一结果。
    fn upload_file(
        &self,
        filepath: &Path,
        path: &str,
        version: &str,
        options: &UploadOptions,
    ) -> Result<PublishResult>;

    /// 下载制品文件到本地，返回实际写入的文件路径。
    fn download_file(&self, path: &str, version: &str, target: &DownloadTarget) -> Result<PathBuf>;

    /// 检查指定版本的制品是否存在。
    fn exist(&self, path: &str, version: &str) -> Result<bool>;

    /// 获取制品的（临时）免密下载地址。
    fn get_download_url(
        &self,
        path: &str,
        version: &str,
        expiration_ms: Option<u64>,
    ) -> Result<String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_filepath_prefers_explicit_filepath() {
        let dir = tempfile::tempdir().unwrap();
        let explicit = dir.path().join("explicit.bin");
        let resolved = get_filepath(
            Some(dir.path()),
            Some("ignored.bin"),
            Some(explicit.as_path()),
        )
        .unwrap();
        assert_eq!(resolved, normalize_absolute(&explicit).unwrap());
    }

    #[test]
    fn get_filepath_combines_dir_and_name() {
        let dir = tempfile::tempdir().unwrap();
        let resolved = get_filepath(Some(dir.path()), Some("a.bin"), None).unwrap();
        assert_eq!(
            resolved,
            normalize_absolute(&dir.path().join("a.bin")).unwrap()
        );
    }

    #[test]
    fn get_filepath_rejects_missing_args() {
        assert!(matches!(
            get_filepath(None, None, None),
            Err(FunPubError::InvalidArgument(_))
        ));
        assert!(matches!(
            get_filepath(Some(Path::new("/tmp")), None, None),
            Err(FunPubError::InvalidArgument(_))
        ));
        assert!(matches!(
            get_filepath(None, Some("a.bin"), None),
            Err(FunPubError::InvalidArgument(_))
        ));
    }

    #[test]
    fn ensure_parent_dir_handles_nested_and_bare_filename() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("nested").join("dir").join("a.bin");
        assert!(!target.parent().unwrap().exists());
        assert_eq!(ensure_parent_dir(&target).unwrap(), target);
        assert!(target.parent().unwrap().is_dir());

        let bare = Path::new("a.bin");
        assert_eq!(ensure_parent_dir(bare).unwrap(), bare);
    }
}
