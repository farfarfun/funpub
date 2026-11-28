//! 发布渠道注册表与工厂方法。

pub mod aliyun;

use crate::base::Publisher;
use crate::error::{FunPubError, Result};
use crate::http::TimeoutConfig;

use self::aliyun::{AliyunConfig, AliyunPublisher, DEFAULT_CHUNK_SIZE, DEFAULT_REPO_TYPE};

/// 单个发布渠道的描述信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub available: bool,
}

const BUILTIN_CHANNELS: &[ChannelSpec] = &[ChannelSpec {
    name: "aliyun",
    description: "阿里云 Packages Generic 制品仓库",
    available: true,
}];

/// 渠道注册表。
#[derive(Debug, Default, Clone, Copy)]
pub struct ChannelRegistry;

impl ChannelRegistry {
    /// 返回所有内置渠道规格。
    pub fn specs() -> &'static [ChannelSpec] {
        BUILTIN_CHANNELS
    }

    /// 列出所有可用渠道。
    pub fn list_available() -> Vec<ChannelSpec> {
        BUILTIN_CHANNELS
            .iter()
            .filter(|s| s.available)
            .cloned()
            .collect()
    }

    /// 列出所有不可用（缺失依赖）的渠道及提示。
    pub fn list_missing() -> Vec<(String, String)> {
        Vec::new()
    }
}

/// 标准化渠道名称（去除首尾空白并转小写）。
pub fn normalize_channel_name(channel: &str) -> String {
    channel.trim().to_ascii_lowercase()
}

/// 列出所有可用渠道规格。
pub fn list_available_channels() -> Vec<ChannelSpec> {
    ChannelRegistry::list_available()
}

/// 列出缺失依赖的渠道及其安装提示（Rust 原生内置全部渠道，默认为空）。
pub fn list_missing_channels() -> Vec<(String, String)> {
    ChannelRegistry::list_missing()
}

/// 通用渠道配置（用于 [`create_publisher`] 工厂函数）。
#[derive(Debug, Clone)]
pub struct ChannelConfig {
    pub repo_name: Option<String>,
    pub repo_type: String,
    pub repo_url: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub chunk_size: usize,
    pub chunk_threshold: Option<usize>,
    pub timeout: TimeoutConfig,
    pub secret_dir: Option<String>,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            repo_name: None,
            repo_type: DEFAULT_REPO_TYPE.to_string(),
            repo_url: None,
            username: None,
            password: None,
            chunk_size: DEFAULT_CHUNK_SIZE,
            chunk_threshold: None,
            timeout: TimeoutConfig::default(),
            secret_dir: None,
        }
    }
}

/// 根据渠道名称创建对应的 [`Publisher`] 实例。
pub fn create_publisher(channel: &str, config: ChannelConfig) -> Result<Box<dyn Publisher>> {
    let key = normalize_channel_name(channel);
    match key.as_str() {
        "aliyun" => {
            let aliyun_cfg = AliyunConfig {
                repo_name: config.repo_name,
                repo_type: config.repo_type,
                repo_url: config.repo_url,
                username: config.username,
                password: config.password,
                chunk_size: config.chunk_size,
                chunk_threshold: config.chunk_threshold,
                timeout: config.timeout,
                secret_dir: config.secret_dir,
            };
            Ok(Box::new(AliyunPublisher::new(aliyun_cfg)?))
        }
        _ => {
            let available: Vec<&str> = BUILTIN_CHANNELS.iter().map(|s| s.name).collect();
            Err(FunPubError::ChannelNotFoundError(format!(
                "不支持的发布渠道: {channel}. 可用渠道: {}",
                available.join(", ")
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_contains_aliyun_and_rejects_unknown() {
        let avail = list_available_channels();
        assert!(avail.iter().any(|c| c.name == "aliyun"));
        assert!(list_missing_channels().is_empty());

        let err = create_publisher("does-not-exist", ChannelConfig::default())
            .err()
            .unwrap();
        assert!(matches!(err, FunPubError::ChannelNotFoundError(_)));
    }

    #[test]
    fn normalizes_channel_name() {
        assert_eq!(normalize_channel_name("  AliYun "), "aliyun");
    }
}
