use axum::{
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::gemini::client::GeminiClient;

#[derive(Debug, Deserialize)]
pub struct GeminiTestRequest {
    pub prompt: String,
}

#[derive(Debug, Serialize)]
pub struct GeminiTestResponse {
    pub response: String,
}

pub async fn test_gemini(
    Json(request): Json<GeminiTestRequest>,
) -> Result<Json<GeminiTestResponse>, (StatusCode, String)> {
    let client = GeminiClient::new()
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Gemini client error: {}", error),
            )
        })?;

    let response = client
        .generate(&request.prompt)
        .await
        .map_err(|error| {
            (
                StatusCode::BAD_GATEWAY,
                format!("Gemini API error: {}", error),
            )
        })?;

    Ok(Json(GeminiTestResponse { response }))
}