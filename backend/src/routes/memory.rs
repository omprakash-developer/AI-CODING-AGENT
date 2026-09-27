use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};

use serde::Deserialize;

use crate::{gemini::GeminiEmbeddingClient, state::AppState};

#[derive(Debug, Deserialize)]
pub struct MemorySearchQuery {
    pub q: String,
}

/// GET /memory/search?q=...
///
/// Performs semantic memory search using:
/// 1. Gemini embedding
/// 2. sqlite-vec similarity search
pub async fn search_memory(
    State(state): State<AppState>,
    Query(query): Query<MemorySearchQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let search_text = query.q.trim();

    // ---------------------------------------------------------
    // Validate query
    // ---------------------------------------------------------

    if search_text.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "Search query cannot be empty."
            })),
        ));
    }

    // ---------------------------------------------------------
    // Create Gemini embedding client
    // ---------------------------------------------------------

    let embedding_client = match GeminiEmbeddingClient::new() {
        Ok(client) => client,

        Err(error) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": error
                })),
            ));
        }
    };

    // ---------------------------------------------------------
    // Generate embedding
    // ---------------------------------------------------------

    let embedding = match embedding_client.embed_query(search_text).await {
        Ok(embedding) => embedding,

        Err(error) => {
            return Err((
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({
                    "error": format!(
                        "Failed to generate search embedding: {}",
                        error
                    )
                })),
            ));
        }
    };

    // ---------------------------------------------------------
    // Search sqlite-vec
    // ---------------------------------------------------------

    let results = match state.database.search_memory_vectors(&embedding, 10) {
        Ok(results) => results,

        Err(error) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!(
                        "Semantic memory search failed: {}",
                        error
                    )
                })),
            ));
        }
    };

    // ---------------------------------------------------------
    // Return results as JSON value
    // ---------------------------------------------------------

    match serde_json::to_value(results) {
        Ok(json_results) => Ok(Json(json_results)),

        Err(error) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!(
                    "Failed to serialize memory results: {}",
                    error
                )
            })),
        )),
    }
}
