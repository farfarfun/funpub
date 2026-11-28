# funpub

`funpub` 是多渠道制品发布工具。核心引擎采用 **Rust** 实现（`funpub-core`），直接通过 `funsecret-core` 读取本地加密仓库凭据，同时提供：

- **原生 CLI 二进制**（`funpub`）：零 Python 依赖、纯 Rust TLS（rustls），通过 `apt` / `brew` / `cargo install` / GitHub Release 静态二进制分发；
- **Python 扩展包**（PyPI `funpub`）：基于 PyO3 + maturin（`abi3-py312`）包装，**零运行时依赖**，完全兼容历史 Python API；
- **Rust 核心库**（`funpub-core`）：发布于 crates.io，供 Rust 工具链与服务直接调用。

当前支持的渠道：

- **aliyun** — 阿里云 Packages generic 仓库（协议细节见 `docs/aliyun/generic/`）

## 安装

### 命令行安装（原生静态二进制，零 Python 依赖）

```bash
# APT（添加 farfarfun 源后，见 https://farfarfun.github.io/packages/funpub/）
sudo apt install funpub

# Homebrew
brew install farfarfun/packages/funpub

# Cargo
cargo install funpub

# 或直接从 GitHub Releases 下载对应平台的静态二进制 / .deb
```

### Python 库安装（仅提供 Python API，零运行时依赖，不含命令行入口）

```bash
pip install funpub
```

### Rust 核心库

```toml
[dependencies]
funpub-core = "0.1"
```

## 命令行用法

阿里云账号下可以有多个仓库，同一类型（如 generic）下也可以有多个不同名字
的仓库，因此凭证按 `(repo-type, repo-name)` 两级区分，一个仓库只需要用
`funsecret` 配置一次，之后命令行只需要传 `--repo-name`：

```bash
funsecret write https://packages.aliyun.com/api/protocol/{org_id}/generic/{repo} \
  funpub aliyun generic {repo_name} repo_url
funsecret write {username} funpub aliyun generic {repo_name} username
funsecret write {password} funpub aliyun generic {repo_name} password
```

```bash
# 上传（repo-type 默认 generic，凭证从 funsecret 按 repo-name 自动读取）
funpub upload ./dist/app-1.0.0.tar.gz path/to/app \
  --version 1.0.0 --repo-name {repo_name}

# 下载
funpub download path/to/app --version 1.0.0 \
  --repo-name {repo_name} --output ./downloads/

# 检查制品是否存在（存在退出码 0，不存在退出码 1）
funpub exist path/to/app --version 1.0.0 --repo-name {repo_name}

# 生成临时免密下载地址
funpub sign-url path/to/app --version 1.0.0 \
  --repo-name {repo_name} --expiration-seconds 3600

# 查看已注册的渠道
funpub channels

# 查看版本
funpub --version
```

CLI 不接受密码参数，认证信息统一通过 `funsecret` 按仓库名称读取，避免凭据进入 shell 历史和进程列表。

## Python 用法

```python
from funpub import get_publisher

# 方式一（推荐）：仓库信息已通过 funsecret 配置过（见上面「命令行用法」
# 里的 funsecret write 示例），只需要传 repo_name，无需在代码里出现凭据
publisher = get_publisher("aliyun", repo_name="funpackage")

# 方式二：测试/一次性脚本场景下，在代码中直接传入配置（不适用于 CLI；
# username/password 不要写成字面量，应从环境变量或自建的 secret 存取读取）
import os

publisher = get_publisher(
    "aliyun",
    repo_url=os.environ["FUNPUB_ALIYUN_REPO_URL"],
    username=os.environ["FUNPUB_ALIYUN_USERNAME"],
    password=os.environ["FUNPUB_ALIYUN_PASSWORD"],
)

# 文件大小超过 chunk_threshold（默认等于 chunk_size，100MB）会自动走分块上传
result = publisher.upload_file(
    filepath="./dist/app-1.0.0.tar.gz",
    path="path/to/app",
    version="1.0.0",
)
print(result.url, result.md5)

publisher.download_file(path="path/to/app", version="1.0.0", save_dir="./downloads")
url = publisher.get_download_url(path="path/to/app", version="1.0.0")
```

## 架构设计

仓库采用与 `funget` / `funsecret` 一致的 3-crate 纯虚拟 Rust 工作区：

- `crates/funpub-core`：纯 Rust 核心库，实现 `Publisher`、`PublishResult`、`ChannelRegistry`、流式校验和、幂等重试 HTTP 客户端、阿里云 Generic 协议以及通过 `funsecret-core` 读取加密凭据。
- `crates/funpub`：独立原生 CLI 二进制（`clap` + `env_logger`），支持 musl 静态构建。
- `crates/funpub-py`：PyO3（`abi3-py312`）+ maturin 扩展模块 `_funpub`，不依赖 CLI crate，提供零依赖 Python API（`get_publisher` / `PublishResult` / `BasePublisher` / `AliyunPublisher` / `FunPubError` 异常族）。
- `src/funpub/`：薄 Python 再导出层与 `.pyi` 类型存根，保持 `from funpub import ...`、`from funpub.core import ...`、`from funpub.channels import ...` 全部兼容。

## 开发

```bash
# Rust 检查与测试
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --exclude funpub-py

# 构建原生 CLI
cargo build --release --package funpub

# 构建 Python 扩展并运行 pytest
maturin build --release --out dist
pytest
```

---

## 关于 farfarfun

[farfarfun](https://github.com/farfarfun) 是一个专注于实用工具库的开源组织，
涵盖云存储、数据处理、AI、多媒体与开发工具链等方向。

- 🏠 组织主页：<https://github.com/farfarfun>
- 📦 PyPI：<https://pypi.org/user/niuliangtao/>
- 📧 联系：farfarfun@qq.com

本项目基于 [MIT](LICENSE) 协议开源。
