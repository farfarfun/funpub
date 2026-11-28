import importlib.metadata as metadata
import os
import subprocess
from pathlib import Path

import pytest

from funpub import __version__

ROOT = Path(__file__).resolve().parent.parent


def _find_cli_binary() -> Path:
    if explicit := os.environ.get("FUNPUB_BIN"):
        p = Path(explicit)
        if p.is_file():
            return p
    target_dirs: list[Path] = []
    if env_target := os.environ.get("CARGO_TARGET_DIR"):
        target_dirs.append(Path(env_target))
    target_dirs.append(ROOT / "target")
    try:
        meta = subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        if meta.returncode == 0:
            import json

            td = json.loads(meta.stdout).get("target_directory")
            if td:
                target_dirs.append(Path(td))
    except OSError:
        pass
    for base in target_dirs:
        for profile in ("release", "debug"):
            candidate = base / profile / "funpub"
            if candidate.is_file():
                return candidate
    pytest.skip("funpub Rust CLI binary not built yet")


def test_wheel_has_zero_dependencies_and_no_console_scripts():
    dist = metadata.distribution("funpub")
    assert not dist.requires, f"expected zero runtime deps, got {dist.requires}"
    console_scripts = [
        ep for ep in dist.entry_points if ep.group == "console_scripts"
    ]
    assert not console_scripts, f"expected no console_scripts, got {console_scripts}"


def test_cli_version_and_help():
    cli = _find_cli_binary()

    res = subprocess.run([str(cli), "--version"], capture_output=True, text=True, check=False)
    assert res.returncode == 0
    assert __version__ in res.stdout

    help_res = subprocess.run([str(cli), "--help"], capture_output=True, text=True, check=False)
    assert help_res.returncode == 0
    for subcmd in ("upload", "download", "exist", "sign-url", "channels"):
        assert subcmd in help_res.stdout


def test_cli_channels():
    cli = _find_cli_binary()
    res = subprocess.run([str(cli), "channels"], capture_output=True, text=True, check=False)
    assert res.returncode == 0
    assert "aliyun: available" in res.stdout


def test_cli_upload_rejects_directory_and_sign_url_rejects_zero_expiration(tmp_path):
    cli = _find_cli_binary()

    dir_res = subprocess.run(
        [str(cli), "upload", str(tmp_path), "a/b", "-v", "1.0.0"],
        capture_output=True,
        text=True,
        check=False,
    )
    assert dir_res.returncode != 0
    assert "错误:" in dir_res.stderr

    exp_res = subprocess.run(
        [str(cli), "sign-url", "a/b", "-v", "1.0.0", "--expiration-seconds", "0"],
        capture_output=True,
        text=True,
        check=False,
    )
    assert exp_res.returncode != 0
