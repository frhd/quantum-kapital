use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmKind {
    Thesis,
    Decay,
    News,
    Ranker,
}

impl LlmKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            LlmKind::Thesis => "thesis",
            LlmKind::Decay => "decay",
            LlmKind::News => "news",
            LlmKind::Ranker => "ranker",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SystemBlock {
    pub text: String,
    pub cache: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone)]
pub enum ToolChoice {
    Auto,
    ForceTool(String),
}

#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub kind: LlmKind,
    pub model: &'static str,
    pub max_tokens: u32,
    pub system: Vec<SystemBlock>,
    pub messages: Vec<Message>,
    pub tools: Option<Vec<ToolSchema>>,
    pub tool_choice: Option<ToolChoice>,
    pub setup_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub input: serde_json::Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_input_tokens: u32,
    pub cache_creation_input_tokens: u32,
}

#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub text: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Usage,
}
