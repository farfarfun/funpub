"""funpub 命令行工具"""

import os
import time
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path

import typer
from farlog import configure

from funpub import __version__
from funpub.channels import (
    get_publisher,
    list_available_channels,
    list_missing_channels,
)
from funpub.core.base import BasePublisher
from funpub.core.exceptions import PublishError

app = typer.Typer(help="funpub 多渠道制品发布命令行工具")

_CLI_ERRORS = (
    PublishError,
    FileExistsError,
    FileNotFoundError,
    IsADirectoryError,
    ValueError,
    ImportError,
)


def _version_callback(value: bool) -> None:
    if value:
        typer.echo(f"funpub {__version__}")
        raise typer.Exit()


@app.callback()
def main(
    version: bool = typer.Option(
        False,
        "--version",
        "-V",
        callback=_version_callback,
        is_eager=True,
        help="显示版本号并退出",
    ),
) -> None:
    """funpub 命令组。"""
    configure()


@contextmanager
def _handle_cli_errors() -> Iterator[None]:
    try:
        yield
    except _CLI_ERRORS as exc:
        typer.echo(f"错误: {exc}", err=True)
        raise typer.Exit(code=1) from exc


def _build_publisher(
    channel: str,
    repo_name: str | None,
    repo_type: str,
    repo_url: str | None,
) -> BasePublisher:
    kwargs: dict[str, str] = {}
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
    file: Path = typer.Argument(
        ...,
        help="本地文件路径",
        exists=True,
        file_okay=True,
        dir_okay=False,
        readable=True,
    ),
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
    """上传制品文件。

    Args:
        file: 本地文件路径，必须存在且可读。
        path: 远端制品路径。
        version: 制品版本号。
        channel: 发布渠道，默认 ``aliyun``。
        filename: 制品名称，默认取本地文件名。
        description: 版本描述。
        overwrite: 是否覆盖已存在的同版本制品，默认不覆盖。
        repo_name: 仓库名称，配合 repo_type 从 funsecret 读取仓库配置。
        repo_type: 仓库类型，默认 ``generic``。
        repo_url: 仓库地址，未传则按 repo_name/repo-type 从 funsecret 读取。

    失败行为：目标版本已存在且未传 ``--overwrite``、本地文件不存在、或
    渠道鉴权/网络失败时向 stderr 输出错误并以非 0 退出码结束。
    """
    with (
        _handle_cli_errors(),
        _build_publisher(channel, repo_name, repo_type, repo_url) as publisher,
    ):
        result = publisher.upload_file(
            filepath=str(file),
            path=path,
            version=version,
            filename=filename,
            description=description,
            overwrite=overwrite,
        )
    url = result.get("url")
    typer.echo(f"上传成功: {url}" if url else f"上传成功: {path}@{version}")


@app.command()
def download(
    path: str = typer.Argument(..., help="远端制品路径"),
    version: str = typer.Option(..., "--version", "-v", help="制品版本号"),
    output: str = typer.Option(".", "--output", "-o", help="保存目录或文件路径"),
    channel: str = ChannelOption,
    overwrite: bool = typer.Option(False, help="是否覆盖已存在的本地文件"),
    repo_name: str | None = RepoNameOption,
    repo_type: str = RepoTypeOption,
    repo_url: str | None = RepoUrlOption,
) -> None:
    """下载制品文件。

    Args:
        path: 远端制品路径。
        version: 制品版本号。
        output: 保存目录（以 ``/`` 结尾或已是目录）或完整文件路径，
            默认当前目录。
        channel: 发布渠道，默认 ``aliyun``。
        overwrite: 是否覆盖已存在的本地文件，默认不覆盖。
        repo_name: 仓库名称，配合 repo_type 从 funsecret 读取仓库配置。
        repo_type: 仓库类型，默认 ``generic``。
        repo_url: 仓库地址，未传则按 repo_name/repo-type 从 funsecret 读取。

    失败行为：本地目标文件已存在且未传 ``--overwrite``、远端制品不存在、
    或渠道鉴权/网络失败时向 stderr 输出错误并以非 0 退出码结束。
    """
    output_path = Path(output)
    is_dir_target = output_path.is_dir() or output.endswith(("/", os.sep))
    with (
        _handle_cli_errors(),
        _build_publisher(channel, repo_name, repo_type, repo_url) as publisher,
    ):
        if is_dir_target:
            publisher.download_file(
                path=path, version=version, save_dir=output, overwrite=overwrite
            )
            saved = output_path / os.path.basename(path.rstrip("/"))
        else:
            publisher.download_file(
                path=path, version=version, filepath=output, overwrite=overwrite
            )
            saved = output_path
    typer.echo(f"下载完成: {saved}")


@app.command()
def exist(
    path: str = typer.Argument(..., help="远端制品路径"),
    version: str = typer.Option(..., "--version", "-v", help="制品版本号"),
    channel: str = ChannelOption,
    repo_name: str | None = RepoNameOption,
    repo_type: str = RepoTypeOption,
    repo_url: str | None = RepoUrlOption,
) -> None:
    """检查指定版本的远端制品是否存在。

    存在时退出码为 0，不存在时退出码为 1。
    """
    with (
        _handle_cli_errors(),
        _build_publisher(channel, repo_name, repo_type, repo_url) as publisher,
    ):
        found = publisher.exist(path=path, version=version)
    if found:
        typer.echo(f"存在: {path}@{version}")
    else:
        typer.echo(f"不存在: {path}@{version}")
        raise typer.Exit(code=1)


@app.command(name="sign-url")
def sign_url(
    path: str = typer.Argument(..., help="远端制品路径"),
    version: str = typer.Option(..., "--version", "-v", help="制品版本号"),
    expiration_seconds: int = typer.Option(3600, min=1, help="有效期（秒）"),
    channel: str = ChannelOption,
    repo_name: str | None = RepoNameOption,
    repo_type: str = RepoTypeOption,
    repo_url: str | None = RepoUrlOption,
) -> None:
    """生成临时免密下载地址。

    Args:
        path: 远端制品路径。
        version: 制品版本号。
        expiration_seconds: 链接从现在起的有效期（秒），默认 3600，必须 >= 1。
        channel: 发布渠道，默认 ``aliyun``。
        repo_name: 仓库名称，配合 repo_type 从 funsecret 读取仓库配置。
        repo_type: 仓库类型，默认 ``generic``。
        repo_url: 仓库地址，未传则按 repo_name/repo-type 从 funsecret 读取。

    失败行为：远端制品不存在或渠道鉴权/网络失败时向 stderr 输出错误并以非 0
    退出码结束。
    """
    with (
        _handle_cli_errors(),
        _build_publisher(channel, repo_name, repo_type, repo_url) as publisher,
    ):
        expiration = int((time.time() + expiration_seconds) * 1000)
        url = publisher.get_download_url(
            path=path, version=version, expiration=expiration
        )
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
