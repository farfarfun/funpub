"""阿里云 Packages 制品仓库（generic 类型）REST 客户端

覆盖官方文档描述的两套能力：

* 单次 API 上传/下载/免密下载地址（见 ``docs/aliyun/common/API上传.md``）。
* 大文件分块上传（见 ``docs/aliyun/common/大文件上传.md``）。协议细节文档
  中没有给出，是从官方 ``chunk_upload.py`` 脚本逆向出来的：创建上传会话
  -> PATCH 分块 -> 触发异步合并 -> 轮询合并状态 -> 将 blob 关联到仓库路径。
"""

import hashlib
import os
import re
import time
from base64 import b64encode
from typing import Any, Dict, Optional, Tuple
from urllib.parse import quote

from farlog import getLogger

from funpub.core.exceptions import (
    AuthenticationError,
    ChecksumMismatchError,
    MergeTimeoutError,
    PublishError,
    RemoteNotFoundError,
)
from funpub.core.http import new_session

logger = getLogger("funpub.aliyun")

DEFAULT_CHUNK_SIZE = 100 * 1024 * 1024  # 100MB，与官方脚本默认值保持一致
DEFAULT_MERGE_POLL_INTERVAL = 3.0  # 秒
DEFAULT_MERGE_TIMEOUT = 20 * 60.0  # 秒

# repo_url 形如 https://packages.aliyun.com/api/protocol/{org_id}/generic/{repo}
_REPO_URL_RE = re.compile(r"^https?://([^/]+)/api/protocol/([^/]+)/[^/]+/([^/?]+)")


def _basic_auth_header(username: str, password: str) -> str:
    token = b64encode(f"{username}:{password}".encode()).decode()
    return f"Basic {token}"


def _file_md5(filepath: str, chunk_size: int = 8 * 1024 * 1024) -> str:
    digest = hashlib.md5()
    with open(filepath, "rb") as f:
        while True:
            chunk = f.read(chunk_size)
            if not chunk:
                break
            digest.update(chunk)
    return digest.hexdigest()


