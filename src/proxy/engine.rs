use crate::proxy::{
    error::{ProxyError, ProxyResult},
    key_pool::KeyPool,
    upstream::{should_rotate_key, UpstreamClient},
};
use crate::types::OpenAIRequest;
use crate::util::{
    convert_axum_headers_to_reqwest, convert_axum_method_to_reqwest,
    convert_reqwest_headers_to_axum,
};
use axum::body::Body;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::Response;
use bytes::Bytes;
use std::sync::Arc;
use tokio_stream::StreamExt;
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone)]
pub struct ProxyEngine {
    key_pool: Arc<KeyPool>,
    upstream_client: UpstreamClient,
    max_retries: u32,
}

impl ProxyEngine {
    pub fn new(key_pool: Arc<KeyPool>, upstream_client: UpstreamClient, max_retries: u32) -> Self {
        Self {
            key_pool,
            upstream_client,
            max_retries,
        }
    }

    /// Process a proxy request with automatic key rotation and retry logic
    pub async fn proxy_request(
        &self,
        method: Method,
        path: String,
        headers: HeaderMap,
        body: Bytes,
    ) -> ProxyResult<Response<Body>> {
        debug!("Processing {} request to {}", method, path);

        // Parse request to extract model if it's a JSON body
        let model = if method == Method::POST && !body.is_empty() {
            self.extract_model_from_body(&body)?
        } else {
            "others".to_string() // Default for non-POST requests
        };

        debug!("Extracted model: {}", model);

        // Attempt the request with retries
        let mut attempt_count = 0;
        let mut last_error = None;
        let mut use_next_key = false;

        while attempt_count <= self.max_retries {
            // Get appropriate API key for the model
            // On first attempt, use model-specific key; on retries, rotate through all keys
            let key_info = if use_next_key {
                match self.key_pool.get_next_key() {
                    Some(key) => key,
                    None => {
                        return Err(ProxyError::NoKeyAvailable {
                            model: model.clone(),
                        })
                    }
                }
            } else {
                match self.key_pool.get_key_for_model(&model) {
                    Some(key) => key,
                    None => {
                        return Err(ProxyError::NoKeyAvailable {
                            model: model.clone(),
                        })
                    }
                }
            };

            info!(
                "Forwarding to {} with API key (redacted) - attempt {}",
                key_info.url,
                attempt_count + 1
            );

            // Make the upstream request
            match self
                .upstream_client
                .forward_request(
                    convert_axum_method_to_reqwest(&method),
                    key_info.clone(),
                    &path,
                    Some(body.clone()),
                    Some(convert_axum_headers_to_reqwest(&headers)),
                )
                .await
            {
                Ok(response) => {
                    let status = response.status();
                    debug!("Received response from upstream. Status: {}", status);

                    // Check if we should rotate the key due to the response
                    if should_rotate_key(status) {
                        warn!(
                            "Error from upstream ({}). Changing API key and retrying.",
                            status
                        );
                        attempt_count += 1;
                        use_next_key = true; // Switch to using get_next_key for retries

                        last_error = Some(ProxyError::UpstreamFailed {
                            source: response.error_for_status_ref().unwrap_err(),
                        });
                        continue;
                    }

                    // Success - convert reqwest::Response to axum::Response
                    return self.convert_response(response).await;
                }
                Err(e) => {
                    error!("Error sending request to upstream: {}", e);
                    last_error = Some(e);
                    attempt_count += 1;
                    use_next_key = true; // Switch to using get_next_key for retries
                }
            }
        }

        // All retries exhausted
        error!("All API keys have been tried for model '{}'", model);
        Err(last_error.unwrap_or(ProxyError::AllRetriesExhausted))
    }

    /// Extract model from request body for routing decisions
    fn extract_model_from_body(&self, body: &Bytes) -> ProxyResult<String> {
        let request: OpenAIRequest = serde_json::from_slice(body)?;
        Ok(request.model)
    }

    /// Convert reqwest::Response to axum::Response
    async fn convert_response(&self, response: reqwest::Response) -> ProxyResult<Response<Body>> {
        let status = StatusCode::from_u16(response.status().as_u16())
            .map_err(|e| ProxyError::internal(format!("Invalid status code: {}", e)))?;

        let mut builder = Response::builder().status(status);

        // Copy headers from upstream response
        let reqwest_headers = response.headers();
        let axum_headers = convert_reqwest_headers_to_axum(reqwest_headers);
        for (name, value) in &axum_headers {
            builder = builder.header(name, value);
        }

        // Handle streaming response body
        let body_stream = response.bytes_stream();
        let body =
            Body::from_stream(body_stream.map(|chunk| chunk.map_err(std::io::Error::other)));

        builder
            .body(body)
            .map_err(|e| ProxyError::internal(format!("Failed to build response: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ApiKeyInfo, UpstreamConfig};
    use secrecy::SecretString;

    fn create_test_key(id: &str, models: Vec<&str>) -> ApiKeyInfo {
        ApiKeyInfo {
            key: SecretString::new(format!("test-key-{}", id)),
            url: format!("https://api-{}.example.com", id),
            models: models.into_iter().map(String::from).collect(),
            latency: None,
            health_score: 1.0,
        }
    }

    #[test]
    fn test_extract_model_from_body() {
        let keys = vec![create_test_key("1", vec!["gpt-3.5-turbo"])];
        let key_pool = Arc::new(KeyPool::new(keys, "round_robin"));
        let upstream_config = UpstreamConfig::default();
        let upstream_client = UpstreamClient::new(upstream_config).unwrap();
        let engine = ProxyEngine::new(key_pool, upstream_client, 3);

        // Test valid JSON with model
        let body = r#"{"model": "gpt-3.5-turbo", "messages": []}"#;
        let body_bytes = Bytes::from(body);
        let model = engine.extract_model_from_body(&body_bytes).unwrap();
        assert_eq!(model, "gpt-3.5-turbo");

        // Test invalid JSON
        let invalid_body = Bytes::from("invalid json");
        let result = engine.extract_model_from_body(&invalid_body);
        assert!(result.is_err());
    }
}
