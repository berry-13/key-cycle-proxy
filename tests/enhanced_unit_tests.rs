use key_cycle_proxy::{
    config::{load_config, ApiKeyInfo, Config, UpstreamConfig},
    proxy::{KeyPool, ProxyEngine, ProxyError, UpstreamClient},
    types::{ErrorResponse, OpenAIRequest},
};
use secrecy::SecretString;
use std::io::Write;
use std::{sync::Arc, time::Duration};
use tempfile::NamedTempFile;

#[test]
fn test_config_loading_from_environment() {
    // Test environment variable configuration
    std::env::set_var("OPENAI_KEYS", "sk-key1,sk-key2,sk-key3");

    // Clean up any existing config.json to ensure env var is used
    std::fs::remove_file("config.json").unwrap_or(());

    let result = load_config();
    std::env::remove_var("OPENAI_KEYS");

    assert!(
        result.is_ok(),
        "Config loading should succeed with env var: {:?}",
        result.err()
    );
    let (_config, keys) = result.unwrap();
    assert_eq!(keys.len(), 3);
    assert_eq!(keys[0].models, vec!["others"]);
}

#[test]
fn test_config_loading_from_json() {
    // Create temporary config file
    let mut temp_file = NamedTempFile::new().unwrap();
    writeln!(
        temp_file,
        r#"{{
        "apiKeys": [
            {{
                "key": "sk-test-key-1",
                "url": "https://api.openai.com/v1",
                "models": ["gpt-3.5-turbo", "gpt-4"]
            }},
            {{
                "key": "sk-test-key-2",
                "url": "https://api.anthropic.com/v1",
                "models": ["claude-2", "others"]
            }}
        ]
    }}"#
    )
    .unwrap();

    // Copy temp file to config.json in current directory
    std::fs::copy(temp_file.path(), "config.json").unwrap();

    let result = load_config();

    // Clean up
    std::fs::remove_file("config.json").unwrap_or(());

    assert!(result.is_ok());
    let (_config, keys) = result.unwrap();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0].models, vec!["gpt-3.5-turbo", "gpt-4"]);
    assert_eq!(keys[1].models, vec!["claude-2", "others"]);
}

#[test]
fn test_config_error_handling_no_keys() {
    // Ensure no environment variable or config file exists
    std::env::remove_var("OPENAI_KEYS");
    std::fs::remove_file("config.json").unwrap_or(());

    let result = load_config();
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("No API keys found"));
}

#[test]
fn test_api_key_model_support() {
    let key_info = ApiKeyInfo {
        key: SecretString::new("test-key".to_string()),
        url: "https://api.test.com".to_string(),
        models: vec!["gpt-3.5-turbo".to_string(), "gpt-4".to_string()],
        latency: None,
        health_score: 1.0,
    };

    assert!(key_info.supports_model("gpt-3.5-turbo"));
    assert!(key_info.supports_model("gpt-4"));
    assert!(!key_info.supports_model("claude-2"));

    let fallback_key = ApiKeyInfo {
        key: SecretString::new("fallback-key".to_string()),
        url: "https://api.fallback.com".to_string(),
        models: vec!["others".to_string()],
        latency: None,
        health_score: 1.0,
    };

    assert!(fallback_key.supports_model("any-model"));
    assert!(fallback_key.supports_model("claude-2"));
    assert!(fallback_key.supports_model("custom-model"));
}

#[test]
fn test_upstream_config_duration_conversion() {
    let config = UpstreamConfig {
        base_url: "https://api.test.com".to_string(),
        connect_timeout_ms: 1000,
        request_timeout_ms: 5000,
        retry_initial_backoff_ms: 100,
        retry_max_backoff_ms: 2000,
        max_retries: 3,
    };

    assert_eq!(config.connect_timeout(), Duration::from_millis(1000));
    assert_eq!(config.request_timeout(), Duration::from_millis(5000));
    assert_eq!(config.retry_max_backoff(), Duration::from_millis(2000));
}

