# 更新日志

## 0.1.6

### 变更

- 补齐并提交 `uv.lock`，保证依赖可复现构建（此前被 `.gitignore` 排除）。

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
