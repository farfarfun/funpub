# funpub

多渠道制品发布工具。通过统一接口向不同的制品仓库上传/下载文件，当前支持：

- **aliyun** — 阿里云 Packages generic 仓库（协议细节见 `docs/aliyun/common/`）

新渠道的接入方式见下方「新增渠道」。

## 安装

```bash
pip install funpub
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

# 生成临时免密下载地址
funpub sign-url path/to/app --version 1.0.0 \
  --repo-name {repo_name} --expiration-seconds 3600

# 查看已注册的渠道
funpub channels
```

也可以不经 funsecret，直接用 `--repo-url`/`--username`/`--password` 传入：

```bash
funpub upload ./dist/app-1.0.0.tar.gz path/to/app \
  --version 1.0.0 \
  --repo-url https://packages.aliyun.com/api/protocol/{org_id}/generic/{repo} \
  --username {username} --password {password}
```

## Python 用法

```python
from funpub import get_publisher

# 方式一：仓库信息已通过 funsecret 配置过，只需要传 repo_name
publisher = get_publisher("aliyun", repo_name="{repo_name}")

# 方式二：直接传 repo_url/username/password，不依赖 funsecret
publisher = get_publisher(
    "aliyun",
    repo_url="https://packages.aliyun.com/api/protocol/{org_id}/generic/{repo}",
    username="...",
    password="...",
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

## 设计

- `funpub.core.BasePublisher`：所有渠道实现的统一接口
  （`upload_file` / `download_file` / `exist` / `get_download_url`）。
- `funpub.core.PublishResult`：上传结果，字典 + 属性双重访问
  （`path` / `version` / `url` / `size` / `md5` / `sha1` / `sha256`）。
- `funpub.channels`：渠道注册表，懒加载各渠道模块 —— `import funpub` 不会
  拉入任何渠道的第三方依赖，只有真正使用某个渠道时才会导入它。

### 新增渠道

1. 在 `src/funpub/channels/<name>/` 下实现一个 `BasePublisher` 子类。
2. 在 `src/funpub/channels/__init__.py` 的 `CHANNEL_SPECS` 里注册一行
   `ChannelSpec`。

## 开发

```bash
uv sync
uv run pytest
uv run ruff check .
```
