use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// OpenAI API request payload - we only need the model field for routing
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OpenAIRequest {
    pub model: String,
    #[serde(flatten)]
    pub other: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[allow(dead_code)]
pub struct OpenAIError {
    pub error: OpenAIErrorDetails,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[allow(dead_code)]
pub struct OpenAIErrorDetails {
    pub message: String,
    #[serde(rename = "type")]
    pub error_type: Option<String>,
    pub code: Option<String>,
}

/// Standard error response format
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

impl ErrorResponse {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            error: message.into(),
        }
    }
}
