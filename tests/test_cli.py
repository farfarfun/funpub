from pathlib import Path
from unittest.mock import MagicMock

from typer.testing import CliRunner

from funpub import __version__
from funpub.cli import app
from funpub.core.base import BasePublisher, PublishResult
from funpub.core.exceptions import PublishError

runner = CliRunner()


def _make_publisher(**overrides):
    pub = MagicMock(spec=BasePublisher)
    pub.__enter__.return_value = pub
    pub.__exit__.return_value = None
    for key, value in overrides.items():
        setattr(pub, key, value)
    return pub


def test_cli_version():
    result = runner.invoke(app, ["--version"])
    assert result.exit_code == 0
    assert __version__ in result.stdout


def test_cli_channels(monkeypatch):
    monkeypatch.setattr(
        "funpub.cli.list_available_channels", lambda: {"aliyun": object}
    )
    monkeypatch.setattr(
        "funpub.cli.list_missing_channels", lambda: {"oss": "funpub[oss]"}
    )

    result = runner.invoke(app, ["channels"])
    assert result.exit_code == 0
    assert "aliyun: available" in result.stdout
    assert "oss: missing (funpub[oss])" in result.stdout


def test_cli_upload_success_and_closes_publisher(monkeypatch, tmp_path):
    local_file = tmp_path / "pkg.tar.gz"
    local_file.write_bytes(b"payload")

    pub = _make_publisher()
    pub.upload_file.return_value = PublishResult(
        path="a/b", version="1.0.0", url="https://dl.example/pkg.tar.gz"
    )
    monkeypatch.setattr("funpub.cli.get_publisher", lambda ch, **kw: pub)

    result = runner.invoke(
        app,
        [
            "upload",
            str(local_file),
            "a/b",
            "--version",
            "1.0.0",
            "--repo-name",
            "myrepo",
        ],
    )

    assert result.exit_code == 0
    assert "上传成功: https://dl.example/pkg.tar.gz" in result.stdout
    pub.upload_file.assert_called_once_with(
        filepath=str(local_file),
        path="a/b",
        version="1.0.0",
        filename=None,
        description=None,
        overwrite=False,
    )
    pub.__exit__.assert_called_once()


def test_cli_upload_without_url_prints_path_and_version(monkeypatch, tmp_path):
    local_file = tmp_path / "big.bin"
    local_file.write_bytes(b"payload")

    pub = _make_publisher()
    pub.upload_file.return_value = PublishResult(path="a/b", version="2.0.0", url=None)
    monkeypatch.setattr("funpub.cli.get_publisher", lambda ch, **kw: pub)

    result = runner.invoke(app, ["upload", str(local_file), "a/b", "-v", "2.0.0"])

    assert result.exit_code == 0
    assert "上传成功: a/b@2.0.0" in result.stdout


def test_cli_upload_rejects_directory(tmp_path):
    result = runner.invoke(app, ["upload", str(tmp_path), "a/b", "-v", "1.0.0"])
    assert result.exit_code != 0


def test_cli_upload_reports_publish_error_cleanly(monkeypatch, tmp_path):
    local_file = tmp_path / "pkg.bin"
    local_file.write_bytes(b"x")

    pub = _make_publisher()
    pub.upload_file.side_effect = PublishError("远端服务不可用")
    monkeypatch.setattr("funpub.cli.get_publisher", lambda ch, **kw: pub)

    result = runner.invoke(app, ["upload", str(local_file), "a/b", "-v", "1.0.0"])

    assert result.exit_code == 1
    assert "错误: 远端服务不可用" in result.output
    pub.__exit__.assert_called_once()


def test_cli_download_trailing_slash_creates_save_dir_target(monkeypatch, tmp_path):
    pub = _make_publisher()
    pub.download_file.return_value = True
    monkeypatch.setattr("funpub.cli.get_publisher", lambda ch, **kw: pub)

    target_dir = f"{tmp_path / 'newdir'}/"
    result = runner.invoke(
        app, ["download", "a/b/app.tgz", "-v", "1.0.0", "-o", target_dir]
    )

    assert result.exit_code == 0
    pub.download_file.assert_called_once_with(
        path="a/b/app.tgz", version="1.0.0", save_dir=target_dir, overwrite=False
    )
    assert str(Path(target_dir) / "app.tgz") in result.stdout
    pub.__exit__.assert_called_once()


def test_cli_download_explicit_filepath(monkeypatch, tmp_path):
    pub = _make_publisher()
    pub.download_file.return_value = True
    monkeypatch.setattr("funpub.cli.get_publisher", lambda ch, **kw: pub)

    target_file = str(tmp_path / "out.bin")
    result = runner.invoke(
        app,
        ["download", "a/b", "-v", "1.0.0", "-o", target_file, "--overwrite"],
    )

    assert result.exit_code == 0
    pub.download_file.assert_called_once_with(
        path="a/b", version="1.0.0", filepath=target_file, overwrite=True
    )


def test_cli_exist_found_and_not_found(monkeypatch):
    pub = _make_publisher()
    monkeypatch.setattr("funpub.cli.get_publisher", lambda ch, **kw: pub)

    pub.exist.return_value = True
    res_found = runner.invoke(app, ["exist", "a/b", "-v", "1.0.0"])
    assert res_found.exit_code == 0
    assert "存在: a/b@1.0.0" in res_found.stdout

    pub.exist.return_value = False
    res_missing = runner.invoke(app, ["exist", "a/b", "-v", "1.0.0"])
    assert res_missing.exit_code == 1
    assert "不存在: a/b@1.0.0" in res_missing.stdout


def test_cli_sign_url_success_and_validation(monkeypatch):
    pub = _make_publisher()
    pub.get_download_url.return_value = "https://signed.example/a"
    monkeypatch.setattr("funpub.cli.get_publisher", lambda ch, **kw: pub)

    ok = runner.invoke(
        app, ["sign-url", "a/b", "-v", "1.0.0", "--expiration-seconds", "120"]
    )
    assert ok.exit_code == 0
    assert "https://signed.example/a" in ok.stdout
    pub.__exit__.assert_called_once()

    bad = runner.invoke(
        app, ["sign-url", "a/b", "-v", "1.0.0", "--expiration-seconds", "0"]
    )
    assert bad.exit_code != 0
