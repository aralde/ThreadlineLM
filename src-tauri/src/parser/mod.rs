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

#[cfg(test)]
mod tests {
    use super::detect::{detect, Format};
    use super::parse;
    use crate::model::Provider;

    fn load(name: &str) -> String {
        let path = format!("../inputExample/{name}");
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    fn assert_example(name: &str, expected: Format, events: usize) -> super::ParseOutcome {
        let content = load(name);
        let format = detect(name, &content);
        assert!(
            std::mem::discriminant(&format) == std::mem::discriminant(&expected),
            "{name}: unexpected format"
        );
        let out = parse(name, &content);
        assert!(out.warnings.is_empty(), "{name}: {:?}", out.warnings);
        assert_eq!(out.events.len(), events, "{name}: event count");
        out
    }

    #[test]
    fn example_audit_array() {
        let out = assert_example("audit-array.json", Format::OperatorLm, 3);
        assert!(matches!(out.events[0].provider, Provider::Xai));
    }

    #[test]
    fn example_otel_spans() {
        // The agent.run span has no gen_ai.* attributes and is skipped.
        let out = assert_example("otel-genai-spans.jsonl", Format::OtelGenAi, 3);
        let tool_turn = &out.events[0];
        let calls = tool_turn.response.assistant.as_ref().and_then(|m| m.tool_calls.as_ref());
        assert_eq!(calls.map(|c| c[0].name.as_str()), Some("search_docs"));
        assert_eq!(out.events[2].status, 500);
    }

    #[test]
    fn example_otel_otlp_envelope() {
        let out = assert_example("otel-genai-otlp.json", Format::OtelGenAi, 2);
        assert!(matches!(out.events[0].provider, Provider::Anthropic));
        assert_eq!(out.events[1].endpoint, "/v1/embeddings");
    }

    #[test]
    fn example_litellm() {
        let out = assert_example("litellm.jsonl", Format::LiteLLM, 5);
        let streamed = out.events[1].response.assistant.as_ref().unwrap();
        assert_eq!(streamed.content.as_deref(), Some("Bonjour le monde."));
        assert_eq!(out.events[4].status, 500);
    }

    #[test]
    fn example_raw_text() {
        assert_example("proxy-raw.log", Format::RawText, 2);
    }
}
