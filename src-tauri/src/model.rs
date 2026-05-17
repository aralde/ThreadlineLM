use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    OpenAI,
    Anthropic,
    Gemini,
    Groq,
    Mistral,
    Cohere,
    Ollama,
    DeepSeek,
    Xai,
    Azure,
    Unknown,
}

impl Provider {
    pub fn from_prefix(model: &str) -> Self {
        let lower = model.to_lowercase();
        let prefix = lower.split('/').next().unwrap_or(&lower);
        match prefix {
            "openai" | "azure-openai" => Provider::OpenAI,
            "anthropic" | "claude" | "bedrock-anthropic" => Provider::Anthropic,
            "gemini" | "google" | "vertex" | "vertex_ai" | "vertexai" => Provider::Gemini,
            "groq" => Provider::Groq,
            "mistral" | "mistral_ai" => Provider::Mistral,
            "cohere" => Provider::Cohere,
            "ollama" => Provider::Ollama,
            "deepseek" => Provider::DeepSeek,
            "xai" | "grok" => Provider::Xai,
            "azure" => Provider::Azure,
            "bedrock" | "aws" | "aws.bedrock" => Provider::Unknown, // model substring decides
            _ => Provider::Unknown,
        }
    }

    /// Map an OpenTelemetry GenAI `gen_ai.system` attribute to a Provider.
    /// Spec values: openai, anthropic, vertex_ai, gcp.gemini, gcp.gen_ai,
    /// aws.bedrock, cohere, mistral_ai, groq, perplexity, deepseek, xai,
    /// az.ai.openai, az.ai.inference, ibm.watsonx.ai.
    pub fn from_otel_system(system: &str) -> Self {
        match system.to_lowercase().as_str() {
            "openai" | "az.ai.openai" => Provider::OpenAI,
            "anthropic" => Provider::Anthropic,
            "vertex_ai" | "gcp.gemini" | "gcp.gen_ai" | "gcp.vertex_ai" => Provider::Gemini,
            "groq" => Provider::Groq,
            "mistral_ai" => Provider::Mistral,
            "cohere" => Provider::Cohere,
            "deepseek" => Provider::DeepSeek,
            "xai" => Provider::Xai,
            "aws.bedrock" => Provider::Unknown,
            _ => Provider::Unknown,
        }
    }

    pub fn from_upstream_url(url: &str) -> Option<Self> {
        let u = url.to_lowercase();
        if u.contains("openai.com") {
            Some(Provider::OpenAI)
        } else if u.contains("anthropic.com") {
            Some(Provider::Anthropic)
        } else if u.contains("generativelanguage.googleapis.com")
            || u.contains("vertexai")
            || u.contains("aiplatform.googleapis.com")
        {
            Some(Provider::Gemini)
        } else if u.contains("groq.com") {
            Some(Provider::Groq)
        } else if u.contains("mistral.ai") {
            Some(Provider::Mistral)
        } else if u.contains("cohere.com") || u.contains("cohere.ai") {
            Some(Provider::Cohere)
        } else if u.contains("ollama") || u.contains("11434") {
            Some(Provider::Ollama)
        } else if u.contains("deepseek") {
            Some(Provider::DeepSeek)
        } else if u.contains("x.ai") || u.contains("grok") {
            Some(Provider::Xai)
        } else if u.contains("azure.com") {
            Some(Provider::Azure)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Upstream {
    pub url: String,
    pub status: Option<u16>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RequestPayload {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
    pub tools_declared: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResponsePayload {
    pub assistant: Option<ChatMessage>,
    pub finish_reason: Option<String>,
    pub usage: Usage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEvent {
    pub id: String,
    pub ts: String,
    pub client: String,
    pub ua: Option<String>,
    pub provider: Provider,
    pub model: String,
    pub endpoint: String,
    pub request: RequestPayload,
    pub response: ResponsePayload,
    pub upstream: Option<Upstream>,
    pub status: u16,
    pub duration_ms: u64,
    pub raw: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceFile {
    pub path: String,
    pub name: String,
    pub bytes: u64,
    pub events: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationTurn {
    pub event_id: String,
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub client: String,
    pub started_at: String,
    pub ended_at: String,
    pub event_ids: Vec<String>,
    pub conversation: Vec<ConversationTurn>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Workspace {
    pub files: Vec<SourceFile>,
    pub events: Vec<LogEvent>,
    pub sessions: Vec<Session>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadResult {
    pub workspace: Workspace,
    pub warnings: Vec<String>,
}
