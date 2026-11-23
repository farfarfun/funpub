"""阿里云 Packages 制品仓库发布渠道"""

import os
import time
from typing import Any

from farlog import getLogger
from funsecret import read_secret

from funpub.core.base import (
    BasePublisher,
    PublishResult,
    ensure_parent_dir,
    get_filepath,
)
from funpub.core.exceptions import AuthenticationError, PublishError
from funpub.core.http import TimeoutValue

from .client import DEFAULT_CHUNK_SIZE, AliyunClient

logger = getLogger("funpub.aliyun")

DEFAULT_REPO_TYPE = "generic"


class AliyunPublisher(BasePublisher):
    """
    基于阿里云 Packages generic 仓库实现的发布渠道。

    阿里云账号下可以有多个仓库，同一类型（如 generic）下也可以有多个不同
    名字的仓库，因此凭证按 (repo_type, repo_name) 两级区分后存储在
    funsecret 里，一个仓库只需要配置一次：

    .. code-block:: bash

        funsecret write {repo_url} funpub aliyun generic funpackage repo_url
        funsecret write {username}  funpub aliyun generic funpackage username
        funsecret write {password}  funpub aliyun generic funpackage password

    之后只需要传 ``repo_name``（``repo_type`` 默认 ``generic``）即可使用：

    .. code-block:: python

        get_publisher("aliyun", repo_name="funpackage")

    凭证解析顺序：构造参数 > funsecret（按 repo_name/repo_type 查找）。
    底层协议细节参见 ``docs/aliyun/generic/`` 下的官方文档。

    Args:
        repo_name: 仓库名称，用于从 funsecret 中查找该仓库的 repo_url/
            username/password（配合 repo_type 一起作为查找的 key）
        repo_type: 仓库类型，默认 ``generic``
        repo_url: 仓库地址，支持两种格式：经典格式
            ``https://packages.aliyun.com/api/protocol/{org_id}/generic/{repo}``，
            或专属域名格式
            ``https://{组织标识}-{地域标识}.devops.aliyuncs.com/packages/api/protocol/{repo_type}/{repo}``；
            不传则按 repo_name/repo_type 从 funsecret 读取
        username: 认证用户名，不传则按 repo_name/repo_type 从 funsecret 读取
        password: 认证密码，不传则按 repo_name/repo_type 从 funsecret 读取
        chunk_size: 分块上传时每块的大小，默认 100MB
        chunk_threshold: 超过该大小的文件自动走分块上传，默认等于 chunk_size
    """

    def __init__(
        self,
        repo_name: str | None = None,
        repo_type: str = DEFAULT_REPO_TYPE,
        repo_url: str | None = None,
        username: str | None = None,
        password: str | None = None,
        chunk_size: int = DEFAULT_CHUNK_SIZE,
        chunk_threshold: int | None = None,
        timeout: TimeoutValue = 60.0,
        *args: Any,
        **kwargs: Any,
    ) -> None:
        super().__init__(*args, **kwargs)

        if chunk_size <= 0:
            raise ValueError("chunk_size 必须大于 0")
        if chunk_threshold is not None and chunk_threshold < 0:
            raise ValueError("chunk_threshold 不能小于 0")

        def _secret(field: str) -> str | None:
            if not repo_name:
                return None
            return read_secret(
                cate1="funpub",
                cate2="aliyun",
                cate3=repo_type,
                cate4=repo_name,
                cate5=field,
            )

        repo_url = repo_url or _secret("repo_url")
        username = username or _secret("username")
        password = password or _secret("password")

        if not repo_url:
            raise PublishError(
                "必须提供 repo_url，或提供 repo_name 并预先通过 funsecret 配置："
                "funsecret write {repo_url} funpub aliyun "
                f"{repo_type} <repo_name> repo_url"
            )
        if not username or not password:
            target = repo_name or "<repo_name>"
            raise AuthenticationError(
                "缺少认证凭据 username/password，请通过参数传入或通过 funsecret 配置："
                f"funsecret write {{value}} funpub aliyun {repo_type} {target} username|password"
            )

        self.chunk_size = chunk_size
        self.chunk_threshold = (
            chunk_size if chunk_threshold is None else chunk_threshold
        )
        self.client = AliyunClient(
            repo_url=repo_url, username=username, password=password, timeout=timeout
        )

    def upload_file(
        self,
        filepath: str,
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
    ) -> PublishResult:
        """上传制品文件并返回统一格式的结果。"""
        if not os.path.isfile(filepath):
            raise FileNotFoundError(filepath)

        if not overwrite and self.exist(path, version):
            raise FileExistsError(
                f"{path}@{version} 已存在，如需覆盖请传 overwrite=True"
            )

        filename = filename or os.path.basename(filepath)
        file_size = os.path.getsize(filepath)
        use_chunked = (
            chunked if chunked is not None else file_size > self.chunk_threshold
        )

        logger.info(
            f"上传 {filepath} -> {path}@{version} "
            f"({'分块' if use_chunked else '简单'}上传, {file_size} bytes)"
        )
        if use_chunked:
            chunk_kwargs: dict[str, Any] = {
                "filename": filename,
                "description": description,
                "chunk_size": self.chunk_size,
            }
            if poll_interval is not None:
                chunk_kwargs["poll_interval"] = poll_interval
            if poll_timeout is not None:
                chunk_kwargs["poll_timeout"] = poll_timeout
            obj = self.client.chunked_upload(filepath, path, version, **chunk_kwargs)
        else:
            obj = self.client.simple_upload(
                filepath, path, version, filename=filename, description=description
            )

        return PublishResult(
            path=path,
            version=version,
            url=obj.get("url"),
            size=obj.get("fileSize", file_size),
            md5=obj.get("fileMd5"),
            sha1=obj.get("fileSha1"),
            sha256=obj.get("fileSha256"),
            filename=filename,
        )

    def download_file(
        self,
        path: str,
        version: str,
        save_dir: str | None = None,
        filename: str | None = None,
        filepath: str | None = None,
        overwrite: bool = False,
        *args: Any,
        **kwargs: Any,
    ) -> bool:
        """下载制品到本地，目标已存在时默认拒绝覆盖。"""
        dest = get_filepath(
            filedir=save_dir,
            filename=filename or os.path.basename(path.rstrip("/")),
            filepath=filepath,
        )
        if os.path.isdir(dest):
            raise IsADirectoryError(f"目标路径是目录，不能作为文件写入: {dest}")
        if os.path.exists(dest) and not overwrite:
            raise FileExistsError(f"{dest} 已存在，如需覆盖请传 overwrite=True")
        ensure_parent_dir(dest)
        logger.info(f"下载 {path}@{version} -> {dest}")
        self.client.download(path, version, dest)
        return True

    def exist(self, path: str, version: str, *args: Any, **kwargs: Any) -> bool:
        """检查远端制品版本是否存在。"""
        return self.client.exist(path, version)

    def get_download_url(
        self,
        path: str,
        version: str,
        expiration: int | None = None,
        *args: Any,
        **kwargs: Any,
    ) -> str:
        """生成临时免密下载地址，默认有效期为一小时。"""
        if expiration is None:
            expiration = int((time.time() + 3600) * 1000)
        return self.client.get_signed_download_url(path, version, expiration)

    def close(self) -> None:
        self.client.close()


__all__ = ["AliyunPublisher"]
