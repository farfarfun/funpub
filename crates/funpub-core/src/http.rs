//! 共享 HTTP 客户端与幂等重试策略。
//!
//! 对齐原 Python `funpub.core.http`：
//! - 支持单值超时或 `(connect, read)` 二元组超时；
//! - 仅对幂等方法（HEAD/GET/PUT/DELETE/OPTIONS/TRACE）在 429/500/502/503/504
//!   或连接类网络错误时自动退避重试；POST/PATCH 不重试，避免重复提交上传或异步合并。

use std::thread;
use std::time::Duration;

use reqwest::blocking::{Client, RequestBuilder, Response};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, RETRY_AFTER};
use reqwest::Method;

use crate::error::{FunPubError, Result};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
pub const DEFAULT_MAX_RETRIES: u32 = 3;
pub const DEFAULT_RETRY_DELAY: Duration = Duration::from_millis(500);
pub const DEFAULT_POOL_SIZE: usize = 10;

const RETRY_STATUS: [u16; 5] = [429, 500, 502, 503, 504];

/// 超时配置：单值或 `(connect, read)` 二元组。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TimeoutConfig {
    Single(Duration),
    ConnectRead { connect: Duration, read: Duration },
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self::Single(DEFAULT_TIMEOUT)
    }
}

impl TimeoutConfig {
    /// 从秒数构造单值超时并校验 > 0。
    pub fn from_secs_f64(secs: f64) -> Result<Self> {
        if !secs.is_finite() || secs <= 0.0 {
            return Err(FunPubError::InvalidArgument(
                "timeout 必须大于 0".to_string(),
            ));
        }
        Ok(Self::Single(Duration::from_secs_f64(secs)))
    }

    /// 从 `(connect, read)` 秒数二元组构造并校验两者均 > 0。
    pub fn from_pair_secs_f64(connect: f64, read: f64) -> Result<Self> {
        if !connect.is_finite() || !read.is_finite() || connect <= 0.0 || read <= 0.0 {
            return Err(FunPubError::InvalidArgument(
                "timeout 元组必须包含两个大于 0 的数值".to_string(),
            ));
        }
        Ok(Self::ConnectRead {
            connect: Duration::from_secs_f64(connect),
            read: Duration::from_secs_f64(read),
        })
    }

    pub fn validate(&self) -> Result<()> {
        match *self {
            Self::Single(d) => {
                if d.is_zero() {
                    return Err(FunPubError::InvalidArgument(
                        "timeout 必须大于 0".to_string(),
                    ));
                }
            }
            Self::ConnectRead { connect, read } => {
                if connect.is_zero() || read.is_zero() {
                    return Err(FunPubError::InvalidArgument(
                        "timeout 元组必须包含两个大于 0 的数值".to_string(),
                    ));
                }
            }
        }
        Ok(())
    }
}

/// HTTP 客户端配置。
#[derive(Debug, Clone)]
pub struct HttpConfig {
    pub timeout: TimeoutConfig,
    pub retries: u32,
    pub backoff_factor: Duration,
    pub pool_size: usize,
    pub default_headers: HeaderMap,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            timeout: TimeoutConfig::default(),
            retries: DEFAULT_MAX_RETRIES,
            backoff_factor: DEFAULT_RETRY_DELAY,
            pool_size: DEFAULT_POOL_SIZE,
            default_headers: HeaderMap::new(),
        }
    }
}

/// 带默认超时、连接池与幂等重试的同步 HTTP 客户端。
#[derive(Debug, Clone)]
pub struct HttpClient {
    client: Client,
    retries: u32,
    backoff_factor: Duration,
}

impl HttpClient {
    /// 按给定配置构建 HTTP 客户端。
    pub fn new(config: HttpConfig) -> Result<Self> {
        config.timeout.validate()?;
        if config.pool_size == 0 {
            return Err(FunPubError::InvalidArgument(
                "pool_size 必须大于 0".to_string(),
            ));
        }

        let mut builder = Client::builder()
            .pool_max_idle_per_host(config.pool_size)
            .default_headers(config.default_headers);

        builder = match config.timeout {
            TimeoutConfig::Single(d) => builder.connect_timeout(d).timeout(d),
            TimeoutConfig::ConnectRead { connect, read } => {
                builder.connect_timeout(connect).timeout(read)
            }
        };

        let client = builder
            .build()
            .map_err(|e| FunPubError::http(None, format!("初始化 HTTP 客户端失败: {e}")))?;

        Ok(Self {
            client,
            retries: config.retries,
            backoff_factor: config.backoff_factor,
        })
    }

