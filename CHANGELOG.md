# 更新日志

## 0.2.0

### 变更

- 迁移为与 `funget` / `funsecret` 一致的 Rust 3-crate 虚拟工作区架构：
  - `crates/funpub-core`（`0.1.0`）：纯 Rust 核心引擎，基于 `reqwest`（rustls，无 OpenSSL）与 `funsecret-core` 实现阿里云 Packages Generic 简单上传、分块上传与合并轮询、流式原子下载、存在性检查与临时签名链接；
  - `crates/funpub`（`0.2.0`）：独立原生 CLI 二进制，零 Python 依赖，支持通过 `apt` / `brew` / `cargo install` / GitHub Release 静态二进制安装；
  - `crates/funpub-py`（`0.2.0`）：基于 PyO3 + maturin（`abi3-py312`）的 Python 扩展模块 `_funpub`，PyPI 包零运行时依赖且不再安装 `console_scripts` 入口，保留全部既有 Python API 与异常继承体系。
- 新增 `packaging/build-deb.sh` 及 `.github/workflows/ci.yml` / `release.yml` 全平台构建与发布流水线。

## 0.1.6

### 变更

- 停止跟踪并忽略本地 `uv.lock`，避免将本地锁文件作为仓库的可复现构建依据。

## 0.1.5

### 新增

- `AliyunClient` 支持专属域名格式的 `repo_url`
  （`https://{组织标识}-{地域标识}.devops.aliyuncs.com/packages/api/protocol/{repo_type}/{repo}`），
  与原有经典格式（`https://packages.aliyun.com/api/protocol/{org_id}/generic/{repo}`）共存。

## 0.1.4

### 新增

- 补充 Aliyun 发布渠道的下载、存在性检查和签名下载地址测试。

### 修复

- 修正文档路径、依赖版本和仓库忽略规则。

### 变更

- 统一使用 Python 3.10 原生泛型和联合类型标注。

### 废弃

- 无。
