use reqwest::Client;
use serde_json::{json, Value};
use std::{env, time::Duration};

#[derive(Clone)]
pub struct GeminiClient {
    client: Client,
    api_key: String,
    model: String,
}

impl GeminiClient {
    pub fn new() -> Result<Self, String> {
        let api_key = env::var("GEMINI_API_KEY")
            .map_err(|_| "GEMINI_API_KEY is not set in .env".to_string())?;

        // GEMINI_MODEL from the environment/.env is used first.
        // If it is not set, use the lightweight model.
        let model =
            env::var("GEMINI_MODEL").unwrap_or_else(|_| "gemini-3.5-flash-lite".to_string());

        if api_key.trim().is_empty() || api_key == "YOUR_ACTUAL_API_KEY" {
            return Err("A valid GEMINI_API_KEY is required.".to_string());
        }

        Ok(Self {
            client: Client::new(),
            api_key,
            model,
        })
    }

    /// Return the configured Gemini model name.
    pub fn model(&self) -> &str {
        &self.model
    }

    // ============================================================
    // NORMAL TEXT GENERATION
    // ============================================================

    /// Generate normal text from Gemini.
    pub async fn generate(&self, prompt: &str) -> Result<String, String> {
        let response = self.send_request(prompt).await?;
        self.extract_text(&response)
    }

    // ============================================================
    // JSON GENERATION
    // ============================================================

    /// Generate structured JSON from Gemini.
    pub async fn generate_json(&self, prompt: &str) -> Result<Value, String> {
        let response = self.send_request(prompt).await?;

        let text = self.extract_text(&response)?;
        let cleaned = Self::clean_json_response(&text);

        serde_json::from_str::<Value>(&cleaned).map_err(|error| {
            format!(
                "Gemini returned invalid JSON: {}. Response: {}",
                error, text
            )
        })
    }

    // ============================================================
    // REVISION SUPPORT
    // ============================================================

    /// Ask Gemini to analyze a failed verification and
    /// generate a correction proposal.
    ///
    /// The response is returned as structured JSON so the
    /// revision loop can process it safely.
    pub async fn generate_revision(
        &self,
        task: &str,
        verification_command: &str,
        verification_output: &str,
        current_code: &str,
    ) -> Result<Value, String> {
        let prompt = format!(
            r#"
You are the correction engine of an AI Coding Agent.

A generated coding task was implemented, but verification failed.

Your job is to analyze the failure and propose a precise correction.

IMPORTANT RULES:

1. Do not invent errors that are not present in the verification output.
2. Analyze the actual verification output carefully.
3. Identify the likely root cause.
4. Provide the exact file that should be changed when possible.
5. Provide a complete replacement for the affected file or a precise patch.
6. Do not modify unrelated files.
7. Do not remove working functionality.
8. The correction must address the verification failure.
9. Verification commands must be fully non-interactive (never wait for keyboard input; pipe stdin via printf or echo if needed).
10. Return ONLY valid JSON.
11. Do not wrap the JSON in Markdown code fences.

ORIGINAL TASK:
{task}

VERIFICATION COMMAND:
{verification_command}

VERIFICATION OUTPUT:
{verification_output}

CURRENT CODE:
{current_code}

Return JSON using exactly this structure:

{{
    "analysis": "Explain the verification failure.",
    "root_cause": "Explain the likely root cause.",
    "action": "Describe what should be changed.",
    "files": [
        {{
            "path": "relative/path/to/file",
            "operation": "modify",
            "content": "complete corrected file content"
        }}
    ],
    "verification_command": "{verification_command}"
}}
"#,
        );

        self.generate_json(&prompt).await
    }

    // ============================================================
    // GEMINI REST API
    // ============================================================

