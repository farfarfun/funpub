"""
funpub 发布渠道注册表

渠道懒加载：``import funpub`` 不会导入任何渠道的第三方依赖，只有真正用到
某个渠道时才会导入它。

接入一个新渠道只需要两步：
1. 在 ``funpub/channels/<name>/`` 下实现一个 :class:`funpub.core.BasePublisher`
   子类。
2. 在 :data:`CHANNEL_SPECS` 里注册一行 :class:`ChannelSpec`。
"""

import importlib
from typing import Any, Dict, NamedTuple, Optional, Type

from funpub.core.exceptions import ChannelNotFoundError


class ChannelSpec(NamedTuple):
    """一个渠道的加载说明。"""

    module: str
    """相对模块路径，如 ``.aliyun``。"""

    cls: str
    """模块中导出的 Publisher 类名。"""

    extra: Optional[str] = None
    """安装该渠道所需的 pip extra；``None`` 表示只依赖核心依赖。"""

    pip_target: Optional[str] = None
    """``pip install`` 的目标覆写；默认是 ``funpub[<extra>]``。"""

    @property
    def install_hint(self) -> str:
        if self.pip_target:
            return self.pip_target
        if self.extra:
            return f"funpub[{self.extra}]"
        return "funpub"


# 渠道注册表 —— 唯一的事实来源（渠道 key -> 模块/类名/pip extra）。
CHANNEL_SPECS: Dict[str, ChannelSpec] = {
    "aliyun": ChannelSpec(".aliyun", "AliyunPublisher"),
}

_resolved: Dict[str, Type[Any]] = {}


def _load(spec: ChannelSpec) -> Type[Any]:
    """导入并返回渠道的 Publisher 类。

    Raises:
        ImportError: 依赖缺失（带 pip 提示），或注册表类名有误（视为 bug）。
    """
    cache_key = f"{spec.module}:{spec.cls}"
    if cache_key in _resolved:
        return _resolved[cache_key]

    try:
        module = importlib.import_module(spec.module, __name__)
    except ImportError as exc:
        missing = getattr(exc, "name", None) or "?"
        raise ImportError(
            f"渠道 {spec.cls} 的依赖 {missing!r} 未安装，"
            f"请运行: pip install {spec.install_hint}"
        ) from exc

    try:
        publisher_cls = getattr(module, spec.cls)
    except AttributeError as exc:
        exported = ", ".join(sorted(n for n in vars(module) if n.endswith("Publisher")))
        raise ImportError(
            f"funpub 内部错误：模块 {spec.module} 没有导出 {spec.cls!r}。"
            f"该模块实际导出的渠道类为: {exported or '(无)'}。"
            f"请修正 funpub/channels/__init__.py 中的 CHANNEL_SPECS。"
        ) from exc

    _resolved[cache_key] = publisher_cls
    return publisher_cls


def get_publisher(channel: str, *args: Any, **kwargs: Any) -> Any:
    """
    根据渠道名称获取一个 Publisher 实例。

    Args:
        channel: 渠道名称，如 ``"aliyun"``
        *args, **kwargs: 传递给渠道构造函数的参数

    Returns:
        BasePublisher: 渠道实例

    Raises:
        ChannelNotFoundError: 不支持的渠道
        ImportError: 渠道依赖未安装

    Examples:
        >>> publisher = get_publisher("aliyun", repo_url="...", username="...", password="...")
    """
    key = channel.lower()
    spec = CHANNEL_SPECS.get(key)
    if spec is None:
        available = ", ".join(sorted(CHANNEL_SPECS))
        raise ChannelNotFoundError(
            f"不支持的发布渠道: {channel}. 可用渠道: {available}"
        )

    return _load(spec)(*args, **kwargs)


def list_available_channels() -> Dict[str, Type[Any]]:
    """列出当前环境中依赖已装好、可以直接实例化的渠道"""
    result: Dict[str, Type[Any]] = {}
    for key, spec in CHANNEL_SPECS.items():
        try:
            result[key] = _load(spec)
        except ImportError:
            continue
    return result


def list_missing_channels() -> Dict[str, str]:
    """列出依赖缺失的渠道及其安装命令"""
    missing: Dict[str, str] = {}
    for key, spec in CHANNEL_SPECS.items():
        try:
            _load(spec)
        except ImportError:
            missing[key] = spec.install_hint
    return missing


__all__ = [
    "get_publisher",
    "list_available_channels",
    "list_missing_channels",
    "CHANNEL_SPECS",
    "ChannelSpec",
]
