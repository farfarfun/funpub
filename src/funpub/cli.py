"""funpub 命令行工具"""

import os
import time
from pathlib import Path
from typing import Any

import typer
from farlog import configure

from funpub.channels import (
    get_publisher,
    list_available_channels,
    list_missing_channels,
)

app = typer.Typer(help="funpub 多渠道制品发布命令行工具")


@app.callback()
def main() -> None:
    """funpub 命令组。"""
    configure()


def _build_publisher(
    channel: str,
    repo_name: str | None,
    repo_type: str,
    repo_url: str | None,
) -> Any:
    kwargs = {}
    if repo_name:
        kwargs["repo_name"] = repo_name
        kwargs["repo_type"] = repo_type
    if repo_url:
        kwargs["repo_url"] = repo_url
    return get_publisher(channel, **kwargs)


ChannelOption = typer.Option("aliyun", "--channel", "-c", help="发布渠道")
RepoNameOption = typer.Option(
    None,
    "--repo-name",
    "-r",
    help="仓库名称，用于从 funsecret 按 (repo-type, repo-name) 查找 "
    "repo-url/username/password，一个仓库只需用 funsecret write 配置一次",
)
RepoTypeOption = typer.Option("generic", "--repo-type", help="仓库类型，默认 generic")
RepoUrlOption = typer.Option(
    None, "--repo-url", help="仓库地址，未传则按 repo-name/repo-type 从 funsecret 读取"
)
@app.command()
def upload(
    file: Path = typer.Argument(..., help="本地文件路径", exists=True, readable=True),
    path: str = typer.Argument(..., help="远端制品路径"),
    version: str = typer.Option(..., "--version", "-v", help="制品版本号"),
    channel: str = ChannelOption,
    filename: str | None = typer.Option(None, help="制品名称，默认取本地文件名"),
    description: str | None = typer.Option(None, help="版本描述"),
    overwrite: bool = typer.Option(False, help="是否覆盖已存在的同版本制品"),
    repo_name: str | None = RepoNameOption,
    repo_type: str = RepoTypeOption,
    repo_url: str | None = RepoUrlOption,
) -> None:
    """上传制品文件"""
    publisher = _build_publisher(
        channel, repo_name, repo_type, repo_url
    )
    result = publisher.upload_file(
        filepath=str(file),
        path=path,
        version=version,
        filename=filename,
        description=description,
        overwrite=overwrite,
    )
    typer.echo(f"上传成功: {result.get('url')}")


@app.command()
def download(
    path: str = typer.Argument(..., help="远端制品路径"),
    version: str = typer.Option(..., "--version", "-v", help="制品版本号"),
    output: Path = typer.Option(Path("."), "--output", "-o", help="保存目录或文件路径"),
    channel: str = ChannelOption,
    overwrite: bool = typer.Option(False, help="是否覆盖已存在的本地文件"),
    repo_name: str | None = RepoNameOption,
    repo_type: str = RepoTypeOption,
    repo_url: str | None = RepoUrlOption,
) -> None:
    """下载制品文件"""
    publisher = _build_publisher(
        channel, repo_name, repo_type, repo_url
    )
    if output.is_dir() or str(output).endswith(os.sep):
        publisher.download_file(
            path=path, version=version, save_dir=str(output), overwrite=overwrite
        )
    else:
        publisher.download_file(
            path=path, version=version, filepath=str(output), overwrite=overwrite
        )
    typer.echo(f"下载完成: {output}")


@app.command(name="sign-url")
def sign_url(
    path: str = typer.Argument(..., help="远端制品路径"),
    version: str = typer.Option(..., "--version", "-v", help="制品版本号"),
    expiration_seconds: int = typer.Option(3600, help="有效期（秒）"),
    channel: str = ChannelOption,
    repo_name: str | None = RepoNameOption,
    repo_type: str = RepoTypeOption,
    repo_url: str | None = RepoUrlOption,
) -> None:
    """生成临时免密下载地址"""
    publisher = _build_publisher(
        channel, repo_name, repo_type, repo_url
    )
    expiration = int((time.time() + expiration_seconds) * 1000)
    url = publisher.get_download_url(path=path, version=version, expiration=expiration)
    typer.echo(url)


@app.command(name="channels")
def channels() -> None:
    """列出所有已注册的发布渠道及其可用状态"""
    available = list_available_channels()
    missing = list_missing_channels()
    for name in sorted(set(available) | set(missing)):
        status = "available" if name in available else f"missing ({missing[name]})"
        typer.echo(f"{name}: {status}")


if __name__ == "__main__":
    app()
