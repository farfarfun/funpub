"""共享 HTTP 会话工厂

提供一个统一入口，让各渠道客户端复用同一套默认超时 + 重试 + 连接池配置，
而不是各写各的 ``requests`` 用法。
"""

from typing import Any

import requests
from requests.adapters import HTTPAdapter
from urllib3.util.retry import Retry

DEFAULT_TIMEOUT = 60.0
DEFAULT_MAX_RETRIES = 3
DEFAULT_RETRY_DELAY = 0.5
DEFAULT_POOL_SIZE = 10

TimeoutValue = float | tuple[float, float]

# 幂等方法才自动重试；POST/PATCH 不重试，避免分块上传/异步合并被重复提交
_RETRY_METHODS = frozenset(["HEAD", "GET", "PUT", "DELETE", "OPTIONS", "TRACE"])
_RETRY_STATUS = (429, 500, 502, 503, 504)


def _validate_timeout(timeout: TimeoutValue) -> None:
    if isinstance(timeout, tuple):
        if len(timeout) != 2 or any(t <= 0 for t in timeout):
            raise ValueError("timeout 元组必须包含两个大于 0 的数值")
    elif timeout <= 0:
        raise ValueError("timeout 必须大于 0")


class TimeoutSession(requests.Session):
    """带会话级默认超时的 :class:`requests.Session`。

    调用点显式传非 ``None`` 的 ``timeout`` 时以调用点为准；不传或传 ``None``
    则使用 ``self.timeout``。
    """

    def __init__(self, timeout: TimeoutValue = DEFAULT_TIMEOUT) -> None:
        _validate_timeout(timeout)
        super().__init__()
        self.timeout = timeout

    def request(  # type: ignore[override]
        self, method: str, url: str | bytes, **kwargs: Any
    ) -> requests.Response:
        """发送 HTTP 请求，未传 ``timeout`` 或传 ``None`` 时使用会话默认值。

        Args:
            method: HTTP 请求方法。
            url: 请求地址。
            **kwargs: 传给 :meth:`requests.Session.request` 的其余参数；其中
                非 ``None`` 的 ``timeout`` 会覆盖会话默认超时。

        Returns:
            :class:`requests.Response` 响应对象。
        """
        if kwargs.get("timeout") is None:
            kwargs["timeout"] = self.timeout
        return super().request(method, url, **kwargs)


def new_session(
    timeout: TimeoutValue = DEFAULT_TIMEOUT,
    retries: int = DEFAULT_MAX_RETRIES,
    backoff_factor: float = DEFAULT_RETRY_DELAY,
    pool_size: int = DEFAULT_POOL_SIZE,
    headers: dict[str, str] | None = None,
) -> TimeoutSession:
    """构造一个带默认超时、幂等请求自动重试、连接池的会话。

    Args:
        timeout: 默认超时（秒），支持单个数值或 ``(connect, read)`` 二元组，调用点可覆盖。
        retries: 幂等请求的重试次数；0 表示不重试。
        backoff_factor: 重试退避因子。
        pool_size: 连接池大小。
        headers: 需要附加到每个请求的默认 header（如 Authorization）。
    """
    _validate_timeout(timeout)
    if retries < 0:
        raise ValueError("retries 不能小于 0")
    if backoff_factor < 0:
        raise ValueError("backoff_factor 不能小于 0")
    if pool_size <= 0:
        raise ValueError("pool_size 必须大于 0")

    session = TimeoutSession(timeout=timeout)

    retry = Retry(
        total=retries,
        connect=retries,
        read=False,
        status=retries,
        backoff_factor=backoff_factor,
        status_forcelist=_RETRY_STATUS,
        allowed_methods=_RETRY_METHODS,
        raise_on_status=False,
        respect_retry_after_header=True,
    )
    adapter = HTTPAdapter(
        max_retries=retry, pool_connections=pool_size, pool_maxsize=pool_size
    )
    session.mount("http://", adapter)
    session.mount("https://", adapter)

    if headers:
        session.headers.update(headers)
    return session


__all__ = ["TimeoutSession", "new_session", "DEFAULT_TIMEOUT"]
