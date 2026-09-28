from unittest.mock import MagicMock

import pytest

from funpub.channels.aliyun.publisher import AliyunPublisher
from funpub.core.exceptions import PublishError

REPO_URL = "https://packages.aliyun.com/api/protocol/x/generic/y"


def _publisher(monkeypatch, client):
    monkeypatch.setattr(
        "funpub.channels.aliyun.publisher.AliyunClient", lambda **kwargs: client
    )
    return AliyunPublisher(
        repo_url=REPO_URL, username="u", password="p", chunk_size=1024
    )


def test_upload_uses_simple_for_small_file(monkeypatch, tmp_path):
    client = MagicMock()
    client.exist.return_value = False
    client.simple_upload.return_value = {
        "fileMd5": "m",
        "fileSha1": "s1",
        "fileSha256": "s2",
        "fileSize": 4,
        "url": "https://x",
    }
    publisher = _publisher(monkeypatch, client)

    filepath = tmp_path / "small.bin"
    filepath.write_bytes(b"data")

    result = publisher.upload_file(str(filepath), "a/b", "1.0.0")
    client.simple_upload.assert_called_once()
    client.chunked_upload.assert_not_called()
    assert result.url == "https://x"
    assert result.md5 == "m"


def test_upload_uses_chunked_for_large_file(monkeypatch, tmp_path):
    client = MagicMock()
    client.exist.return_value = False
    client.chunked_upload.return_value = {"fileMd5": "m"}
    publisher = _publisher(monkeypatch, client)

    filepath = tmp_path / "big.bin"
    filepath.write_bytes(b"x" * 2000)

    publisher.upload_file(str(filepath), "a/b", "1.0.0")
    client.chunked_upload.assert_called_once()
    client.simple_upload.assert_not_called()


def test_upload_rejects_existing_without_overwrite(monkeypatch, tmp_path):
    client = MagicMock()
    client.exist.return_value = True
    publisher = _publisher(monkeypatch, client)

    filepath = tmp_path / "a.bin"
    filepath.write_bytes(b"x")

    with pytest.raises(FileExistsError):
        publisher.upload_file(str(filepath), "a/b", "1.0.0")


def test_upload_missing_local_file_raises(monkeypatch):
    client = MagicMock()
    publisher = _publisher(monkeypatch, client)
    with pytest.raises(FileNotFoundError):
        publisher.upload_file("/no/such/file", "a/b", "1.0.0")


def test_get_download_url_default_expiration(monkeypatch):
    client = MagicMock()
    client.get_signed_download_url.return_value = "https://signed"
    publisher = _publisher(monkeypatch, client)

    url = publisher.get_download_url("a/b", "1.0.0")
    assert url == "https://signed"
    client.get_signed_download_url.assert_called_once()
    _, _, expiration = client.get_signed_download_url.call_args[0]
    assert expiration > 0


def test_download_file_writes_to_save_dir(monkeypatch, tmp_path):
    client = MagicMock()
    publisher = _publisher(monkeypatch, client)

    assert publisher.download_file("a/b", "1.0.0", save_dir=str(tmp_path)) is True

    client.download.assert_called_once_with("a/b", "1.0.0", str(tmp_path / "b"))


def test_download_file_rejects_existing_destination(monkeypatch, tmp_path):
    client = MagicMock()
    publisher = _publisher(monkeypatch, client)
    destination = tmp_path / "b"
    destination.write_bytes(b"old")

    with pytest.raises(FileExistsError):
        publisher.download_file("a/b", "1.0.0", filepath=str(destination))
    client.download.assert_not_called()


def test_exist_delegates_to_client(monkeypatch):
    client = MagicMock()
    client.exist.return_value = True
    publisher = _publisher(monkeypatch, client)

    assert publisher.exist("a/b", "1.0.0") is True
    client.exist.assert_called_once_with("a/b", "1.0.0")


def test_get_download_url_uses_explicit_expiration(monkeypatch):
    client = MagicMock()
    client.get_signed_download_url.return_value = "https://signed"
    publisher = _publisher(monkeypatch, client)

    assert (
        publisher.get_download_url("a/b", "1.0.0", expiration=123) == "https://signed"
    )
    client.get_signed_download_url.assert_called_once_with("a/b", "1.0.0", 123)


def test_close_closes_client(monkeypatch):
    client = MagicMock()
    publisher = _publisher(monkeypatch, client)

    publisher.close()

    client.close.assert_called_once_with()


def test_credentials_looked_up_by_repo_type_and_repo_name(monkeypatch):
    monkeypatch.setattr(
        "funpub.channels.aliyun.publisher.AliyunClient", lambda **kwargs: MagicMock()
    )
    calls = []

    def fake_read_secret(**kwargs):
        calls.append(kwargs)
        return {
            "repo_url": REPO_URL,
            "username": "u",
            "password": "p",
        }[kwargs["cate5"]]

    monkeypatch.setattr(
        "funpub.channels.aliyun.publisher.read_secret", fake_read_secret
    )

    AliyunPublisher(repo_name="funpackage")

    for field in ("repo_url", "username", "password"):
        assert {
            "cate1": "funpub",
            "cate2": "aliyun",
            "cate3": "generic",
            "cate4": "funpackage",
            "cate5": field,
        } in calls


def test_explicit_credentials_skip_secret_lookup(monkeypatch):
    monkeypatch.setattr(
        "funpub.channels.aliyun.publisher.AliyunClient", lambda **kwargs: MagicMock()
    )

    def fail_read_secret(**kwargs):
        raise AssertionError("read_secret 不应被调用")

    monkeypatch.setattr(
        "funpub.channels.aliyun.publisher.read_secret", fail_read_secret
    )

    AliyunPublisher(repo_url=REPO_URL, username="u", password="p")


def test_missing_repo_url_and_repo_name_raises(monkeypatch):
    monkeypatch.setattr(
        "funpub.channels.aliyun.publisher.AliyunClient", lambda **kwargs: MagicMock()
    )
    monkeypatch.setattr(
        "funpub.channels.aliyun.publisher.read_secret", lambda **kwargs: None
    )

    with pytest.raises(PublishError):
        AliyunPublisher(username="u", password="p")
