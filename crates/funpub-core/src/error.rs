//! funpub 统一错误定义。

use std::path::PathBuf;

use thiserror::Error;

/// funpub 核心错误类型。
#[derive(Debug, Error)]
pub enum FunPubError {
    /// 认证失败（如 401 或缺少凭据）。
    #[error("{0}")]
    AuthenticationError(String),

    /// 远端制品或上传会话不存在（404）。
    #[error("{0}")]
    NotFoundError(String),

    /// 本地计算的校验和与远端返回值不一致。
    #[error("{0}")]
    ChecksumMismatchError(String),

    /// 分块上传合并轮询超时。
    #[error("{0}")]
    MergeTimeoutError(String),

    /// 请求了未注册的发布渠道。
    #[error("{0}")]
    ChannelNotFoundError(String),

    /// 上传阶段错误。
    #[error("{0}")]
    UploadError(String),

    /// 下载阶段错误。
    #[error("{0}")]
    DownloadError(String),

    /// HTTP 请求或非预期状态码错误。
    #[error("{message}")]
    HttpError {
        status: Option<u16>,
        message: String,
    },

    /// 配置错误（如 repo_url 缺失或格式无效）。
    #[error("{0}")]
    ConfigError(String),

    /// 参数非法（对应 Python `ValueError`）。
    #[error("{0}")]
    InvalidArgument(String),

    /// 本地文件不存在（对应 Python `FileNotFoundError`）。
    #[error("文件不存在: {}", .0.display())]
    FileNotFound(PathBuf),

    /// 目标制品或本地文件已存在且未允许覆盖（对应 Python `FileExistsError`）。
    #[error("{0}")]
    FileExists(String),

    /// 目标路径是目录，不能作为文件写入（对应 Python `IsADirectoryError`）。
    #[error("目标路径是目录，不能作为文件写入: {}", .0.display())]
    IsADirectory(PathBuf),

    /// 读取 funsecret 凭据库失败。
    #[error("读取 funsecret 凭据失败: {0}")]
    SecretError(String),

    /// 本地文件系统 I/O 错误。
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl FunPubError {
    /// 构造带状态码的 HTTP 错误。
    pub fn http(status: Option<u16>, message: impl Into<String>) -> Self {
        Self::HttpError {
            status,
            message: message.into(),
        }
    }
}

/// funpub 核心结果类型别名。
pub type Result<T> = std::result::Result<T, FunPubError>;
