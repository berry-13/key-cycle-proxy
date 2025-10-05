use crate::proxy::{engine::ProxyEngine, error::ProxyResult};
use axum::{
    body::Body,
    extract::{Path, Request, State},
    http::{HeaderMap, Method, StatusCode},
    response::Response,
};
use bytes::Bytes;
use std::sync::Arc;
use tracing::{debug, error};

#[derive(Debug, Clone)]
pub struct ProxyHandler {
    engine: Arc<ProxyEngine>,
}

impl ProxyHandler {
    pub fn new(engine: Arc<ProxyEngine>) -> Self {
        Self { engine }
    }

    /// Handle all OpenAI API requests
    pub async fn handle_request(
        State(handler): State<Arc<ProxyHandler>>,
        method: Method,
        uri: axum::http::Uri,
        headers: HeaderMap,
        body: Bytes,
    ) -> ProxyResult<Response<Body>> {
        debug!("Received {} request: {}", method, uri);

        // Only allow POST requests (matching the original Node.js behavior)
        if method != Method::POST {
            error!("Method not allowed: {}", method);
            return Err(crate::proxy::error::ProxyError::MethodNotAllowed);
        }

        // Extract the path from the URI
        let path = uri.path().to_string();

        // Forward the request to the proxy engine
        handler
            .engine
            .proxy_request(method, path, headers, body)
            .await
    }

    /// Handle requests with path parameters (for /v1/* routes)
    #[allow(dead_code)]
    pub async fn handle_v1_request(
        State(handler): State<Arc<ProxyHandler>>,
        Path(path): Path<String>,
        method: Method,
        headers: HeaderMap,
        body: Bytes,
    ) -> ProxyResult<Response<Body>> {
        debug!("Received {} request to /v1/{}", method, path);

        // Only allow POST requests
        if method != Method::POST {
            return Err(crate::proxy::error::ProxyError::MethodNotAllowed);
        }

        let full_path = format!("/v1/{}", path);

        // Forward the request to the proxy engine
        handler
            .engine
            .proxy_request(method, full_path, headers, body)
            .await
    }

    /// Health check endpoint
    pub async fn health_check() -> Result<&'static str, StatusCode> {
        Ok("OK")
    }
}

/// Extract the request body as bytes
#[allow(dead_code)]
pub async fn extract_body(request: Request) -> Result<Bytes, StatusCode> {
    match axum::body::to_bytes(request.into_body(), usize::MAX).await {
        Ok(bytes) => Ok(bytes),
        Err(e) => {
            error!("Failed to read request body: {}", e);
            Err(StatusCode::BAD_REQUEST)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health_check() {
        let result = ProxyHandler::health_check().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "OK");
    }
}
