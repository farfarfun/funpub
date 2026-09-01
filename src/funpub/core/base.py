"""funpub 发布渠道的通用接口定义"""

import os
from pathlib import Path
from typing import Any, Dict, Optional


def get_filepath(
    filedir: Optional[str] = None,
    filename: Optional[str] = None,
    filepath: Optional[str] = None,
) -> str:
    """根据 (filedir, filename) 或 filepath 计算出完整的本地文件路径"""
    if filepath:
        return str(Path(filepath).resolve())
    elif filedir and filename:
        return str(Path(filedir).joinpath(filename).resolve())
    raise ValueError("Either filepath or (filedir and filename) must be provided")


def ensure_parent_dir(path: str) -> str:
    """确保 path 的父目录存在，返回 path 本身，方便链式调用"""
    parent = os.path.dirname(path)
    if parent:
        os.makedirs(parent, exist_ok=True)
    return path


class PublishResult(dict):
    """
    一次上传操作的结果。继承自 dict，支持字典式和属性式访问。

    基础字段：path、version、url、size、md5、sha1、sha256、filename。
    各渠道可以通过 ext 或额外 kwargs 附加渠道特有的信息。
    """

    def __init__(
        self,
        path: str,
        version: str,
        url: Optional[str] = None,
        size: Optional[int] = None,
        md5: Optional[str] = None,
        sha1: Optional[str] = None,
        sha256: Optional[str] = None,
        filename: Optional[str] = None,
        ext: Optional[Dict[str, Any]] = None,
        **kwargs: Any,
    ) -> None:
        base = {
            "path": path,
            "version": version,
            "url": url,
            "size": size,
            "md5": md5,
            "sha1": sha1,
            "sha256": sha256,
            "filename": filename,
        }
        if ext:
            base.update(ext)
        base.update(kwargs)
        super().__init__(base)

    def __getattr__(self, name: str) -> Any:
        try:
            return self[name]
        except KeyError:
            raise AttributeError(name) from None

    def __setattr__(self, name: str, value: Any) -> None:
        self[name] = value

    def __repr__(self) -> str:
        return (
            f"PublishResult(path={self.get('path')!r}, "
            f"version={self.get('version')!r}, url={self.get('url')!r})"
        )


class BasePublisher:
    """
    发布渠道基类。

    每个渠道（aliyun / pypi / oss / github-release ...）通过子类实现下列方法，
    对上层（CLI、脚本、CI）暴露统一接口。未实现的方法调用会抛出
    ``NotImplementedError``，方便接入新渠道时明确知道还差什么。
    """

    def __init__(self, *args: Any, **kwargs: Any) -> None:
        super().__init__()

    def upload_file(
        self,
        filepath: str,
        path: str,
        version: str,
        filename: Optional[str] = None,
        description: Optional[str] = None,
        overwrite: bool = False,
        *args: Any,
        **kwargs: Any,
    ) -> PublishResult:
        """
        上传制品文件。

        Args:
            filepath: 本地文件路径
            path: 远端制品路径（渠道内的"包名"）
            version: 制品版本号
            filename: 制品名称，默认取本地文件名
            description: 版本描述
            overwrite: 是否覆盖已存在的同版本制品

        Returns:
            PublishResult: 上传结果，包含远端 url、校验和等信息
        """
        raise NotImplementedError()

    def download_file(
        self,
        path: str,
        version: str,
        save_dir: Optional[str] = None,
        filename: Optional[str] = None,
        filepath: Optional[str] = None,
        overwrite: bool = False,
        *args: Any,
        **kwargs: Any,
    ) -> bool:
        """下载制品文件到本地，返回是否成功"""
        raise NotImplementedError()

    def exist(self, path: str, version: str, *args: Any, **kwargs: Any) -> bool:
        """检查指定版本的制品是否已存在"""
        raise NotImplementedError()

    def get_download_url(
        self,
        path: str,
        version: str,
        expiration: Optional[int] = None,
        *args: Any,
        **kwargs: Any,
    ) -> str:
        """获取制品的（临时）下载地址"""
        raise NotImplementedError()
