"""funpub：多渠道制品发布工具。"""

from importlib.metadata import PackageNotFoundError, version

try:
    __version__ = version("funpub")
except PackageNotFoundError:
    __version__ = "0.0.0"

from funpub.channels import (
    get_publisher,
    list_available_channels,
    list_missing_channels,
)
from funpub.core import (
    AuthenticationError,
    BasePublisher,
    ChannelNotFoundError,
    ChecksumMismatchError,
    MergeTimeoutError,
    PublishError,
    PublishResult,
    RemoteNotFoundError,
)

__all__ = [
    "AuthenticationError",
    "BasePublisher",
    "ChannelNotFoundError",
    "ChecksumMismatchError",
    "MergeTimeoutError",
    "PublishError",
    "PublishResult",
    "RemoteNotFoundError",
    "__version__",
    "get_publisher",
    "list_available_channels",
    "list_missing_channels",
]
