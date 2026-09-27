use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Serialize)]
struct EmbeddingRequest {
    model: String,
    content: EmbeddingContent,
    #[serde(rename = "outputDimensionality")]
    output_dimensionality: usize,
    #[serde(rename = "taskType")]
    task_type: String,
}

#[derive(Debug, Serialize)]
struct EmbeddingContent {
    parts: Vec<EmbeddingPart>,
}

#[derive(Debug, Serialize)]
struct EmbeddingPart {
    text: String,
}

#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    embedding: EmbeddingValues,
}

#[derive(Debug, Deserialize)]
struct EmbeddingValues {
    values: Vec<f32>,
}

#[derive(Clone)]
pub struct GeminiEmbeddingClient {
    client: Client,
    api_key: String,
    model: String,
    dimensions: usize,
}

impl GeminiEmbeddingClient {
    pub fn new() -> Result<Self, String> {
        let api_key = env::var("GEMINI_API_KEY")
            .map_err(|_| "GEMINI_API_KEY is not configured.".to_string())?;

        let model = env::var("GEMINI_EMBEDDING_MODEL")
            .unwrap_or_else(|_| "gemini-embedding-001".to_string());

        let dimensions = env::var("GEMINI_EMBEDDING_DIMENSIONS")
            .unwrap_or_else(|_| "768".to_string())
            .parse::<usize>()
            .map_err(|_| "GEMINI_EMBEDDING_DIMENSIONS must be a number.".to_string())?;

        if dimensions != 768 {
            return Err(
                "This sqlite-vec memory configuration expects 768-dimensional embeddings."
                    .to_string(),
            );
        }

        Ok(Self {
            client: Client::new(),
            api_key,
            model,
            dimensions,
        })
    }

    pub async fn embed_document(&self, text: &str) -> Result<Vec<f32>, String> {
        self.embed(text, "RETRIEVAL_DOCUMENT").await
    }

    pub async fn embed_query(&self, text: &str) -> Result<Vec<f32>, String> {
        self.embed(text, "RETRIEVAL_QUERY").await
    }

    async fn embed(&self, text: &str, task_type: &str) -> Result<Vec<f32>, String> {
        if text.trim().is_empty() {
            return Err("Cannot create an embedding for empty text.".to_string());
        }

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:embedContent",
            self.model
        );

        let request = EmbeddingRequest {
            model: format!("models/{}", self.model),

            content: EmbeddingContent {
                parts: vec![EmbeddingPart {
                    text: text.to_string(),
                }],
            },

            output_dimensionality: self.dimensions,

            task_type: task_type.to_string(),
        };

        let response = self
            .client
            .post(url)
            .header("x-goog-api-key", &self.api_key)
            .json(&request)
            .send()
            .await
            .map_err(|error| format!("Embedding request failed: {}", error))?;

        let status = response.status();

        let body = response
            .text()
            .await
            .map_err(|error| format!("Failed to read embedding response: {}", error))?;

        if !status.is_success() {
            return Err(format!(
                "Gemini embedding API returned {}: {}",
                status, body
            ));
        }

        let result: EmbeddingResponse = serde_json::from_str(&body)
            .map_err(|error| format!("Failed to parse embedding response: {}", error))?;

        if result.embedding.values.len() != self.dimensions {
            return Err(format!(
                "Expected {} embedding dimensions, received {}.",
                self.dimensions,
                result.embedding.values.len()
            ));
        }

        Ok(result.embedding.values)
    }
}
