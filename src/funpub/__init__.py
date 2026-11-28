"""funpub —— 多渠道制品发布工具（Rust 核心 + PyO3 包装）。

本模块仅做再导出，核心实现在 Rust 扩展 ``funpub._funpub`` 中，Python 侧零运行时依赖。
原生命令行工具通过 apt / brew / cargo / GitHub Release 分发。
"""

from importlib.metadata import PackageNotFoundError, version
from pkgutil import extend_path

__path__ = extend_path(__path__, __name__)

try:
    __version__ = version("funpub")
except PackageNotFoundError:
    __version__ = "0.0.0"

from ._funpub import (  # noqa: E402
    DEFAULT_CHUNK_SIZE,
    DEFAULT_REPO_TYPE,
    AliyunClient,
    AliyunPublisher,
    AuthenticationError,
    BasePublisher,
    ChannelNotFoundError,
    ChecksumMismatchError,
    ConfigError,
    DownloadError,
    FunPubError,
    HttpError,
    MergeTimeoutError,
    NotFoundError,
    PublishError,
    PublishResult,
    RemoteNotFoundError,
    UploadError,
    ensure_parent_dir,
    get_filepath,
    get_publisher,
    list_available_channels,
    list_missing_channels,
)

__all__ = [
    "AliyunClient",
    "AliyunPublisher",
    "AuthenticationError",
    "BasePublisher",
    "ChannelNotFoundError",
    "ChecksumMismatchError",
    "ConfigError",
    "DEFAULT_CHUNK_SIZE",
    "DEFAULT_REPO_TYPE",
    "DownloadError",
    "FunPubError",
    "HttpError",
    "MergeTimeoutError",
    "NotFoundError",
    "PublishError",
    "PublishResult",
    "RemoteNotFoundError",
    "UploadError",
    "__version__",
    "ensure_parent_dir",
    "get_filepath",
    "get_publisher",
    "list_available_channels",
    "list_missing_channels",
]
