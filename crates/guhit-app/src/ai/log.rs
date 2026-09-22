//! Evaluation log: one JSON line per event in `<project_dir>/ai-log.jsonl`.
//! Holds the user intent, each tool call with its validated args and result
//! or error, latency, the proposal id and the outcome. Never holds the API
//! key, request headers or a full project dump.

use std::path::Path;

use guhit_model::defaults::now_rfc3339;
use serde_json::{json, Value};

pub const LOG_FILE: &str = "ai-log.jsonl";
/// Longest string kept in a log line. Tool results can be long lists.
const MAX_TEXT: usize = 600;

/// Shorten long strings inside a JSON value so a log line stays small.
pub fn compact(value: &Value) -> Value {
    match value {
        Value::String(s) if s.chars().count() > MAX_TEXT => {
            let cut: String = s.chars().take(MAX_TEXT).collect();
            Value::String(format!("{cut}... [{} chars]", s.chars().count()))
        }
        Value::Array(items) if items.len() > 40 => {
            let mut kept: Vec<Value> = items.iter().take(40).map(compact).collect();
            kept.push(json!(format!("... [{} items]", items.len())));
            Value::Array(kept)
        }
        Value::Array(items) => Value::Array(items.iter().map(compact).collect()),
        Value::Object(map) => Value::Object(map.iter().map(|(k, v)| (k.clone(), compact(v))).collect()),
        other => other.clone(),
    }
}

/// Append one event. Logging must never break a chat turn, so failures are
/// swallowed. `dir` None (no open project) skips the write.
pub async fn append(dir: Option<&Path>, event: &str, mut fields: Value) {
    let Some(dir) = dir else { return };
    if let Some(map) = fields.as_object_mut() {
        map.insert("event".into(), json!(event));
        map.insert("at".into(), json!(now_rfc3339()));
    }
    let Ok(mut line) = serde_json::to_string(&fields) else { return };
    line.push('\n');
    // A few hundred bytes, appended synchronously: simpler than async file
    // I/O and short enough not to matter on the runtime.
    use std::io::Write;
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let file = std::fs::OpenOptions::new().create(true).append(true).open(dir.join(LOG_FILE));
    if let Ok(mut file) = file {
        let _ = file.write_all(line.as_bytes());
    }
}
