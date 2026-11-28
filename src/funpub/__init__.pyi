from collections.abc import ItemsView, Iterator, KeysView, ValuesView
from os import PathLike
from typing import Any, Self

__version__: str
DEFAULT_CHUNK_SIZE: int
DEFAULT_REPO_TYPE: str

TimeoutValue = float | tuple[float, float]

class FunPubError(Exception): ...
class PublishError(FunPubError): ...
class AuthenticationError(PublishError): ...
class RemoteNotFoundError(PublishError): ...

NotFoundError = RemoteNotFoundError

class ChecksumMismatchError(PublishError): ...
class MergeTimeoutError(PublishError): ...
class ChannelNotFoundError(PublishError): ...
class UploadError(PublishError): ...
class DownloadError(PublishError): ...
class HttpError(PublishError): ...
class ConfigError(PublishError): ...

class PublishResult:
    path: str
    version: str
    url: str | None
    size: int | None
    md5: str | None
    sha1: str | None
    sha256: str | None
    filename: str | None

    def __init__(
        self,
        path: str,
        version: str,
        url: str | None = None,
        size: int | None = None,
        md5: str | None = None,
        sha1: str | None = None,
        sha256: str | None = None,
        filename: str | None = None,
        ext: dict[str, Any] | None = None,
        **kwargs: Any,
    ) -> None: ...
    def __getattr__(self, name: str) -> Any: ...
    def __setattr__(self, name: str, value: Any) -> None: ...
    def __delattr__(self, name: str) -> None: ...
    def __getitem__(self, key: str) -> Any: ...
    def __setitem__(self, key: str, value: Any) -> None: ...
    def __delitem__(self, key: str) -> None: ...
    def __contains__(self, key: object) -> bool: ...
    def __len__(self) -> int: ...
    def __iter__(self) -> Iterator[str]: ...
    def get(self, key: str, default: Any = None) -> Any: ...
    def keys(self) -> KeysView[str]: ...
    def values(self) -> ValuesView[Any]: ...
    def items(self) -> ItemsView[str, Any]: ...
    def to_dict(self) -> dict[str, Any]: ...

class BasePublisher:
    def __init__(self, *args: Any, **kwargs: Any) -> None: ...
    def upload_file(
        self,
        filepath: str | PathLike[str],
        path: str,
        version: str,
        filename: str | None = None,
        description: str | None = None,
        overwrite: bool = False,
        *args: Any,
        **kwargs: Any,
    ) -> PublishResult: ...
    def download_file(
        self,
        path: str,
        version: str,
        save_dir: str | PathLike[str] | None = None,
        filename: str | None = None,
        filepath: str | PathLike[str] | None = None,
        overwrite: bool = False,
        *args: Any,
        **kwargs: Any,
    ) -> bool: ...
    def exist(self, path: str, version: str, *args: Any, **kwargs: Any) -> bool: ...
    def get_download_url(
        self,
        path: str,
        version: str,
        expiration: int | None = None,
        *args: Any,
        **kwargs: Any,
    ) -> str: ...
    def close(self) -> None: ...
    def __enter__(self) -> Self: ...
    def __exit__(self, *exc: Any) -> None: ...

class AliyunClient:
    repo_url: str
    username: str
    scheme: str
    host: str
    org_id: str
    repo_id: str

    def __init__(
        self,
        repo_url: str,
        username: str,
        password: str,
        timeout: TimeoutValue | None = None,
    ) -> None: ...
    def simple_upload(
        self,
        filepath: str | PathLike[str],
        path: str,
        version: str,
        filename: str | None = None,
        description: str | None = None,
    ) -> dict[str, Any]: ...
    def download(
        self,
        path: str,
        version: str,
        dest_path: str | PathLike[str],
        chunk_size: int = ...,
    ) -> None: ...
    def exist(self, path: str, version: str) -> bool: ...
    def get_signed_download_url(
        self, path: str, version: str, expiration: int
    ) -> str: ...
    def chunked_upload(
        self,
        filepath: str | PathLike[str],
        path: str,
        version: str,
        filename: str | None = None,
        description: str | None = None,
        chunk_size: int = ...,
        poll_interval: float = ...,
        poll_timeout: float = ...,
    ) -> dict[str, Any]: ...
    def close(self) -> None: ...
    def __enter__(self) -> Self: ...
    def __exit__(self, *exc: Any) -> None: ...

class AliyunPublisher(BasePublisher):
    chunk_size: int
    chunk_threshold: int
    client: AliyunClient
    repo_url: str
    username: str

    def __init__(
        self,
        repo_name: str | None = None,
        repo_type: str = ...,
        repo_url: str | None = None,
        username: str | None = None,
        password: str | None = None,
        chunk_size: int = ...,
        chunk_threshold: int | None = None,
        timeout: TimeoutValue | None = None,
        *args: Any,
        **kwargs: Any,
    ) -> None: ...
    def upload_file(
        self,
        filepath: str | PathLike[str],
        path: str,
        version: str,
        filename: str | None = None,
        description: str | None = None,
        overwrite: bool = False,
        chunked: bool | None = None,
        poll_interval: float | None = None,
        poll_timeout: float | None = None,
        *args: Any,
        **kwargs: Any,
    ) -> PublishResult: ...
    def download_file(
        self,
        path: str,
        version: str,
        save_dir: str | PathLike[str] | None = None,
        filename: str | None = None,
        filepath: str | PathLike[str] | None = None,
        overwrite: bool = False,
        *args: Any,
        **kwargs: Any,
    ) -> bool: ...
    def exist(self, path: str, version: str, *args: Any, **kwargs: Any) -> bool: ...
    def get_download_url(
        self,
        path: str,
        version: str,
        expiration: int | None = None,
        *args: Any,
        **kwargs: Any,
    ) -> str: ...
    def close(self) -> None: ...

def get_publisher(channel: str, *args: Any, **kwargs: Any) -> BasePublisher: ...
def list_available_channels() -> dict[str, type[BasePublisher]]: ...
def list_missing_channels() -> dict[str, str]: ...
def get_filepath(
    filedir: str | PathLike[str] | None = None,
    filename: str | None = None,
    filepath: str | PathLike[str] | None = None,
) -> str: ...
def ensure_parent_dir(path: str | PathLike[str]) -> str: ...

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
