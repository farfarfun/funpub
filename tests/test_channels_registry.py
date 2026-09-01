import pytest

from funpub.channels import CHANNEL_SPECS, get_publisher, list_available_channels
from funpub.core.exceptions import ChannelNotFoundError


def test_unknown_channel_raises():
    with pytest.raises(ChannelNotFoundError):
        get_publisher("does-not-exist")


def test_aliyun_registered():
    assert "aliyun" in CHANNEL_SPECS


def test_aliyun_available():
    available = list_available_channels()
    assert "aliyun" in available
