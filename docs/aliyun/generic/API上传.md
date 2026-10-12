# 阿里云 generic API 用法

`funpub` 封装了阿里云 Packages generic 的上传、下载和临时下载地址 API。建议使用
CLI 或 Python API，而不是在 `curl` 命令中传递 Basic Auth 凭据；凭据由 `funsecret`
管理，避免进入 shell 历史和进程列表。

## 配置

为仓库配置一次认证信息：

```bash
funsecret write https://packages.aliyun.com/api/protocol/{org_id}/generic/{repo} \
  funpub aliyun generic {repo_name} repo_url
funsecret write {username} funpub aliyun generic {repo_name} username
funsecret write {password} funpub aliyun generic {repo_name} password
```

## 上传

```bash
funpub upload ./output.tgz a/b/c --version 1.2.3 --repo-name {repo_name}
```

可选参数包括 `--filename`（制品名称）、`--description`（版本描述）和
`--overwrite`（允许覆盖同版本制品）。默认不覆盖已存在的版本。

## 下载

```bash
funpub download a/b/c --version 1.2.3 \
  --repo-name {repo_name} --output ./download.tgz
```

`--output` 可以是保存目录或完整文件路径；目标已存在时默认失败，使用
`--overwrite` 明确允许覆盖。

## 生成临时下载地址

```bash
funpub sign-url a/b/c --version 1.2.3 \
  --repo-name {repo_name} --expiration-seconds 3600
```

该命令输出可直接下载的临时免密 URL。有效期默认 3600 秒。

## Python API

```python
from funpub import get_publisher

publisher = get_publisher("aliyun", repo_name="{repo_name}")

result = publisher.upload_file(
    filepath="./output.tgz",
    path="a/b/c",
    version="1.2.3",
    description="logfile",
)
print(result.url)

publisher.download_file(
    path="a/b/c", version="1.2.3", filepath="./download.tgz"
)
url = publisher.get_download_url(path="a/b/c", version="1.2.3")
```

认证失败会抛出 `AuthenticationError`，制品不存在会抛出 `RemoteNotFoundError`，其余
远端 API 错误会抛出 `PublishError`。
