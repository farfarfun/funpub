import hashlib
import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

import pytest

from funpub import (
    AliyunClient,
    AuthenticationError,
    ChecksumMismatchError,
    MergeTimeoutError,
    PublishError,
    RemoteNotFoundError,
)

REPO_URL = "https://packages.aliyun.com/api/protocol/5fc5eb115dbd287006145e5f/generic/funpackage"
DEVOPS_REPO_URL = (
    "https://farfarfun-cn-hangzhou.devops.aliyuncs.com"
    "/packages/api/protocol/generic/funpackage"
)


class _MockServer:
    def __init__(self, handler_fn):
        self.requests: list[dict] = []
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
                status, headers, resp_body = handler_fn(req)
                self.send_response(status)
                for k, v in headers.items():
                    self.send_header(k, v)
                if "Content-Length" not in headers and "content-length" not in {
                    k.lower() for k in headers
                }:
                    self.send_header("Content-Length", str(len(resp_body)))
                self.send_header("Connection", "close")
                self.end_headers()
                if self.command != "HEAD" and resp_body:
                    self.wfile.write(resp_body)

            do_GET = _handle
            do_HEAD = _handle
            do_POST = _handle
            do_PATCH = _handle

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        host, port = self.server.server_address
        self.repo_url = f"http://{host}:{port}/api/protocol/org1/generic/repo1"
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


def test_repo_url_parsing():
    client = AliyunClient(repo_url=REPO_URL, username="u", password="p")
    assert client.host == "packages.aliyun.com"
    assert client.org_id == "5fc5eb115dbd287006145e5f"
    assert client.repo_id == "funpackage"


def test_devops_repo_url_parsing():
    client = AliyunClient(repo_url=DEVOPS_REPO_URL, username="u", password="p")
    assert client.host == "farfarfun-cn-hangzhou.devops.aliyuncs.com"
    assert client.org_id == "funpackage"
    assert client.repo_id == "funpackage"


@pytest.mark.parametrize(
    "bad_url",
    [
        "https://example.com/not-aliyun",
        f"{REPO_URL}/extra",
        f"{REPO_URL}?foo=bar",
        f"{DEVOPS_REPO_URL}/extra",
    ],
)
def test_invalid_repo_url_raises(bad_url):
    with pytest.raises(PublishError):
        AliyunClient(repo_url=bad_url, username="u", password="p")


def test_missing_credentials_raise():
    with pytest.raises(AuthenticationError):
        AliyunClient(repo_url=REPO_URL, username="", password="")


def test_client_repr_does_not_leak_password():
    client = AliyunClient(repo_url=REPO_URL, username="alice", password="top-secret")
    text = repr(client)
    assert "alice" in text
    assert "top-secret" not in text


def test_simple_upload_success_and_errors(tmp_path):
    responses = [
        (
            200,
            {"Content-Type": "application/json"},
            json.dumps(
                {
                    "successful": True,
                    "object": {"fileMd5": "abc", "fileSize": 4, "url": "https://x/y"},
                }
            ).encode(),
        ),
        (404, {}, b"not found"),
        (
            200,
            {"Content-Type": "application/json"},
            json.dumps({"successful": False, "errorMessage": "bad"}).encode(),
        ),
        (
            200,
            {"Content-Type": "application/json"},
            json.dumps({"successful": True, "object": None}).encode(),
        ),
    ]
    idx = 0

    def handle(_req):
        nonlocal idx
        res = responses[idx]
        idx += 1
        return res

    filepath = tmp_path / "a.txt"
    filepath.write_bytes(b"data")

    with _MockServer(handle) as srv:
        client = AliyunClient(srv.repo_url, "u", "p")
        obj = client.simple_upload(str(filepath), "a/b/c", "1.0.0", filename="out.txt")
        assert obj["fileMd5"] == "abc"
        assert srv.requests[0]["query"]["fileName"] == ["out.txt"]

        with pytest.raises(RemoteNotFoundError):
            client.simple_upload(str(filepath), "a/b/c", "1.0.0")

        with pytest.raises(PublishError):
            client.simple_upload(str(filepath), "a/b", "1.0.0")

        assert client.simple_upload(str(filepath), "a/b", "1.0.0") == {}


