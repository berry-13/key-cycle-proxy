use anyhow::{Context, Result};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub upstream: UpstreamConfig,
    #[serde(default)]
    pub keys: KeysConfig,
    #[serde(default)]
    pub rate_limit: RateLimitConfig,
    #[serde(default)]
    pub observability: ObservabilityConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    #[serde(default = "default_bind_addr")]
    pub bind_addr: String,
    #[serde(default = "default_request_body_limit")]
    pub request_body_limit_bytes: usize,
    #[serde(default = "default_graceful_shutdown_seconds")]
    pub graceful_shutdown_seconds: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UpstreamConfig {
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout_ms: u64,
    #[serde(default = "default_request_timeout")]
    pub request_timeout_ms: u64,
    #[serde(default = "default_retry_initial_backoff")]
    pub retry_initial_backoff_ms: u64,
    #[serde(default = "default_retry_max_backoff")]
    pub retry_max_backoff_ms: u64,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KeysConfig {
    #[serde(default = "default_rotation_strategy")]
    pub rotation_strategy: String,
    #[serde(default = "default_unhealthy_penalty")]
    pub unhealthy_penalty: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RateLimitConfig {
    #[serde(default = "default_per_key_rps")]
    pub per_key_rps: u32,
    #[serde(default = "default_global_rps")]
    pub global_rps: u32,
    #[serde(default = "default_burst")]
    pub burst: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ObservabilityConfig {
    #[serde(default = "default_metrics_bind")]
    pub metrics_bind: String,
    #[serde(default = "default_tracing_level")]
    pub tracing_level: String,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct ApiKeyInfo {
    #[serde(skip_serializing)]
    pub key: SecretString,
    pub url: String,
    pub models: Vec<String>,
    #[serde(skip)]
    pub latency: Option<Duration>,
    #[serde(skip)]
    pub health_score: f64,
}

impl ApiKeyInfo {
    pub fn supports_model(&self, model: &str) -> bool {
        self.models.iter().any(|m| m == model || m == "others")
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LegacyConfig {
    #[serde(rename = "apiKeys")]
    pub api_keys: Vec<LegacyApiKeyInfo>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LegacyApiKeyInfo {
    pub key: String,
    pub url: String,
    pub models: Vec<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: default_bind_addr(),
            request_body_limit_bytes: default_request_body_limit(),
            graceful_shutdown_seconds: default_graceful_shutdown_seconds(),
        }
    }
}

impl Default for UpstreamConfig {
    fn default() -> Self {
        Self {
            base_url: default_base_url(),
            connect_timeout_ms: default_connect_timeout(),
            request_timeout_ms: default_request_timeout(),
            retry_initial_backoff_ms: default_retry_initial_backoff(),
            retry_max_backoff_ms: default_retry_max_backoff(),
            max_retries: default_max_retries(),
        }
    }
}

impl Default for KeysConfig {
    fn default() -> Self {
        Self {
            rotation_strategy: default_rotation_strategy(),
            unhealthy_penalty: default_unhealthy_penalty(),
        }
    }
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            per_key_rps: default_per_key_rps(),
            global_rps: default_global_rps(),
            burst: default_burst(),
        }
    }
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            metrics_bind: default_metrics_bind(),
            tracing_level: default_tracing_level(),
        }
    }
}

// Default value functions
fn default_bind_addr() -> String {
    "0.0.0.0:8080".to_string()
}
fn default_request_body_limit() -> usize {
    262_144
}
fn default_graceful_shutdown_seconds() -> u64 {
    10
}
fn default_base_url() -> String {
    "https://api.openai.com/v1".to_string()
}
fn default_connect_timeout() -> u64 {
    800
}
fn default_request_timeout() -> u64 {
    60_000
}
fn default_retry_initial_backoff() -> u64 {
    50
}
fn default_retry_max_backoff() -> u64 {
    2000
}
fn default_max_retries() -> u32 {
    3
}
fn default_rotation_strategy() -> String {
    "round_robin_health_weighted".to_string()
}
fn default_unhealthy_penalty() -> u32 {
    5
}
fn default_per_key_rps() -> u32 {
    3
}
fn default_global_rps() -> u32 {
    50
}
fn default_burst() -> u32 {
    10
}
fn default_metrics_bind() -> String {
    "0.0.0.0:9090".to_string()
}
fn default_tracing_level() -> String {
    "info".to_string()
}

pub fn load_config() -> Result<(Config, Vec<ApiKeyInfo>)> {
    // Load main config
    let mut config = Config::default();

    // Try to load from config file if it exists
    if let Ok(config_str) = std::fs::read_to_string("config.toml") {
        config = toml::from_str(&config_str).context("Failed to parse config.toml")?;
    }

    // Load API keys from environment or legacy config.json
    let api_keys = load_api_keys()?;

    Ok((config, api_keys))
}

fn load_api_keys() -> Result<Vec<ApiKeyInfo>> {
    // First try environment variable
    if let Ok(keys_env) = std::env::var("OPENAI_KEYS") {
        let keys: Vec<&str> = keys_env.split(',').collect();
        return Ok(keys
            .into_iter()
            .map(|key| ApiKeyInfo {
                key: SecretString::new(key.trim().to_string()),
                url: default_base_url(),
                models: vec!["others".to_string()],
                latency: None,
                health_score: 1.0,
            })
            .collect());
    }

    // Fallback to legacy config.json
    if let Ok(config_content) = std::fs::read_to_string("config.json") {
        let legacy_config: LegacyConfig =
            serde_json::from_str(&config_content).context("Failed to parse config.json")?;

        return Ok(legacy_config
            .api_keys
            .into_iter()
            .map(|key_info| ApiKeyInfo {
                key: SecretString::new(key_info.key),
                url: key_info.url,
                models: key_info.models,
                latency: None,
                health_score: 1.0,
            })
            .collect());
    }

    anyhow::bail!("No API keys found. Set OPENAI_KEYS environment variable or create config.json");
}

impl UpstreamConfig {
    pub fn connect_timeout(&self) -> Duration {
        Duration::from_millis(self.connect_timeout_ms)
    }

    pub fn request_timeout(&self) -> Duration {
        Duration::from_millis(self.request_timeout_ms)
    }

    #[allow(dead_code)]
    pub fn retry_initial_backoff(&self) -> Duration {
        Duration::from_millis(self.retry_initial_backoff_ms)
    }

    pub fn retry_max_backoff(&self) -> Duration {
        Duration::from_millis(self.retry_max_backoff_ms)
    }
}

impl ServerConfig {
    pub fn graceful_shutdown_duration(&self) -> Duration {
        Duration::from_secs(self.graceful_shutdown_seconds)
    }
}
