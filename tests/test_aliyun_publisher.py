import hashlib
import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

import pytest

from funpub import AliyunPublisher, AuthenticationError, PublishError

REPO_URL = "https://packages.aliyun.com/api/protocol/x/generic/y"


class _PublisherServer:
    def __init__(self):
        self.exists = False
        self.requests: list[dict] = []
        self.last_md5 = ""
        outer = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def log_message(self, format, *args):
                return

            def _handle(self):
                length = int(self.headers.get("Content-Length", "0"))
                body = self.rfile.read(length) if length > 0 else b""
                parsed = urlparse(self.path)
                req = {
                    "method": self.command,
                    "path": parsed.path,
                    "query": parse_qs(parsed.query),
                    "headers": {k.lower(): v for k, v in self.headers.items()},
                    "body": body,
                }
                outer.requests.append(req)

                if self.command == "HEAD" and "signUrl" in req["query"]:
                    self._send(200, {"x-artlab-generic-sign-url": "https://signed"}, b"")
                elif self.command == "HEAD":
                    self._send(200 if outer.exists else 404, {}, b"")
                elif self.command == "GET" and "/merge-status" in parsed.path:
                    self._send(200, {"x-blob-checksum-md5": outer.last_md5}, b"{}")
                elif self.command == "GET":
                    self._send(200, {}, b"downloaded-content")
                elif self.command == "POST" and parsed.path.endswith("/blobs/uploads"):
                    self._send(
                        201,
                        {"location": "/uploads/u1", "package-upload-uuid": "u1"},
                        b"",
                    )
                elif self.command == "PATCH":
                    self._send(202, {}, b"")
                elif self.command == "POST" and parsed.path.endswith("/async-commit"):
                    digest = req["query"].get("digest", [""])[0]
                    outer.last_md5 = digest.removeprefix("md5:")
                    self._send(202, {}, b"")
                elif self.command == "POST" and "/files/" in parsed.path:
                    if req["headers"].get("x-artlab-checksum-upload") == "true":
                        payload = {
                            "successful": True,
                            "object": {"fileMd5": outer.last_md5, "url": "https://chunked"},
                        }
                    else:
                        payload = {
                            "successful": True,
                            "object": {
                                "fileMd5": "m",
                                "fileSha1": "s1",
                                "fileSha256": "s2",
                                "fileSize": 4,
                                "url": "https://x",
                            },
                        }
                    self._send(
                        200,
                        {"Content-Type": "application/json"},
                        json.dumps(payload).encode(),
                    )
                else:
                    self._send(404, {}, b"")

            def _send(self, status, headers, body):
                self.send_response(status)
                for k, v in headers.items():
                    self.send_header(k, v)
                self.send_header("Content-Length", str(len(body)))
                self.send_header("Connection", "close")
                self.end_headers()
                if self.command != "HEAD" and body:
                    self.wfile.write(body)

            do_GET = _handle
            do_HEAD = _handle
            do_POST = _handle
            do_PATCH = _handle

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        host, port = self.server.server_address
        self.repo_url = f"http://{host}:{port}/api/protocol/x/generic/y"
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def close(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=2)

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()


def test_upload_uses_simple_for_small_file(tmp_path):
    filepath = tmp_path / "small.bin"
    filepath.write_bytes(b"data")

    with _PublisherServer() as srv:
        publisher = AliyunPublisher(
            repo_url=srv.repo_url, username="u", password="p", chunk_size=1024
        )
        result = publisher.upload_file(str(filepath), "a/b", "1.0.0")
        assert result.url == "https://x"
        assert result.md5 == "m"
        assert result.sha1 == "s1"
        assert result.sha256 == "s2"
        assert result["size"] == 4
        assert not any(r["method"] == "PATCH" for r in srv.requests)


def test_upload_uses_chunked_for_large_file(tmp_path):
    data = b"x" * 2000
    filepath = tmp_path / "big.bin"
    filepath.write_bytes(data)

    with _PublisherServer() as srv:
        publisher = AliyunPublisher(
            repo_url=srv.repo_url, username="u", password="p", chunk_size=1024
        )
        result = publisher.upload_file(
            str(filepath), "a/b", "1.0.0", poll_interval=0.01, poll_timeout=5.0
        )
        assert result.md5 == hashlib.md5(data).hexdigest()
        assert len([r for r in srv.requests if r["method"] == "PATCH"]) == 2


def test_chunk_threshold_zero_triggers_chunked_upload(tmp_path):
    filepath = tmp_path / "tiny.bin"
    filepath.write_bytes(b"a")

    with _PublisherServer() as srv:
        publisher = AliyunPublisher(
            repo_url=srv.repo_url,
            username="u",
            password="p",
            chunk_size=1024,
            chunk_threshold=0,
        )
        assert publisher.chunk_threshold == 0
        publisher.upload_file(
            str(filepath), "a/b", "1.0.0", poll_interval=0.01, poll_timeout=5.0
        )
        assert any(r["method"] == "PATCH" for r in srv.requests)


def test_upload_rejects_existing_without_overwrite(tmp_path):
    filepath = tmp_path / "a.bin"
    filepath.write_bytes(b"x")

    with _PublisherServer() as srv:
        srv.exists = True
        publisher = AliyunPublisher(repo_url=srv.repo_url, username="u", password="p")
        with pytest.raises(FileExistsError):
            publisher.upload_file(str(filepath), "a/b", "1.0.0")

        res = publisher.upload_file(str(filepath), "a/b", "1.0.0", overwrite=True)
        assert res.url == "https://x"


def test_upload_missing_local_file_raises():
    publisher = AliyunPublisher(repo_url=REPO_URL, username="u", password="p")
    with pytest.raises(FileNotFoundError):
        publisher.upload_file("/no/such/file", "a/b", "1.0.0")


def test_download_exist_and_signed_url(tmp_path):
    with _PublisherServer() as srv:
        srv.exists = True
        with AliyunPublisher(
            repo_url=srv.repo_url, username="u", password="p"
        ) as publisher:
            assert publisher.exist("a/b", "1.0.0") is True

            url = publisher.get_download_url("a/b", "1.0.0")
            assert url == "https://signed"

            assert (
                publisher.get_download_url("a/b", "1.0.0", expiration=123)
                == "https://signed"
            )
            assert srv.requests[-1]["query"]["expiration"] == ["123"]

            assert (
                publisher.download_file("a/b/", "1.0.0", save_dir=str(tmp_path)) is True
            )
            dest = tmp_path / "b"
            assert dest.read_bytes() == b"downloaded-content"

            with pytest.raises(FileExistsError):
                publisher.download_file("a/b", "1.0.0", filepath=str(dest))

            with pytest.raises(IsADirectoryError):
                publisher.download_file(
                    "a/b", "1.0.0", filepath=str(tmp_path), overwrite=True
                )


def test_missing_repo_url_and_credentials_raise(monkeypatch, tmp_path):
    monkeypatch.setenv("FUN_SECRET_PATH", str(tmp_path))

    with pytest.raises(PublishError):
        AliyunPublisher(username="u", password="p")

    with pytest.raises(AuthenticationError, match="funsecret"):
        AliyunPublisher(repo_url=REPO_URL, repo_name="myrepo")


def test_invalid_chunk_params_raise_value_error():
    with pytest.raises(ValueError, match="chunk_size"):
        AliyunPublisher(repo_url=REPO_URL, username="u", password="p", chunk_size=0)
    with pytest.raises(ValueError, match="chunk_threshold"):
        AliyunPublisher(
            repo_url=REPO_URL, username="u", password="p", chunk_threshold=-1
        )