#[test]
fn test_key_pool_rotation_strategies() {
    let keys = vec![
        ApiKeyInfo {
            key: SecretString::new("key1".to_string()),
            url: "https://api1.com".to_string(),
            models: vec!["gpt-3.5-turbo".to_string()],
            latency: None,
            health_score: 1.0,
        },
        ApiKeyInfo {
            key: SecretString::new("key2".to_string()),
            url: "https://api2.com".to_string(),
            models: vec!["gpt-3.5-turbo".to_string()],
            latency: None,
            health_score: 1.0,
        },
        ApiKeyInfo {
            key: SecretString::new("key3".to_string()),
            url: "https://api3.com".to_string(),
            models: vec!["gpt-4".to_string()],
            latency: None,
            health_score: 1.0,
        },
    ];

    // Test round-robin strategy
    let pool = KeyPool::new(keys.clone(), "round_robin");
    let key1 = pool.get_key_for_model("gpt-3.5-turbo").unwrap();
    let key2 = pool.get_key_for_model("gpt-3.5-turbo").unwrap();
    assert_ne!(key1.url, key2.url); // Should rotate between keys

    // Test health-weighted strategy
    let pool = KeyPool::new(keys.clone(), "round_robin_health_weighted");
    let key = pool.get_key_for_model("gpt-3.5-turbo");
    assert!(key.is_some());

    // Test least-latency strategy
    let pool = KeyPool::new(keys, "least_latency");
    let key = pool.get_key_for_model("gpt-4");
    assert!(key.is_some());
}

#[test]
fn test_key_pool_latency_tracking() {
    let keys = vec![
        ApiKeyInfo {
            key: SecretString::new("fast-key".to_string()),
            url: "https://fast-api.com".to_string(),
            models: vec!["gpt-3.5-turbo".to_string()],
            latency: None,
            health_score: 1.0,
        },
        ApiKeyInfo {
            key: SecretString::new("slow-key".to_string()),
            url: "https://slow-api.com".to_string(),
            models: vec!["gpt-3.5-turbo".to_string()],
            latency: None,
            health_score: 1.0,
        },
    ];

    let pool = KeyPool::new(keys, "least_latency");

    // Update latency measurements
    pool.update_latency(0, Duration::from_millis(50)); // Fast key
    pool.update_latency(1, Duration::from_millis(200)); // Slow key

    // Should prefer the faster key in least_latency mode
    // Note: This tests the latency tracking mechanism
}

#[test]
fn test_error_response_serialization() {
    let error = ErrorResponse::new("Test error message");
    let json = serde_json::to_string(&error).unwrap();
    let parsed: ErrorResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.error, "Test error message");
}

#[test]
fn test_openai_request_parsing() {
    let json_str = r#"{
        "model": "gpt-3.5-turbo",
        "messages": [
            {"role": "user", "content": "Hello"}
        ],
        "temperature": 0.7,
        "max_tokens": 150
    }"#;

    let request: OpenAIRequest = serde_json::from_str(json_str).unwrap();
    assert_eq!(request.model, "gpt-3.5-turbo");
    assert!(request.other.contains_key("messages"));
    assert!(request.other.contains_key("temperature"));
    assert!(request.other.contains_key("max_tokens"));
}

#[test]
fn test_openai_request_minimal() {
    let json_str = r#"{"model": "gpt-4"}"#;
    let request: OpenAIRequest = serde_json::from_str(json_str).unwrap();
    assert_eq!(request.model, "gpt-4");
    assert!(request.other.is_empty());
}

#[test]
fn test_proxy_error_status_codes() {
    assert_eq!(
        ProxyError::NoKeyAvailable {
            model: "test".to_string()
        }
        .status_code(),
        axum::http::StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        ProxyError::MethodNotAllowed.status_code(),
        axum::http::StatusCode::METHOD_NOT_ALLOWED
    );
    assert_eq!(
        ProxyError::Timeout.status_code(),
        axum::http::StatusCode::GATEWAY_TIMEOUT
    );
    assert_eq!(
        ProxyError::RateLimited.status_code(),
        axum::http::StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        ProxyError::PayloadTooLarge.status_code(),
        axum::http::StatusCode::PAYLOAD_TOO_LARGE
    );
}