    /// Send a request to the Gemini REST API.
    async fn send_request(&self, prompt: &str) -> Result<Value, String> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        );

        // Gemini 3.x does not need the old temperature setting.
        //
        // responseMimeType forces Gemini to return JSON instead
        // of normal conversational text.
        let request_body = json!({
            "contents": [
                {
                    "parts": [
                        {
                            "text": prompt
                        }
                    ]
                }
            ],
            "generationConfig": {
                "responseMimeType": "application/json"
            }
        });

        const MAX_ATTEMPTS: usize = 4;

        for attempt in 1..=MAX_ATTEMPTS {
            let response = self
                .client
                .post(&url)
                .json(&request_body)
                .send()
                .await
                .map_err(|error| format!("Gemini request failed: {}", error))?;

            let status = response.status();

            let body = response
                .text()
                .await
                .map_err(|error| format!("Failed to read Gemini response: {}", error))?;

            // ----------------------------------------------------
            // Successful response
            // ----------------------------------------------------

            if status.is_success() {
                return serde_json::from_str::<Value>(&body)
                    .map_err(|error| format!("Invalid Gemini JSON response: {}", error));
            }

            // ----------------------------------------------------
            // Temporary errors
            // ----------------------------------------------------

            let temporary_error =
                status.as_u16() == 429 || status.as_u16() == 500 || status.as_u16() == 503;

            if temporary_error && attempt < MAX_ATTEMPTS {
                let delay_seconds = 2_u64.pow((attempt - 1) as u32);

                eprintln!(
                    "Gemini returned {}. Retry {}/{} in {} seconds...",
                    status,
                    attempt,
                    MAX_ATTEMPTS - 1,
                    delay_seconds
                );

                tokio::time::sleep(Duration::from_secs(delay_seconds)).await;

                continue;
            }

            // ----------------------------------------------------
            // 503 Service Unavailable
            // ----------------------------------------------------

            if status.as_u16() == 503 {
                return Err(format!(
                    "Gemini API returned 503 Service Unavailable. \
The selected model '{}' is temporarily unavailable or experiencing high demand. \
Please try again later or use another available Gemini model. \
Response: {}",
                    self.model, body
                ));
            }

            // ----------------------------------------------------
            // 429 Rate Limit
            // ----------------------------------------------------

            if status.as_u16() == 429 {
                // Attempt to extract a retry delay from the response body.
                let retry_hint = serde_json::from_str::<Value>(&body)
                    .ok()
                    .and_then(|v| {
                        v.get("error")
                            .and_then(|e| e.get("details"))
                            .and_then(|d| d.as_array())
                            .and_then(|arr| {
                                arr.iter().find_map(|item| {
                                    item.get("retryDelay")
                                        .and_then(|r| r.as_str())
                                        .map(|s| s.to_string())
                                })
                            })
                    });

                let retry_msg = match retry_hint {
                    Some(delay) => format!(" Retry after approximately {}.", delay),
                    None => String::new(),
                };

                return Err(format!(
                    "Gemini API quota exceeded (429 Too Many Requests). \
Model: '{}'.{} \
Please wait for the quota reset or configure a Gemini API key/model with available quota.",
                    self.model, retry_msg
                ));
            }

            // ----------------------------------------------------
            // Authentication
            // ----------------------------------------------------

            if status.as_u16() == 401 || status.as_u16() == 403 {
                return Err(format!(
                    "Gemini API authentication/permission error ({}). \
Check GEMINI_API_KEY and model access. Response: {}",
                    status, body
                ));
            }

            // ----------------------------------------------------
            // Model not found
            // ----------------------------------------------------

            if status.as_u16() == 404 {
                return Err(format!(
                    "Gemini model '{}' was not found or is not available \
for this API endpoint. Response: {}",
                    self.model, body
                ));
            }

            // ----------------------------------------------------
            // Other API errors
            // ----------------------------------------------------

            return Err(format!("Gemini API returned {}: {}", status, body));
        }

        Err("Gemini request failed after all retry attempts.".to_string())
    }

    // ============================================================
    // RESPONSE TEXT EXTRACTION
    // ============================================================

    /// Extract generated text from Gemini's response.
    fn extract_text(&self, response: &Value) -> Result<String, String> {
        response
            .get("candidates")
            .and_then(|candidates| candidates.get(0))
            .and_then(|candidate| candidate.get("content"))
            .and_then(|content| content.get("parts"))
            .and_then(|parts| parts.get(0))
            .and_then(|part| part.get("text"))
            .and_then(|text| text.as_str())
            .map(|text| text.to_string())
            .ok_or_else(|| {
                format!(
                    "Gemini response did not contain generated text: {}",
                    response
                )
            })
    }

    // ============================================================
    // JSON CLEANING
    // ============================================================

    /// Remove Markdown code fences if Gemini wraps JSON in them.
    fn clean_json_response(text: &str) -> String {
        let trimmed = text.trim();

        // ```json
        // {...}
        // ```
        if trimmed.starts_with("```json") && trimmed.ends_with("```") {
            return trimmed
                .trim_start_matches("```json")
                .trim_end_matches("```")
                .trim()
                .to_string();
        }

        // ```
        // {...}
        // ```
        if trimmed.starts_with("```") && trimmed.ends_with("```") {
            return trimmed
                .trim_start_matches("```")
                .trim_end_matches("```")
                .trim()
                .to_string();
        }

        trimmed.to_string()
    }
}
