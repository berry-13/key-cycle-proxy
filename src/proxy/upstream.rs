use crate::config::{ApiKeyInfo, UpstreamConfig};
use crate::proxy::error::{ProxyError, ProxyResult};
use reqwest::{Client, Method, Response};
use secrecy::ExposeSecret;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;
use tracing::{debug, warn};

#[derive(Debug, Clone)]
pub struct UpstreamClient {
    client: Client,
    config: UpstreamConfig,
}

impl UpstreamClient {
    pub fn new(config: UpstreamConfig) -> ProxyResult<Self> {
        let client = Client::builder()
            .timeout(config.request_timeout())
            .connect_timeout(config.connect_timeout())
            .build()
            .map_err(|e| ProxyError::internal(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self { client, config })
    }

    /// Make a request to the upstream API with simple retry logic
    pub async fn request(
        &self,
        method: Method,
        key_info: Arc<ApiKeyInfo>,
        path: &str,
        body: Option<bytes::Bytes>,
        headers: Option<reqwest::header::HeaderMap>,
    ) -> ProxyResult<Response> {
        let url = format!("{}{}", key_info.url, path);
        
        debug!(
            "Making {} request to {} with API key (redacted)",
            method, url
        );

        // Try the request with simple retry logic
        for attempt in 0..=self.config.max_retries {
            let mut request = self.client.request(method.clone(), &url);

            // Add authorization header
            request = request.header(
                "Authorization",
                format!("Bearer {}", key_info.key.expose_secret()),
            );

            // Add body if provided
            if let Some(body) = body.as_ref() {
                request = request.body(body.clone());
                request = request.header("Content-Type", "application/json");
            }

            // Add custom headers if provided
            if let Some(headers) = headers.as_ref() {
                request = request.headers(headers.clone());
            }

            // Execute request with timeout
            match timeout(self.config.request_timeout(), request.send()).await {
                Ok(Ok(response)) => {
                    // Check for retryable HTTP status codes
                    if self.should_retry_status(response.status()) && attempt < self.config.max_retries {
                        warn!(
                            "Received retryable status {} from upstream {}, attempt {}/{}",
                            response.status(),
                            url,
                            attempt + 1,
                            self.config.max_retries + 1
                        );
                        
                        // Wait before retry
                        let wait_time = Duration::from_millis(self.config.retry_initial_backoff_ms * (2_u64.pow(attempt)));
                        tokio::time::sleep(wait_time.min(self.config.retry_max_backoff())).await;
                        continue;
                    }
                    
                    return Ok(response);
                }
                Ok(Err(e)) => {
                    if self.should_retry_error(&e) && attempt < self.config.max_retries {
                        warn!("Request error: {}, retrying attempt {}/{}", e, attempt + 1, self.config.max_retries + 1);
                        
                        // Wait before retry
                        let wait_time = Duration::from_millis(self.config.retry_initial_backoff_ms * (2_u64.pow(attempt)));
                        tokio::time::sleep(wait_time.min(self.config.retry_max_backoff())).await;
                        continue;
                    } else {
                        return Err(ProxyError::UpstreamFailed { source: e });
                    }
                }
                Err(_) => {
                    if attempt < self.config.max_retries {
                        warn!("Request timeout, retrying attempt {}/{}", attempt + 1, self.config.max_retries + 1);
                        
                        // Wait before retry
                        let wait_time = Duration::from_millis(self.config.retry_initial_backoff_ms * (2_u64.pow(attempt)));
                        tokio::time::sleep(wait_time.min(self.config.retry_max_backoff())).await;
                        continue;
                    } else {
                        return Err(ProxyError::Timeout);
                    }
                }
            }
        }

        Err(ProxyError::AllRetriesExhausted)
    }

    /// Forward a request as-is (streaming)
    pub async fn forward_request(
        &self,
        method: Method,
        key_info: Arc<ApiKeyInfo>,
        path: &str,
        body: Option<bytes::Bytes>,
        headers: Option<reqwest::header::HeaderMap>,
    ) -> ProxyResult<Response> {
        self.request(method, key_info, path, body, headers).await
    }

    fn should_retry_error(&self, error: &reqwest::Error) -> bool {
        // Retry on connection errors, timeouts, and server errors
        error.is_connect() || error.is_timeout() || error.is_request()
    }

    fn should_retry_status(&self, status: reqwest::StatusCode) -> bool {
        // Retry on specific status codes that indicate temporary issues
        matches!(
            status.as_u16(),
            429 | // Too Many Requests
            418 | // I'm a teapot (some APIs use this for rate limiting)
            502 | // Bad Gateway
            503 | // Service Unavailable
            504   // Gateway Timeout
        )
    }
}

/// Check if the upstream response indicates an error that should trigger a key rotation
pub fn should_rotate_key(status: reqwest::StatusCode) -> bool {
    matches!(
        status.as_u16(),
        429 | // Too Many Requests
        418 | // I'm a teapot
        502 | // Bad Gateway
        400   // Bad Request (potentially invalid key)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;
    use secrecy::SecretString;

    fn create_test_config() -> UpstreamConfig {
        UpstreamConfig {
            base_url: "https://api.openai.com/v1".to_string(),
            connect_timeout_ms: 1000,
            request_timeout_ms: 5000,
            retry_initial_backoff_ms: 100,
            retry_max_backoff_ms: 1000,
            max_retries: 3,
        }
    }

    fn create_test_key() -> Arc<ApiKeyInfo> {
        Arc::new(ApiKeyInfo {
            key: SecretString::new("test-key".to_string()),
            url: "https://api.test.com".to_string(),
            models: vec!["gpt-3.5-turbo".to_string()],
            latency: None,
            health_score: 1.0,
        })
    }

    #[test]
    fn test_should_retry_status() {
        let config = create_test_config();
        let client = UpstreamClient::new(config).unwrap();

        assert!(client.should_retry_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(client.should_retry_status(StatusCode::BAD_GATEWAY));
        assert!(client.should_retry_status(StatusCode::SERVICE_UNAVAILABLE));
        assert!(client.should_retry_status(StatusCode::GATEWAY_TIMEOUT));
        assert!(client.should_retry_status(StatusCode::IM_A_TEAPOT));

        assert!(!client.should_retry_status(StatusCode::OK));
        assert!(!client.should_retry_status(StatusCode::BAD_REQUEST));
        assert!(!client.should_retry_status(StatusCode::UNAUTHORIZED));
        assert!(!client.should_retry_status(StatusCode::NOT_FOUND));
    }

    #[test]
    fn test_should_rotate_key() {
        assert!(should_rotate_key(StatusCode::TOO_MANY_REQUESTS));
        assert!(should_rotate_key(StatusCode::BAD_GATEWAY));
        assert!(should_rotate_key(StatusCode::BAD_REQUEST));
        assert!(should_rotate_key(StatusCode::IM_A_TEAPOT));

        assert!(!should_rotate_key(StatusCode::OK));
        assert!(!should_rotate_key(StatusCode::CREATED));
        assert!(!should_rotate_key(StatusCode::NOT_FOUND));
    }
}