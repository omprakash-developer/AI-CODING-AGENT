use axum::{extract::State, http::StatusCode, Json};

use serde::Serialize;

use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct GraphTestResponse {
    pub success: bool,
    pub message: String,
}

pub async fn test_graph(
    State(state): State<AppState>,
) -> Result<Json<GraphTestResponse>, (StatusCode, String)> {
    state
        .kuzu
        .test_graph()
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;

    Ok(Json(GraphTestResponse {
        success: true,
        message: "Kuzu code graph test completed successfully.".to_string(),
    }))
}
