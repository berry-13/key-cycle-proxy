use crate::proxy::ProxyHandler;
use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{any, get},
    Router,
};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use std::time::Duration;

pub fn create_router(
    handler: Arc<ProxyHandler>, 
    body_limit: usize,
    request_timeout: Duration,
) -> Router {
    Router::new()
        // Health check endpoint
        .route("/health", get(ProxyHandler::health_check))
        
        // Catch-all for OpenAI API requests (maintaining compatibility with existing paths)
        .route("/*path", any(proxy_request_handler))
        
        // Add the handler as application state
        .with_state(handler)
        
        // Add middleware layers
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_headers(Any)
                .allow_methods(Any),
        )
        .layer(RequestBodyLimitLayer::new(body_limit))
        .layer(TimeoutLayer::new(request_timeout))
        .layer(TraceLayer::new_for_http())
}

/// Handler that processes all proxy requests
async fn proxy_request_handler(
    State(handler): State<Arc<ProxyHandler>>,
    method: Method,
    uri: axum::http::Uri,
    headers: HeaderMap,
    request: Request,
) -> Result<Response<Body>, StatusCode> {
    // Extract body from request
    let body = match axum::body::to_bytes(request.into_body(), usize::MAX).await {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::error!("Failed to read request body: {}", e);
            return Err(StatusCode::BAD_REQUEST);
        }
    };

    // Forward to the proxy handler
    match ProxyHandler::handle_request(State(handler), method, uri, headers, body).await {
        Ok(response) => Ok(response),
        Err(proxy_error) => {
            // Convert ProxyError to HTTP response
            Ok(proxy_error.into_response())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ApiKeyInfo, UpstreamConfig};
    use crate::proxy::{KeyPool, ProxyEngine, UpstreamClient};
    use axum::http::{Request, StatusCode};
    use axum::body::Body;
    use secrecy::SecretString;
    use tower::ServiceExt;

    fn create_test_app() -> Router {
        let key = ApiKeyInfo {
            key: SecretString::new("test-key".to_string()),
            url: "https://api.test.com".to_string(),
            models: vec!["gpt-3.5-turbo".to_string()],
            latency: None,
            health_score: 1.0,
        };

        let key_pool = Arc::new(KeyPool::new(vec![key], "round_robin"));
        let upstream_config = UpstreamConfig::default();
        let upstream_client = UpstreamClient::new(upstream_config).unwrap();
        let engine = Arc::new(ProxyEngine::new(key_pool, upstream_client, 3));
        let handler = Arc::new(ProxyHandler::new(engine));

        create_router(handler, 1024 * 1024, Duration::from_secs(30))
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let app = create_test_app();

        let request = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_method_not_allowed() {
        let app = create_test_app();

        let request = Request::builder()
            .method("GET")
            .uri("/v1/chat/completions")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        // Should return method not allowed since we only accept POST
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }
}