class AliyunClient:
    """阿里云 Packages generic 仓库客户端"""

    def __init__(
        self,
        repo_url: str,
        username: str,
        password: str,
        timeout: float = 60.0,
    ) -> None:
        if not repo_url:
            raise PublishError("repo_url 不能为空")
        if not username or not password:
            raise AuthenticationError("username/password 不能为空")

        self.repo_url = repo_url.rstrip("/")
        self.username = username
        self.password = password

        match = _REPO_URL_RE.match(self.repo_url)
        if not match:
            raise PublishError(
                f"无法解析 repo_url: {repo_url!r}，期望形如 "
                "https://packages.aliyun.com/api/protocol/{org_id}/generic/{repo}"
            )
        self.host, self.org_id, self.repo_id = match.groups()

        self._session = new_session(
            timeout=timeout,
            headers={"Authorization": _basic_auth_header(username, password)},
        )

    def close(self) -> None:
        self._session.close()

    def __enter__(self) -> "AliyunClient":
        return self

    def __exit__(self, *exc: Any) -> None:
        self.close()

    # ------------------------------------------------------------------ #
    # 简单上传/下载（小文件，见 API上传.md）
    # ------------------------------------------------------------------ #

    def _files_url(self, path: str) -> str:
        return f"{self.repo_url}/files/{quote(path.lstrip('/'))}"

    def simple_upload(
        self,
        filepath: str,
        path: str,
        version: str,
        filename: Optional[str] = None,
        description: Optional[str] = None,
    ) -> Dict[str, Any]:
        """一次性上传整个文件，返回响应中的 ``object`` 字段"""
        params: Dict[str, str] = {"version": version}
        if filename:
            params["fileName"] = filename
        if description:
            params["versionDescription"] = description

        with open(filepath, "rb") as fp:
            resp = self._session.post(
                self._files_url(path),
                params=params,
                files={"file": (filename or os.path.basename(filepath), fp)},
            )
        self._raise_for_status(resp, "上传")
        payload = self._safe_json(resp)
        if not payload.get("successful", True):
            raise PublishError(f"上传失败: {payload}")
        return payload.get("object", {})

    def download(
        self,
        path: str,
        version: str,
        dest_path: str,
        chunk_size: int = 8 * 1024 * 1024,
    ) -> None:
        """下载文件到 dest_path"""
        with self._session.get(
            self._files_url(path), params={"version": version}, stream=True
        ) as resp:
            self._raise_for_status(resp, "下载")
            with open(dest_path, "wb") as f:
                for chunk in resp.iter_content(chunk_size=chunk_size):
                    if chunk:
                        f.write(chunk)

    def exist(self, path: str, version: str) -> bool:
        """检查指定版本的制品是否存在"""
        resp = self._session.head(self._files_url(path), params={"version": version})
        if resp.status_code == 404:
            return False
        self._raise_for_status(resp, "查询")
        return True

    def get_signed_download_url(self, path: str, version: str, expiration: int) -> str:
        """获取临时免密下载地址，expiration 为毫秒级过期时间戳"""
        resp = self._session.head(
            self._files_url(path),
            params={"version": version, "signUrl": "true", "expiration": expiration},
        )
        self._raise_for_status(resp, "生成免密下载地址")
        url = resp.headers.get("x-artlab-generic-sign-url")
        if not url:
            raise PublishError("响应中没有 x-artlab-generic-sign-url header")
        return url

    # ------------------------------------------------------------------ #
    # 分块上传（大文件，见 大文件上传.md）
    # ------------------------------------------------------------------ #

    def chunked_upload(
        self,
        filepath: str,
        path: str,
        version: str,
        filename: Optional[str] = None,
        description: Optional[str] = None,
        chunk_size: int = DEFAULT_CHUNK_SIZE,
        poll_interval: float = DEFAULT_MERGE_POLL_INTERVAL,
        poll_timeout: float = DEFAULT_MERGE_TIMEOUT,
    ) -> Dict[str, Any]:
        """分块上传大文件：创建会话 -> 分块 PATCH -> 异步合并 -> 轮询 -> 关联仓库"""
        file_size = os.path.getsize(filepath)
        file_md5 = _file_md5(filepath)
        logger.info(f"开始分块上传 {filepath} ({file_size} bytes, md5={file_md5})")

        upload_uuid, location = self._start_blob_upload()
        self._upload_chunks(location, filepath, file_size, chunk_size)
        self._async_commit(upload_uuid, file_md5)
        self._wait_for_merge(upload_uuid, file_md5, poll_interval, poll_timeout)
        return self._associate(path, version, filename, description, file_md5)

    def _blob_uploads_url(self, suffix: str = "") -> str:
        return (
            f"https://{self.host}/api/external/repo/{self.org_id}"
            f"/GENERIC/repos/{self.repo_id}/blobs/uploads{suffix}"
        )

    def _start_blob_upload(self) -> Tuple[str, str]:
        resp = self._session.post(
            self._blob_uploads_url(), headers={"Content-Length": "0"}
        )
        if resp.status_code not in (201, 202):
            raise PublishError(
                f"创建分块上传会话失败: {resp.status_code} {resp.text[:500]}"
            )
        location = resp.headers.get("location")
        upload_uuid = resp.headers.get("package-upload-uuid")
        if not location or not upload_uuid:
            raise PublishError("创建分块上传会话响应缺少 location/package-upload-uuid")
        if location.startswith("/"):
            location = f"https://{self.host}{location}"
        return upload_uuid, location

    def _upload_chunks(
        self, location: str, filepath: str, file_size: int, chunk_size: int
    ) -> None:
        offset = 0
        with open(filepath, "rb") as f:
            while offset < file_size:
                chunk = f.read(chunk_size)
                if not chunk:
                    break
                start, end = offset, offset + len(chunk) - 1
                resp = self._session.patch(
                    location,
                    data=chunk,
                    headers={
                        "Content-Type": "application/octet-stream",
                        "Content-Range": f"{start}-{end}",
                        "Content-Length": str(len(chunk)),
                    },
                )
                if resp.status_code not in (200, 201, 202, 204):
                    raise PublishError(
                        f"分块上传失败 [{start}-{end}]: {resp.status_code} {resp.text[:500]}"
                    )
                offset = end + 1
                logger.info(f"已上传 {offset}/{file_size} bytes")

    def _async_commit(self, upload_uuid: str, file_md5: str) -> None:
        resp = self._session.post(
            self._blob_uploads_url(f"/{upload_uuid}/async-commit"),
            params={"digest": f"md5:{file_md5}"},
            headers={"Content-Length": "0"},
        )
        if resp.status_code != 202:
            raise PublishError(
                f"触发异步合并失败: {resp.status_code} {resp.text[:500]}"
            )

    def _wait_for_merge(
        self,
        upload_uuid: str,
        file_md5: str,
        poll_interval: float,
        poll_timeout: float,
    ) -> None:
        deadline = time.monotonic() + poll_timeout
        url = self._blob_uploads_url(f"/{upload_uuid}/merge-status")

        while True:
            resp = self._session.get(url)
            if resp.status_code == 200:
                merged_md5 = resp.headers.get("x-blob-checksum-md5")
                if not merged_md5:
                    obj = self._safe_json(resp).get("object", {})
                    merged_md5 = obj.get("md5")
                if merged_md5 and merged_md5.lower() != file_md5.lower():
                    raise ChecksumMismatchError(
                        f"合并后 md5 不一致: 本地={file_md5} 远端={merged_md5}"
                    )
                logger.info("分块合并完成")
                return
            elif resp.status_code == 202:
                status = (
                    self._safe_json(resp).get("object", {}).get("status", "PENDING")
                )
                if status == "FAILED":
                    raise PublishError("分块合并失败")
            elif resp.status_code == 404:
                raise RemoteNotFoundError("分块上传会话不存在")
            elif resp.status_code == 500:
                obj = self._safe_json(resp)
                message = obj.get("object", {}).get("errorMessage") or obj.get(
                    "errorMessage"
                )
                raise PublishError(f"分块合并失败: {message or resp.text[:500]}")
            else:
                raise PublishError(
                    f"查询合并状态失败: {resp.status_code} {resp.text[:500]}"
                )

            if time.monotonic() > deadline:
                raise MergeTimeoutError(f"分块合并轮询超时（{poll_timeout}s）")
            time.sleep(poll_interval)

    def _associate(
        self,
        path: str,
        version: str,
        filename: Optional[str],
        description: Optional[str],
        file_md5: str,
    ) -> Dict[str, Any]:
        params: Dict[str, str] = {"version": version}
        if filename:
            params["fileName"] = filename
        if description:
            params["versionDescription"] = description
        resp = self._session.post(
            self._files_url(path),
            params=params,
            headers={
                "x-artlab-checksum-upload": "true",
                "x-artlab-checksum-md5": file_md5,
            },
        )
        if resp.status_code not in (200, 201):
            raise PublishError(
                f"关联制品到仓库失败: {resp.status_code} {resp.text[:500]}"
            )
        payload = self._safe_json(resp)
        obj = payload.get("object") or {"fileMd5": file_md5}
        return obj

    @staticmethod
    def _safe_json(resp: Any) -> Dict[str, Any]:
        try:
            return resp.json()
        except ValueError:
            return {}

    @staticmethod
    def _raise_for_status(resp: Any, action: str) -> None:
        if resp.status_code == 401:
            raise AuthenticationError(f"{action}失败: 认证失败(401)")
        if resp.status_code == 404:
            raise RemoteNotFoundError(f"{action}失败: 远端制品不存在(404)")
        if resp.status_code >= 400:
            raise PublishError(f"{action}失败: {resp.status_code} {resp.text[:500]}")


__all__ = ["AliyunClient", "DEFAULT_CHUNK_SIZE"]
