from unittest.mock import MagicMock

import requests

from funpub.core.http import DEFAULT_TIMEOUT, TimeoutSession, new_session


def test_new_session_returns_timeout_session_with_default_timeout():
    session = new_session()
    assert isinstance(session, TimeoutSession)
    assert session.timeout == DEFAULT_TIMEOUT


def test_new_session_custom_timeout():
    session = new_session(timeout=5.0)
    assert session.timeout == 5.0


def test_new_session_applies_headers():
    session = new_session(headers={"Authorization": "Basic xxx"})
    assert session.headers["Authorization"] == "Basic xxx"


def test_new_session_without_headers_does_not_raise():
    # headers=None 是默认值，不应抛异常或污染默认 header
    session = new_session(headers=None)
    assert "Authorization" not in session.headers


def test_new_session_mounts_adapters_for_http_and_https():
    session = new_session()
    assert isinstance(
        session.get_adapter("http://example.com"), requests.adapters.HTTPAdapter
    )
    assert isinstance(
        session.get_adapter("https://example.com"), requests.adapters.HTTPAdapter
    )


def test_timeout_session_injects_default_timeout_when_not_given(monkeypatch):
    session = TimeoutSession(timeout=12.0)
    captured = {}

    def fake_request(self, method, url, **kwargs):
        captured.update(kwargs)
        return MagicMock()

    monkeypatch.setattr(requests.Session, "request", fake_request)

    session.request("GET", "https://example.com")

    assert captured["timeout"] == 12.0


def test_timeout_session_respects_explicit_timeout(monkeypatch):
    session = TimeoutSession(timeout=12.0)
    captured = {}

    def fake_request(self, method, url, **kwargs):
        captured.update(kwargs)
        return MagicMock()

    monkeypatch.setattr(requests.Session, "request", fake_request)

    session.request("GET", "https://example.com", timeout=1.0)

    assert captured["timeout"] == 1.0


def test_new_session_retry_config_only_retries_idempotent_methods():
    session = new_session(retries=2, pool_size=3)
    adapter = session.get_adapter("https://example.com")
    retry = adapter.max_retries
    assert retry.total == 2
    assert "POST" not in retry.allowed_methods
    assert "GET" in retry.allowed_methods


def test_new_session_zero_retries_disables_retry():
    session = new_session(retries=0)
    adapter = session.get_adapter("https://example.com")
    assert adapter.max_retries.total == 0