#[test]
fn test_config_defaults() {
    let config = Config::default();
    assert_eq!(config.server.bind_addr, "0.0.0.0:8080");
    assert_eq!(config.server.request_body_limit_bytes, 262_144);
    assert_eq!(config.upstream.max_retries, 3);
    assert_eq!(config.keys.rotation_strategy, "round_robin_health_weighted");
    assert_eq!(config.rate_limit.global_rps, 50);
    assert_eq!(config.observability.tracing_level, "info");
}

#[test]
fn test_upstream_client_creation() {
    let config = UpstreamConfig::default();
    let client = UpstreamClient::new(config);
    assert!(client.is_ok());
}

#[test]
fn test_proxy_engine_creation() {
    let keys = vec![ApiKeyInfo {
        key: SecretString::new("test-key".to_string()),
        url: "https://api.test.com".to_string(),
        models: vec!["gpt-3.5-turbo".to_string()],
        latency: None,
        health_score: 1.0,
    }];

    let key_pool = Arc::new(KeyPool::new(keys, "round_robin"));
    let upstream_config = UpstreamConfig::default();
    let upstream_client = UpstreamClient::new(upstream_config).unwrap();
    let _engine = ProxyEngine::new(key_pool, upstream_client, 3);

    // Engine should be created successfully
    // This is a smoke test for the constructor
}

#[test]
fn test_json_error_handling() {
    let invalid_json = "{ invalid json }";
    let result: Result<OpenAIRequest, _> = serde_json::from_str(invalid_json);
    assert!(result.is_err());
}

#[test]
fn test_empty_key_pool() {
    let empty_keys: Vec<ApiKeyInfo> = vec![];
    let pool = KeyPool::new(empty_keys, "round_robin");

    let result = pool.get_key_for_model("gpt-3.5-turbo");
    assert!(result.is_none());

    let result = pool.get_next_key();
    assert!(result.is_none());
}

#[test]
fn test_key_pool_concurrent_access() {
    use std::sync::Arc;
    use std::thread;

    let keys = vec![
        ApiKeyInfo {
            key: SecretString::new("key1".to_string()),
            url: "https://api1.com".to_string(),
            models: vec!["gpt-3.5-turbo".to_string()],
            latency: None,
            health_score: 1.0,
        },
        ApiKeyInfo {
            key: SecretString::new("key2".to_string()),
            url: "https://api2.com".to_string(),
            models: vec!["gpt-3.5-turbo".to_string()],
            latency: None,
            health_score: 1.0,
        },
    ];

    let pool = Arc::new(KeyPool::new(keys, "round_robin"));

    // Spawn multiple threads accessing the pool concurrently
    let mut handles = vec![];
    for _ in 0..10 {
        let pool_clone = pool.clone();
        let handle = thread::spawn(move || {
            for _ in 0..100 {
                let _key = pool_clone.get_key_for_model("gpt-3.5-turbo");
                let _next_key = pool_clone.get_next_key();
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // If we get here without deadlocks or panics, concurrent access works
}

#[tokio::test]
async fn test_latency_measurement_timeout() {
    // Test that latency measurement handles timeouts gracefully
    let keys = vec![ApiKeyInfo {
        key: SecretString::new("timeout-key".to_string()),
        url: "https://definitely-not-a-real-domain-12345.com".to_string(),
        models: vec!["gpt-3.5-turbo".to_string()],
        latency: None,
        health_score: 1.0,
    }];

    let pool = KeyPool::new(keys, "round_robin");

    // This should complete without hanging, even though the URL is unreachable
    let start = std::time::Instant::now();
    pool.update_all_latencies().await;
    let duration = start.elapsed();

    // Should complete within reasonable time (timeout mechanism working)
    assert!(
        duration < Duration::from_secs(10),
        "Latency measurement should timeout quickly"
    );
}