    /// 返回底层 `reqwest::blocking::Client` 的引用。
    pub fn inner(&self) -> &Client {
        &self.client
    }

    /// 执行一次请求；仅对幂等方法在可重试状态码或连接错误时按指数退避重试。
    pub fn send_with_retry<F>(&self, method: &Method, mut build: F) -> Result<Response>
    where
        F: FnMut(&Client) -> RequestBuilder,
    {
        let retryable = is_idempotent(method);
        let max_attempts = if retryable { self.retries + 1 } else { 1 };

        for attempt in 0..max_attempts {
            let result = build(&self.client).send();
            match result {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    if retryable && attempt + 1 < max_attempts && RETRY_STATUS.contains(&status) {
                        let delay = retry_delay_from_response(&resp)
                            .unwrap_or_else(|| self.backoff_delay(attempt));
                        log::debug!(
                            "{} {} 返回 {}，{:?} 后重试（{}/{}）",
                            method,
                            resp.url(),
                            status,
                            delay,
                            attempt + 1,
                            self.retries
                        );
                        if !delay.is_zero() {
                            thread::sleep(delay);
                        }
                        continue;
                    }
                    return Ok(resp);
                }
                Err(err) => {
                    if retryable && attempt + 1 < max_attempts && err.is_connect() {
                        let delay = self.backoff_delay(attempt);
                        log::debug!(
                            "{} 连接失败（{}），{:?} 后重试（{}/{}）",
                            method,
                            err,
                            delay,
                            attempt + 1,
                            self.retries
                        );
                        if !delay.is_zero() {
                            thread::sleep(delay);
                        }
                        continue;
                    }
                    return Err(FunPubError::http(
                        err.status().map(|s| s.as_u16()),
                        format!("HTTP 请求失败: {err}"),
                    ));
                }
            }
        }

        unreachable!("loop always returns on final attempt")
    }

    fn backoff_delay(&self, attempt: u32) -> Duration {
        if self.backoff_factor.is_zero() {
            return Duration::ZERO;
        }
        let mult = 1u32.checked_shl(attempt).unwrap_or(u32::MAX);
        self.backoff_factor.saturating_mul(mult)
    }
}

/// 将静态键值对转换为 `HeaderMap`。
pub fn headers_from_pairs<I, K, V>(pairs: I) -> Result<HeaderMap>
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    let mut map = HeaderMap::new();
    for (k, v) in pairs {
        let name = HeaderName::from_bytes(k.as_ref().as_bytes())
            .map_err(|e| FunPubError::InvalidArgument(format!("无效的请求头名: {e}")))?;
        let val = HeaderValue::from_str(v.as_ref())
            .map_err(|e| FunPubError::InvalidArgument(format!("无效的请求头值: {e}")))?;
        map.insert(name, val);
    }
    Ok(map)
}

fn is_idempotent(method: &Method) -> bool {
    matches!(
        *method,
        Method::HEAD | Method::GET | Method::PUT | Method::DELETE | Method::OPTIONS | Method::TRACE
    )
}

fn retry_delay_from_response(resp: &Response) -> Option<Duration> {
    let val = resp.headers().get(RETRY_AFTER)?.to_str().ok()?.trim();
    val.parse::<u64>().ok().map(Duration::from_secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_timeouts() {
        assert!(TimeoutConfig::from_secs_f64(0.0).is_err());
        assert!(TimeoutConfig::from_secs_f64(-1.0).is_err());
        assert!(TimeoutConfig::from_secs_f64(f64::NAN).is_err());
        assert!(TimeoutConfig::from_pair_secs_f64(3.0, 0.0).is_err());
        assert!(TimeoutConfig::from_pair_secs_f64(3.0, 30.0).is_ok());
    }

    #[test]
    fn idempotent_methods_exclude_post_and_patch() {
        assert!(is_idempotent(&Method::GET));
        assert!(is_idempotent(&Method::HEAD));
        assert!(is_idempotent(&Method::PUT));
        assert!(is_idempotent(&Method::DELETE));
        assert!(!is_idempotent(&Method::POST));
        assert!(!is_idempotent(&Method::PATCH));
    }
}
