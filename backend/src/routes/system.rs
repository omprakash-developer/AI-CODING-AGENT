use axum::Json;
use serde::Serialize;
use std::env;

// ============================================================
// RESPONSE TYPES
// ============================================================

#[derive(Debug, Serialize)]
pub struct SystemStatus {
    pub backend: &'static str,
    pub model: String,
    pub transport: &'static str,
}

// ============================================================
// GET /system/status
// ============================================================

/// Return the current system status including the actual
/// Gemini model configured in the backend environment.
///
/// The model name is read from the `GEMINI_MODEL` environment
/// variable (the same source used by `GeminiClient::new()`).
/// If the variable is not set, the same default fallback
/// (`gemini-3.5-flash-lite`) is used so the value displayed
/// in the UI is always consistent with what the backend uses.
pub async fn system_status() -> Json<SystemStatus> {
    let model =
        env::var("GEMINI_MODEL").unwrap_or_else(|_| "gemini-3.5-flash-lite".to_string());

    Json(SystemStatus {
        backend: "online",
        model,
        transport: "SSE",
    })
}
