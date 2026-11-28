# funpub-core

`funpub-core` 是 [funpub](https://github.com/farfarfun/funpub) 的纯 Rust 核心引擎，提供：

- 统一的发布渠道抽象（`Publisher` trait、`PublishResult`、`ChannelRegistry`）与细分错误类型（`FunPubError`）；
- 阿里云 Packages Generic 制品仓库客户端与发布器（`AliyunClient`、`AliyunPublisher`），支持简单上传、大文件分块上传（会话创建 -> 分块 PATCH -> 异步合并 -> 轮询校验 -> 仓库关联）、流式原子下载、存在性检查与临时免密签名链接；
- 通过 `funsecret-core` 直接读取按 `(repo_type, repo_name)` 两级分类存储的加密仓库凭据，纯 Rust TLS（rustls），零 OpenSSL 与 Python 依赖。
