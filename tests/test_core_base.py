import os

import pytest

from funpub.core.base import (
    BasePublisher,
    PublishResult,
    ensure_parent_dir,
    get_filepath,
)


def test_get_filepath_uses_filepath_when_given(tmp_path):
    target = tmp_path / "a.bin"
    result = get_filepath(filepath=str(target))
    assert result == str(target.resolve())


def test_get_filepath_combines_filedir_and_filename(tmp_path):
    result = get_filepath(filedir=str(tmp_path), filename="a.bin")
    assert result == str((tmp_path / "a.bin").resolve())


def test_get_filepath_prefers_filepath_over_filedir(tmp_path):
    explicit = tmp_path / "explicit.bin"
    result = get_filepath(
        filedir=str(tmp_path), filename="ignored.bin", filepath=str(explicit)
    )
    assert result == str(explicit.resolve())


def test_get_filepath_raises_without_enough_arguments():
    with pytest.raises(ValueError):
        get_filepath()
    with pytest.raises(ValueError):
        get_filepath(filedir="/tmp")
    with pytest.raises(ValueError):
        get_filepath(filename="a.bin")


def test_ensure_parent_dir_creates_missing_parent(tmp_path):
    target = tmp_path / "nested" / "dir" / "a.bin"
    assert not target.parent.exists()

    result = ensure_parent_dir(str(target))

    assert result == str(target)
    assert target.parent.is_dir()


def test_ensure_parent_dir_noop_for_bare_filename():
    # 没有目录部分时不应抛异常，也不应尝试创建当前目录
    assert ensure_parent_dir("a.bin") == "a.bin"


def test_ensure_parent_dir_idempotent_when_parent_exists(tmp_path):
    target = tmp_path / "a.bin"
    ensure_parent_dir(str(target))
    # 第二次调用父目录已存在，不应报错
    assert ensure_parent_dir(str(target)) == str(target)
    assert os.path.isdir(tmp_path)


def test_publish_result_dict_and_attribute_access():
    result = PublishResult(
        path="a/b", version="1.0.0", url="https://x", size=10, md5="m"
    )
    assert result["path"] == "a/b"
    assert result.path == "a/b"
    assert result.version == "1.0.0"
    assert result.url == "https://x"
    assert result.size == 10
    assert result.md5 == "m"
    # 未显式传入的可选字段默认为 None
    assert result.sha1 is None
    assert result.sha256 is None
    assert result.filename is None


def test_publish_result_ext_and_kwargs_merge_into_dict():
    result = PublishResult(
        path="a/b",
        version="1.0.0",
        ext={"extra_a": 1},
        extra_b=2,
    )
    assert result["extra_a"] == 1
    assert result.extra_b == 2


def test_publish_result_missing_attribute_raises_attribute_error():
    result = PublishResult(path="a/b", version="1.0.0")
    with pytest.raises(AttributeError):
        _ = result.does_not_exist


def test_publish_result_setattr_writes_into_dict():
    result = PublishResult(path="a/b", version="1.0.0")
    result.custom = "value"
    assert result["custom"] == "value"


def test_publish_result_repr_contains_key_fields():
    result = PublishResult(path="a/b", version="1.0.0", url="https://x")
    text = repr(result)
    assert "a/b" in text
    assert "1.0.0" in text
    assert "https://x" in text


def test_base_publisher_methods_raise_not_implemented(tmp_path):
    publisher = BasePublisher()

    with pytest.raises(NotImplementedError):
        publisher.upload_file(str(tmp_path / "a.bin"), "a/b", "1.0.0")
    with pytest.raises(NotImplementedError):
        publisher.download_file("a/b", "1.0.0")
    with pytest.raises(NotImplementedError):
        publisher.exist("a/b", "1.0.0")
    with pytest.raises(NotImplementedError):
        publisher.get_download_url("a/b", "1.0.0")
