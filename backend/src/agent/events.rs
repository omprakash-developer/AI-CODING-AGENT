use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum AgentEvent {
    Plan { message: String },

    ToolCall { tool: String, input: String },

    Diff { file: String, content: String },

    Test { command: String, output: String },

    Verify { message: String, success: bool },

    Completed { message: String },

    Error { message: String },
}

impl AgentEvent {
    pub fn plan(message: impl Into<String>) -> Self {
        Self::Plan {
            message: message.into(),
        }
    }

    pub fn tool_call(tool: impl Into<String>, input: impl Into<String>) -> Self {
        Self::ToolCall {
            tool: tool.into(),
            input: input.into(),
        }
    }

    pub fn diff(file: impl Into<String>, content: impl Into<String>) -> Self {
        Self::Diff {
            file: file.into(),
            content: content.into(),
        }
    }

    pub fn test(command: impl Into<String>, output: impl Into<String>) -> Self {
        Self::Test {
            command: command.into(),
            output: output.into(),
        }
    }

    pub fn verify(message: impl Into<String>, success: bool) -> Self {
        Self::Verify {
            message: message.into(),
            success,
        }
    }

    pub fn completed(message: impl Into<String>) -> Self {
        Self::Completed {
            message: message.into(),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
        }
    }
}
