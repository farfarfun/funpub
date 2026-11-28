"""funpub 统一异常定义（从 Rust 扩展再导出）。"""

from funpub._funpub import (
    AuthenticationError,
    ChannelNotFoundError,
    ChecksumMismatchError,
    ConfigError,
    DownloadError,
    FunPubError,
    HttpError,
    MergeTimeoutError,
    NotFoundError,
    PublishError,
    RemoteNotFoundError,
    UploadError,
)

__all__ = [
    "AuthenticationError",
    "ChannelNotFoundError",
    "ChecksumMismatchError",
    "ConfigError",
    "DownloadError",
    "FunPubError",
    "HttpError",
    "MergeTimeoutError",
    "NotFoundError",
    "PublishError",
    "RemoteNotFoundError",
    "UploadError",
]
