pub mod detect;
pub mod litellm;
pub mod openai_audit;
pub mod otel_genai;
pub mod raw_text;
pub mod sse;

use crate::model::LogEvent;

pub struct ParseOutcome {
    pub events: Vec<LogEvent>,
    pub warnings: Vec<String>,
}

pub fn parse(name: &str, content: &str) -> ParseOutcome {
    match detect::detect(name, content) {
        detect::Format::OperatorLm => openai_audit::parse(content),
        detect::Format::OtelGenAi => otel_genai::parse(content),
        detect::Format::LiteLLM => litellm::parse(content),
        detect::Format::RawText => raw_text::parse(content),
    }
}

/// Dispatch a top-level JSON value: pass each object to `f`, accumulating
/// events and warnings. Handles both a JSONL stream (one object per line) and
/// a JSON array of objects.
pub(crate) fn parse_jsonl_or_array<F>(content: &str, mut f: F) -> ParseOutcome
where
    F: FnMut(&serde_json::Value) -> Result<LogEvent, String>,
{
    let mut events = Vec::new();
    let mut warnings = Vec::new();
    let trimmed = content.trim_start();
    if trimmed.starts_with('[') {
        match serde_json::from_str::<serde_json::Value>(content) {
            Ok(serde_json::Value::Array(arr)) => {
                for (idx, v) in arr.iter().enumerate() {
                    match f(v) {
                        Ok(e) => events.push(e),
                        Err(err) => warnings.push(format!("item {}: {}", idx, err)),
                    }
                }
            }
            Ok(_) => warnings.push("expected a JSON array".into()),
            Err(e) => warnings.push(format!("invalid json: {}", e)),
        }
    } else {
        for (idx, line) in content.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<serde_json::Value>(line) {
                Ok(v) => match f(&v) {
                    Ok(e) => events.push(e),
                    Err(err) => warnings.push(format!("line {}: {}", idx + 1, err)),
                },
                Err(e) => warnings.push(format!("line {}: invalid json: {}", idx + 1, e)),
            }
        }
    }
    ParseOutcome { events, warnings }
}