def test_exist_download_and_signed_url(tmp_path):
    def handle(req):
        if req["method"] == "HEAD" and "signUrl" in req["query"]:
            if req["path"].endswith("/missing-header"):
                return (200, {}, b"")
            return (200, {"x-artlab-generic-sign-url": "https://signed"}, b"")
        if req["method"] == "HEAD":
            if req["path"].endswith("/exists"):
                return (200, {}, b"")
            return (404, {}, b"")
        if req["method"] == "GET":
            return (200, {}, b"helloworld")
        return (400, {}, b"")

    with _MockServer(handle) as srv:
        client = AliyunClient(srv.repo_url, "u", "p")
        assert client.exist("a/exists", "1.0.0") is True
        assert client.exist("a/missing", "1.0.0") is False

        with pytest.raises(ValueError, match="path"):
            client.exist("///", "1.0.0")

        dest = tmp_path / "download.bin"
        client.download("a/exists", "1.0.0", str(dest))
        assert dest.read_bytes() == b"helloworld"

        assert (
            client.get_signed_download_url("a/exists", "1.0.0", 1234567890)
            == "https://signed"
        )
        with pytest.raises(PublishError, match="x-artlab-generic-sign-url"):
            client.get_signed_download_url("a/missing-header", "1.0.0", 1234567890)


def test_chunked_upload_flow_and_errors(tmp_path):
    payload = b"x" * 10
    expected_md5 = hashlib.md5(payload).hexdigest()
    filepath = tmp_path / "big.bin"
    filepath.write_bytes(payload)

    mode = {"case": "ok", "merge_calls": 0}

    def handle(req):
        case = mode["case"]
        if req["method"] == "POST" and req["path"].endswith("/blobs/uploads"):
            if case == "401":
                return (401, {}, b"unauthorized")
            return (
                201,
                {"location": "/uploads/xyz", "package-upload-uuid": "xyz"},
                b"",
            )
        if req["method"] == "PATCH":
            return (202, {}, b"")
        if req["method"] == "POST" and req["path"].endswith("/async-commit"):
            return (202, {}, b"")
        if req["method"] == "GET" and req["path"].endswith("/merge-status"):
            mode["merge_calls"] += 1
            if case == "ok":
                if mode["merge_calls"] == 1:
                    return (
                        202,
                        {"Content-Type": "application/json"},
                        b'{"object":{"status":"PENDING"}}',
                    )
                return (200, {"x-blob-checksum-md5": expected_md5}, b"{}")
            if case == "mismatch":
                return (200, {"x-blob-checksum-md5": "wrong"}, b"{}")
            if case == "timeout":
                return (
                    202,
                    {"Content-Type": "application/json"},
                    b'{"object":{"status":"PENDING"}}',
                )
        if req["method"] == "POST" and "/files/" in req["path"]:
            return (
                200,
                {"Content-Type": "application/json"},
                json.dumps({"successful": True, "object": {"fileMd5": expected_md5}}).encode(),
            )
        return (500, {}, b"unexpected")

    with _MockServer(handle) as srv:
        client = AliyunClient(srv.repo_url, "u", "p")

        obj = client.chunked_upload(
            str(filepath),
            "a/b",
            "1.0.0",
            chunk_size=4,
            poll_interval=0.01,
            poll_timeout=5.0,
        )
        assert obj["fileMd5"] == expected_md5
        patch_reqs = [r for r in srv.requests if r["method"] == "PATCH"]
        assert len(patch_reqs) == 3

        mode["case"] = "401"
        with pytest.raises(AuthenticationError):
            client.chunked_upload(str(filepath), "a/b", "1.0.0", chunk_size=4)

        mode["case"] = "mismatch"
        with pytest.raises(ChecksumMismatchError):
            client.chunked_upload(
                str(filepath),
                "a/b",
                "1.0.0",
                chunk_size=4,
                poll_interval=0.01,
                poll_timeout=5.0,
            )

        mode["case"] = "timeout"
        with pytest.raises(MergeTimeoutError):
            client.chunked_upload(
                str(filepath),
                "a/b",
                "1.0.0",
                chunk_size=4,
                poll_interval=0.01,
                poll_timeout=0.0,
            )
