pub enum Format {
    /// operatorlm audit envelope: `{id, ts, req_body, resp_body, ...}` either
    /// as JSONL or as a JSON array.
    OperatorLm,
    /// OpenTelemetry GenAI spans, either as bare JSONL spans or as an OTLP/JSON
    /// `resourceSpans` envelope.
    OtelGenAi,
    /// LiteLLM StandardLoggingPayload, JSONL or JSON array.
    LiteLLM,
    /// Anything else — `raw_text` scans for embedded operatorlm objects.
    RawText,
}

/// Detect the format of a log file by sniffing its contents. The filename is
/// intentionally ignored — users dump these files with all sorts of
/// extensions (.log, .jsonl, .json, .ndjson).
pub fn detect(_name: &str, content: &str) -> Format {
    let trimmed = content.trim_start();

    if trimmed.starts_with('[') {
        if let Some(first) = first_array_element(trimmed) {
            if let Some(f) = sniff_known_object(&first) {
                return f;
            }
        }
        // Unknown array shape — try operatorlm; it will warn per-item.
        return Format::OperatorLm;
    }

    if trimmed.starts_with('{') {
        // Keep sniffing past unmarked lines: OTel exports usually open with a
        // root span (e.g. `agent.run`) that carries no gen_ai.* attributes.
        let mut saw_object = false;
        for line in content.lines().take(8) {
            let l = line.trim();
            if l.is_empty() {
                continue;
            }
            if !(l.starts_with('{') && l.ends_with('}')) {
                break;
            }
            if let Some(f) = sniff_known_object(l) {
                return f;
            }
            saw_object = true;
        }
        if saw_object {
            return Format::OperatorLm;
        }
    }

    Format::RawText
}

/// Return Some(Format) if the given JSON-object literal carries marker keys of
/// a known schema. Cheap substring tests — we don't parse here.
fn sniff_known_object(s: &str) -> Option<Format> {
    if s.contains("\"gen_ai.") || s.contains("\"resourceSpans\"") {
        return Some(Format::OtelGenAi);
    }
    if s.contains("\"call_type\"")
        && (s.contains("\"custom_llm_provider\"") || s.contains("\"api_base\""))
    {
        return Some(Format::LiteLLM);
    }
    if s.contains("\"req_body\"") && s.contains("\"id\"") {
        return Some(Format::OperatorLm);
    }
    None
}

/// Extract the first object literal inside a JSON array string, without doing
/// a full parse.
fn first_array_element(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b'[' {
        return None;
    }
    i += 1;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b'{' {
        return None;
    }
    let start = i;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    while i < bytes.len() {
        let c = bytes[i];
        if in_str {
            if esc {
                esc = false;
            } else if c == b'\\' {
                esc = true;
            } else if c == b'"' {
                in_str = false;
            }
        } else if c == b'"' {
            in_str = true;
        } else if c == b'{' {
            depth += 1;
        } else if c == b'}' {
            depth -= 1;
            if depth == 0 {
                return Some(s[start..=i].to_string());
            }
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_operatorlm_jsonl() {
        let line = r#"{"id":"abc","ts":"2026-01-01T00:00:00Z","req_body":{}}"#;
        assert!(matches!(detect("audit.log", line), Format::OperatorLm));
    }

    #[test]
    fn detects_otel_genai_jsonl() {
        let line = r#"{"spanId":"x","attributes":{"gen_ai.system":"openai","gen_ai.request.model":"gpt-4"}}"#;
        assert!(matches!(detect("traces.jsonl", line), Format::OtelGenAi));
    }

    #[test]
    fn detects_otel_otlp_envelope() {
        let line = r#"{"resourceSpans":[{"scopeSpans":[{"spans":[]}]}]}"#;
        assert!(matches!(detect("otlp.json", line), Format::OtelGenAi));
    }

    #[test]
    fn detects_litellm() {
        let line = r#"{"id":"x","call_type":"completion","custom_llm_provider":"openai","model":"gpt-4","messages":[]}"#;
        assert!(matches!(detect("litellm.jsonl", line), Format::LiteLLM));
    }

    #[test]
    fn falls_back_to_raw_text() {
        assert!(matches!(detect("a.txt", "hello world\n"), Format::RawText));
    }

    #[test]
    fn json_array_of_operatorlm_records() {
        let s = r#"[{"id":"a","req_body":{},"ts":"x"}]"#;
        assert!(matches!(detect("x.json", s), Format::OperatorLm));
    }

    #[test]
    fn json_array_of_otel_spans() {
        let s = r#"[{"spanId":"x","attributes":{"gen_ai.system":"openai"}}]"#;
        assert!(matches!(detect("spans.json", s), Format::OtelGenAi));
    }
}
