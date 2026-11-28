"""funpub 发布渠道注册表（从 Rust 扩展再导出）。"""

from typing import NamedTuple

from funpub._funpub import (
    AliyunPublisher,
    get_publisher,
    list_available_channels,
    list_missing_channels,
)


class ChannelSpec(NamedTuple):
    """一个渠道的描述信息。"""

    module: str
    cls: str
    extra: str | None = None
    pip_target: str | None = None

    @property
    def install_hint(self) -> str:
        """返回安装此渠道所需依赖的 pip 目标字符串。"""
        if self.pip_target:
            return self.pip_target
        if self.extra:
            return f"funpub[{self.extra}]"
        return "funpub"


CHANNEL_SPECS: dict[str, ChannelSpec] = {
    "aliyun": ChannelSpec(".aliyun", "AliyunPublisher"),
}

__all__ = [
    "AliyunPublisher",
    "CHANNEL_SPECS",
    "ChannelSpec",
    "get_publisher",
    "list_available_channels",
    "list_missing_channels",
]
