import types

import pytest

from funpub import channels as channels_mod
from funpub.channels import (
    CHANNEL_SPECS,
    ChannelSpec,
    get_publisher,
    list_available_channels,
    list_missing_channels,
)
from funpub.core.exceptions import ChannelNotFoundError


def test_unknown_channel_raises():
    with pytest.raises(ChannelNotFoundError):
        get_publisher("does-not-exist")


def test_aliyun_registered():
    assert "aliyun" in CHANNEL_SPECS


def test_aliyun_available():
    available = list_available_channels()
    assert "aliyun" in available


def test_channel_spec_install_hint():
    assert ChannelSpec(".a", "APublisher").install_hint == "funpub"
    assert ChannelSpec(".a", "APublisher", extra="oss").install_hint == "funpub[oss]"
    assert (
        ChannelSpec(".a", "APublisher", extra="oss", pip_target="oss2").install_hint
        == "oss2"
    )


def test_get_publisher_normalizes_case_and_whitespace(monkeypatch):
    created = {}

    class DummyPub:
        def __init__(self, **kwargs):
            created.update(kwargs)

    monkeypatch.setattr(channels_mod, "_load", lambda spec: DummyPub)
    pub = get_publisher("  AliYun ", foo="bar")
    assert isinstance(pub, DummyPub)
    assert created == {"foo": "bar"}


def test_load_missing_external_dependency_shows_install_hint(monkeypatch):
    spec = ChannelSpec(".fake_ext", "FakePublisher", extra="fake")
    monkeypatch.setattr(channels_mod, "_resolved", {})

    def fake_import(name, package=None):
        err = ModuleNotFoundError("No module named 'third_party'")
        err.name = "third_party"
        raise err

    monkeypatch.setattr(channels_mod.importlib, "import_module", fake_import)

    with pytest.raises(ImportError, match=r"pip install funpub\[fake\]"):
        channels_mod._load(spec)


def test_load_missing_internal_module_reports_internal_error(monkeypatch):
    spec = ChannelSpec(".broken_internal", "BrokenPublisher")
    monkeypatch.setattr(channels_mod, "_resolved", {})

    def fake_import(name, package=None):
        err = ModuleNotFoundError("No module named 'funpub.channels.broken_internal'")
        err.name = "funpub.channels.broken_internal"
        raise err

    monkeypatch.setattr(channels_mod.importlib, "import_module", fake_import)

    with pytest.raises(ImportError, match="内部错误"):
        channels_mod._load(spec)


def test_load_missing_class_reports_internal_error(monkeypatch):
    spec = ChannelSpec(".wrong_cls", "MissingPublisher")
    monkeypatch.setattr(channels_mod, "_resolved", {})
    fake_mod = types.SimpleNamespace(RealPublisher=object)
    monkeypatch.setattr(
        channels_mod.importlib, "import_module", lambda name, package=None: fake_mod
    )

    with pytest.raises(ImportError, match="RealPublisher"):
        channels_mod._load(spec)


def test_list_missing_channels_reports_unavailable_channel(monkeypatch):
    specs = {
        "aliyun": CHANNEL_SPECS["aliyun"],
        "oss": ChannelSpec(".oss_missing", "OssPublisher", extra="oss"),
    }
    monkeypatch.setattr(channels_mod, "CHANNEL_SPECS", specs)
    missing = list_missing_channels()
    assert missing == {"oss": "funpub[oss]"}
