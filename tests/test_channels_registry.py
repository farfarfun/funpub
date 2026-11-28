import pytest

from funpub import (
    AliyunPublisher,
    ChannelNotFoundError,
    get_publisher,
    list_available_channels,
    list_missing_channels,
)
from funpub.channels import CHANNEL_SPECS, ChannelSpec

REPO_URL = "https://packages.aliyun.com/api/protocol/org123/generic/funpackage"


def test_unknown_channel_raises():
    with pytest.raises(ChannelNotFoundError):
        get_publisher("does-not-exist")


def test_aliyun_registered():
    assert "aliyun" in CHANNEL_SPECS


def test_aliyun_available_and_no_missing():
    available = list_available_channels()
    assert "aliyun" in available
    assert available["aliyun"] is AliyunPublisher
    assert list_missing_channels() == {}


def test_channel_spec_install_hint():
    assert ChannelSpec(".a", "APublisher").install_hint == "funpub"
    assert ChannelSpec(".a", "APublisher", extra="oss").install_hint == "funpub[oss]"
    assert (
        ChannelSpec(".a", "APublisher", extra="oss", pip_target="oss2").install_hint
        == "oss2"
    )


def test_get_publisher_normalizes_case_and_whitespace():
    pub = get_publisher("  AliYun ", repo_url=REPO_URL, username="u", password="p")
    assert isinstance(pub, AliyunPublisher)
    assert pub.repo_url == REPO_URL
    assert pub.username == "u"
