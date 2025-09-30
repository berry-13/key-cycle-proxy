use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use crate::types::ErrorResponse;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProxyError {
    #[error("No API key available for model '{model}'")]
    NoKeyAvailable { model: String },
    
    #[error("No API key found")]
    NoKeyFound,
    
    #[error("Invalid API key")]
    InvalidApiKey,
    
    #[error("Upstream request failed: {source}")]
    UpstreamFailed {
        #[from]
        source: reqwest::Error,
    },
    
    #[error("Request timeout")]
    Timeout,
    
    #[error("Rate limit exceeded")]
    RateLimited,
    
    #[error("Invalid JSON payload: {source}")]
    InvalidJson {
        #[from]
        source: serde_json::Error,
    },
    
    #[error("Request body too large")]
    PayloadTooLarge,
    
    #[error("Method not allowed")]
    MethodNotAllowed,
    
    #[error("All retries exhausted")]
    AllRetriesExhausted,
    
    #[error("Internal server error: {message}")]
    Internal { message: String },
}

impl ProxyError {
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }
    
    pub fn status_code(&self) -> StatusCode {
        match self {
            ProxyError::NoKeyAvailable { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            ProxyError::NoKeyFound => StatusCode::INTERNAL_SERVER_ERROR,
            ProxyError::InvalidApiKey => StatusCode::INTERNAL_SERVER_ERROR,
            ProxyError::UpstreamFailed { source } => {
                // Map specific reqwest errors to appropriate status codes
                if source.is_timeout() {
                    StatusCode::GATEWAY_TIMEOUT
                } else if source.is_connect() {
                    StatusCode::BAD_GATEWAY
                } else {
                    StatusCode::BAD_GATEWAY
                }
            }
            ProxyError::Timeout => StatusCode::GATEWAY_TIMEOUT,
            ProxyError::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            ProxyError::InvalidJson { .. } => StatusCode::BAD_REQUEST,
            ProxyError::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            ProxyError::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            ProxyError::AllRetriesExhausted => StatusCode::BAD_GATEWAY,
            ProxyError::Internal { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let error_response = ErrorResponse::new(self.to_string());
        
        tracing::error!("Proxy error: {} (status: {})", self, status);
        
        (status, Json(error_response)).into_response()
    }
}

pub type ProxyResult<T> = Result<T, ProxyError>;