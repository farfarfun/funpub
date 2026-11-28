# funpub CLI

`funpub` 是多渠道制品发布命令行工具，采用纯 Rust 实现、零 Python 运行时依赖，支持：

- `funpub upload`：上传制品文件（自动按阈值切换普通上传或分块上传，默认拒绝覆盖）；
- `funpub download`：流式原子下载制品文件（支持目录或完整文件路径）；
- `funpub exist`：检查指定版本制品是否存在（存在退出码 `0`，不存在退出码 `1`）；
- `funpub sign-url`：生成临时免密下载地址；
- `funpub channels`：列出已注册的发布渠道及可用状态。

所有仓库凭据均通过 `funsecret` 按 `(repo-type, repo-name)` 安全读取，命令行不接受密码参数。
