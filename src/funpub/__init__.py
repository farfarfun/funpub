"""funpub：多渠道制品发布工具"""

from funpub.channels import (
    get_publisher,
    list_available_channels,
    list_missing_channels,
)
from funpub.core import BasePublisher, PublishResult

__version__ = "0.1.0"

__all__ = [
    "get_publisher",
    "list_available_channels",
    "list_missing_channels",
    "BasePublisher",
    "PublishResult",
    "__version__",
]
