//! 阿里云 Packages Generic 制品仓库渠道。

pub mod client;
pub mod publisher;

pub use client::{
    parse_repo_url, AliyunClient, ParsedRepoUrl, DEFAULT_CHUNK_SIZE, DEFAULT_MERGE_POLL_INTERVAL,
    DEFAULT_MERGE_TIMEOUT,
};
pub use publisher::{AliyunConfig, AliyunPublisher, DEFAULT_REPO_TYPE};
