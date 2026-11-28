//! funpub 纯 Rust 核心库。
//!
//! 提供多渠道制品发布抽象（[`Publisher`]、[`PublishResult`]）、
//! 渠道注册表（[`ChannelRegistry`]）以及阿里云 Packages Generic 仓库实现
//! （[`AliyunClient`]、[`AliyunPublisher`]）。

pub mod base;
pub mod channels;
pub mod checksum;
pub mod error;
pub mod http;

pub use base::{
    ensure_parent_dir, get_filepath, DownloadTarget, PublishResult, Publisher, UploadOptions,
};
pub use channels::aliyun::{
    parse_repo_url, AliyunClient, AliyunConfig, AliyunPublisher, ParsedRepoUrl, DEFAULT_CHUNK_SIZE,
    DEFAULT_MERGE_POLL_INTERVAL, DEFAULT_MERGE_TIMEOUT, DEFAULT_REPO_TYPE,
};
pub use channels::{
    create_publisher, list_available_channels, list_missing_channels, normalize_channel_name,
    ChannelConfig, ChannelRegistry, ChannelSpec,
};
pub use checksum::{compute_checksums, file_md5, file_sha1, file_sha256, FileChecksums};
pub use error::{FunPubError, Result};
pub use http::{
    HttpClient, HttpConfig, TimeoutConfig, DEFAULT_MAX_RETRIES, DEFAULT_POOL_SIZE,
    DEFAULT_RETRY_DELAY, DEFAULT_TIMEOUT,
};
