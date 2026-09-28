from unittest.mock import MagicMock

import pytest

from funpub.channels.aliyun.client import AliyunClient
from funpub.core.exceptions import (
    AuthenticationError,
    PublishError,
    RemoteNotFoundError,
)

REPO_URL = "https://packages.aliyun.com/api/protocol/5fc5eb115dbd287006145e5f/generic/funpackage"
DEVOPS_REPO_URL = (
    "https://farfarfun-cn-hangzhou.devops.aliyuncs.com"
    "/packages/api/protocol/generic/funpackage"
)


def _client(monkeypatch, session, repo_url=REPO_URL):
    monkeypatch.setattr(
        "funpub.channels.aliyun.client.new_session", lambda **kwargs: session
    )
    return AliyunClient(repo_url=repo_url, username="u", password="p")


def test_repo_url_parsing(monkeypatch):
    client = _client(monkeypatch, MagicMock())
    assert client.host == "packages.aliyun.com"
    assert client.org_id == "5fc5eb115dbd287006145e5f"
    assert client.repo_id == "funpackage"


def test_devops_repo_url_parsing(monkeypatch):
    client = _client(monkeypatch, MagicMock(), repo_url=DEVOPS_REPO_URL)
    assert client.host == "farfarfun-cn-hangzhou.devops.aliyuncs.com"
    assert client.org_id == "funpackage"
    assert client.repo_id == "funpackage"


def test_invalid_repo_url_raises(monkeypatch):
    monkeypatch.setattr(
        "funpub.channels.aliyun.client.new_session", lambda **kwargs: MagicMock()
    )
    with pytest.raises(PublishError):
        AliyunClient(
            repo_url="https://example.com/not-aliyun", username="u", password="p"
        )


def test_missing_credentials_raise(monkeypatch):
    monkeypatch.setattr(
        "funpub.channels.aliyun.client.new_session", lambda **kwargs: MagicMock()
    )
    with pytest.raises(AuthenticationError):
        AliyunClient(repo_url=REPO_URL, username="", password="")


def test_simple_upload_success(monkeypatch, tmp_path):
    session = MagicMock()
    resp = MagicMock(status_code=200)
    resp.json.return_value = {
        "successful": True,
        "object": {"fileMd5": "abc", "fileSize": 4, "url": "https://x/y"},
    }
    session.post.return_value = resp
    client = _client(monkeypatch, session)

    filepath = tmp_path / "a.txt"
    filepath.write_bytes(b"data")

    obj = client.simple_upload(str(filepath), "a/b/c", "1.0.0", filename="out.txt")
    assert obj["fileMd5"] == "abc"
    session.post.assert_called_once()


def test_simple_upload_failure_raises(monkeypatch, tmp_path):
    session = MagicMock()
    resp = MagicMock(status_code=404, text="not found")
    session.post.return_value = resp
    client = _client(monkeypatch, session)

    filepath = tmp_path / "a.txt"
    filepath.write_bytes(b"data")

    with pytest.raises(RemoteNotFoundError):
        client.simple_upload(str(filepath), "a/b/c", "1.0.0")


def test_exist_true_false(monkeypatch):
    session = MagicMock()
    client = _client(monkeypatch, session)

    session.head.return_value = MagicMock(status_code=200)
    assert client.exist("a/b", "1.0.0") is True

    session.head.return_value = MagicMock(status_code=404)
    assert client.exist("a/b", "1.0.0") is False


def test_download_writes_streamed_response(monkeypatch, tmp_path):
    session = MagicMock()
    response = MagicMock(status_code=200)
    response.__enter__.return_value = response
    response.iter_content.return_value = [b"hello", b"", b"world"]
    session.get.return_value = response
    client = _client(monkeypatch, session)

    destination = tmp_path / "download.bin"
    client.download("a/b", "1.0.0", str(destination))

    assert destination.read_bytes() == b"helloworld"
    session.get.assert_called_once_with(
        f"{REPO_URL}/files/a/b", params={"version": "1.0.0"}, stream=True
    )


def test_signed_url(monkeypatch):
    session = MagicMock()
    resp = MagicMock(
        status_code=200, headers={"x-artlab-generic-sign-url": "https://signed"}
    )
    session.head.return_value = resp
    client = _client(monkeypatch, session)

    url = client.get_signed_download_url("a/b", "1.0.0", 1234567890)
    assert url == "https://signed"


def test_chunked_upload_flow(monkeypatch, tmp_path):
    session = MagicMock()

    start_resp = MagicMock(
        status_code=201,
        headers={"location": "/uploads/xyz", "package-upload-uuid": "xyz"},
    )
    commit_resp = MagicMock(status_code=202)
    associate_resp = MagicMock(status_code=200)
    associate_resp.json.return_value = {"object": {"fileMd5": "known"}}

    merge_resp = MagicMock(status_code=200, headers={})
    merge_resp.json.return_value = {}

    session.post.side_effect = [start_resp, commit_resp, associate_resp]
    session.patch.return_value = MagicMock(status_code=202)
    session.get.return_value = merge_resp

    client = _client(monkeypatch, session)
    monkeypatch.setattr(
        "funpub.channels.aliyun.client._file_md5", lambda filepath: "known"
    )

    filepath = tmp_path / "big.bin"
    filepath.write_bytes(b"x" * 10)

    obj = client.chunked_upload(str(filepath), "a/b", "1.0.0", chunk_size=4)
    assert obj["fileMd5"] == "known"
    assert session.patch.call_count == 3  # 4 + 4 + 2 bytes
