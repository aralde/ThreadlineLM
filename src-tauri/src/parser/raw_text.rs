use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;

use super::ParseOutcome;
use crate::parser::openai_audit;

static OBJECT_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"\{[\s\S]*?"id"[\s\S]*?"req_body"[\s\S]*?\}\s*$"#).unwrap()
});

pub fn parse(content: &str) -> ParseOutcome {
    let mut events = Vec::new();
    let mut warnings = Vec::new();

    // Strategy: scan for balanced JSON objects with the expected shape.
    for cand in balanced_json_objects(content) {
        if let Ok(v) = serde_json::from_str::<Value>(&cand) {
            if v.get("req_body").is_some() && v.get("id").is_some() {
                let single = format!("{}\n", cand);
                let mut out = openai_audit::parse_jsonl(&single);
                events.append(&mut out.events);
                warnings.append(&mut out.warnings);
            }
        }
    }

    if events.is_empty() {
        warnings.push(
            "raw_text parser found no recognizable audit records; \
             only OpenAI-compatible audit JSON is supported in the MVP"
                .into(),
        );
    }
    let _ = &*OBJECT_RE; // suppress unused warning if regex strategy is enabled later
    ParseOutcome { events, warnings }
}

fn balanced_json_objects(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            let mut depth = 0i32;
            let mut in_str = false;
            let mut esc = false;
            let start = i;
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
                        out.push(s[start..=i].to_string());
                        i += 1;
                        break;
                    }
                }
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    out
}
