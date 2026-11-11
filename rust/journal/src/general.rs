//! The general event of the journal, as Python's `journal.make_event` made it: a small explicit schema, never an
//! unbounded payload. The row has the same columns as the events of the window and of native processes (`codec`).
//!
//! The producer gives a JSON object; keys it may carry: kind, phase, outcome, error, duration_ms, actor, source,
//! context (an object, the first 12 keys), occurred_at, trace, action, parent, request, attempt, subject,
//! input_tokens, output_tokens, cost_nano_usd, usage_known, identity. The producer's context (trace, action, parent,
//! actor) is put into the object by the caller; what is absent here is empty.

use rusqlite::types::Value as Sql;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Only technical codes and ids, never exception text, argv, messages or response bodies.
pub fn code(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit && value.bytes().all(|c| c.is_ascii_alphanumeric() || b"_.:/@-".contains(&c)) {
        value.to_string()
    } else {
        String::new()
    }
}

fn code_of(value: Option<&Value>, limit: usize) -> String {
    value.and_then(Value::as_str).map(|text| code(text, limit)).unwrap_or_default()
}

/// Paths and projects are joined locally and enter the journal as a hash.
pub fn reference(value: Option<&Value>) -> String {
    let text = match value {
        None | Some(Value::Null) => return String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    };
    if text.is_empty() {
        return String::new();
    }
    let plain = code(&text, 128);
    if !text.starts_with(['/', '~']) && !plain.is_empty() {
        plain
    } else {
        format!("sha256:{}", &format!("{:x}", Sha256::digest(text.as_bytes()))[..24])
    }
}

/// A finite number in range, else None (Python's `number`): a flag is not one.
fn number(value: Option<&Value>, signed: bool) -> Option<Sql> {
    let Value::Number(number) = value? else { return None };
    if let Some(whole) = number.as_i64() {
        return (signed || whole >= 0).then_some(Sql::Integer(whole));
    }
    if let Some(big) = number.as_u64() {
        return (big <= i64::MAX as u64).then_some(Sql::Integer(big as i64));
    }
    let real = number.as_f64()?;
    let low = if signed { -9223372036854775808.0 } else { 0.0 };
    (real.is_finite() && real >= low && real < 9223372036854775808.0).then_some(Sql::Real(real))
}

fn integer(value: Option<&Value>) -> Option<Sql> {
    match number(value, false)? {
        Sql::Real(real) => Some(Sql::Integer(real as i64)),
        other => Some(other),
    }
}

/// Who acts when the event does not say: `CONVEYOR_ACTOR`, else a model if a chat of an agent runs this, else a person.
fn actor(given: &str) -> String {
    if matches!(given, "user" | "automation" | "model") {
        return given.into();
    }
    let named = std::env::var("CONVEYOR_ACTOR").unwrap_or_default();
    if matches!(named.as_str(), "user" | "automation" | "model") {
        return named;
    }
    let agent = ["CODEX_THREAD_ID", "CLAUDECODE", "OPENCODE"].iter().any(|key| std::env::var(key).is_ok_and(|v| !v.is_empty()));
    if agent { "model" } else { "user" }.into()
}

fn context(value: Option<&Value>) -> String {
    let mut kept = serde_json::Map::new();
    if let Some(Value::Object(fields)) = value {
        for (key, item) in fields.iter().take(12) {
            let name = code(key, 80);
            if name.is_empty() {
                continue;
            }
            let item = match item {
                Value::Number(_) => match number(Some(item), true) {
                    Some(Sql::Integer(whole)) => Value::from(whole),
                    Some(Sql::Real(real)) => serde_json::json!(real),
                    _ => Value::Null,
                },
                Value::String(text) => Value::String(code(text, 80)),
                _ => Value::String(String::new()),
            };
            kept.insert(name, item);
        }
    }
    serde_json::to_string(&Value::Object(kept)).unwrap_or_else(|_| "{}".into())
}

/// The row of one event, in the order of the table's columns (process and seq are set by the writer).
pub fn row(event: &Value, now: f64) -> Vec<Sql> {
    let get = |key: &str| event.get(key);
    let at = match number(get("occurred_at"), false) {
        Some(Sql::Real(real)) if real != 0.0 => real,
        Some(Sql::Integer(whole)) if whole != 0 => whole as f64,
        _ => now,
    };
    let source = code_of(get("source"), 80);
    let source = if source.is_empty() {
        let named = code(&std::env::var("CONVEYOR_SOURCE").unwrap_or_default(), 80);
        if named.is_empty() { "cli".into() } else { named }
    } else {
        source
    };
    let phase = code_of(get("phase"), 80);
    let mut row = vec![
        Sql::Real(at),
        Sql::Null,
        Sql::Null,
        match get("identity") {
            Some(value) if !matches!(value, Value::Null) && value.as_str() != Some("") => Sql::Text(reference(Some(value))),
            _ => Sql::Null,
        },
        Sql::Text(actor(get("actor").and_then(Value::as_str).unwrap_or(""))),
        Sql::Text(source),
        Sql::Text(code_of(get("kind"), 80)),
        Sql::Text(if phase.is_empty() { "event".into() } else { phase }),
    ];
    for key in ["trace", "action", "parent", "request", "attempt", "subject"] {
        row.push(Sql::Text(reference(get(key))));
    }
    row.extend([
        Sql::Text(code_of(get("outcome"), 80)),
        Sql::Text(code_of(get("error"), 80)),
        number(get("duration_ms"), false).unwrap_or(Sql::Null),
        integer(get("input_tokens")).unwrap_or(Sql::Null),
        integer(get("output_tokens")).unwrap_or(Sql::Null),
        integer(get("cost_nano_usd")).unwrap_or(Sql::Null),
        match get("usage_known") {
            Some(Value::Bool(flag)) => Sql::Integer(*flag as i64),
            _ => Sql::Null,
        },
        Sql::Text(context(get("context"))),
    ]);
    row
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn text(row: &[Sql], at: usize) -> String {
        match &row[at] {
            Sql::Text(text) => text.clone(),
            other => format!("{other:?}"),
        }
    }

    #[test]
    fn a_general_event_has_the_columns_python_wrote() {
        let found = row(
            &json!({"kind": "hitl.queued", "source": "hitl", "request": "0101-ticket", "outcome": "queued", "actor": "automation",
                    "context": {"kind": "plan", "items": 3, "bad key!": 1, "text": "not a code!"}}),
            100.0,
        );
        assert_eq!(found.len(), 22);
        assert_eq!(text(&found, 4), "automation");
        assert_eq!(text(&found, 5), "hitl");
        assert_eq!(text(&found, 6), "hitl.queued");
        assert_eq!(text(&found, 7), "event");
        assert_eq!(text(&found, 11), "0101-ticket");
        assert_eq!(text(&found, 14), "queued");
        assert_eq!(text(&found, 21), r#"{"kind":"plan","items":3,"text":""}"#);
        assert!(matches!(found[3], Sql::Null) && matches!(found[16], Sql::Null) && matches!(found[20], Sql::Null));
    }

    #[test]
    fn paths_are_hashed_and_codes_kept() {
        assert_eq!(reference(Some(&json!("abc-1"))), "abc-1");
        assert!(reference(Some(&json!("/Users/a/b"))).starts_with("sha256:"));
        assert!(reference(Some(&json!("has space"))).starts_with("sha256:"));
        assert_eq!(reference(Some(&json!(null))), "");
        assert_eq!(reference(Some(&json!(7))), "7");
        assert_eq!(reference(Some(&json!(""))), "");
    }

    #[test]
    fn numbers_and_flags_follow_the_old_limits() {
        let found = row(&json!({"kind": "model.jev", "duration_ms": 12.5, "input_tokens": 10, "output_tokens": -1,
                                 "usage_known": true, "cost_nano_usd": 7.9, "identity": "/a"}), 1.0);
        assert!(matches!(found[16], Sql::Real(real) if real == 12.5));
        assert!(matches!(found[17], Sql::Integer(10)));
        assert!(matches!(found[18], Sql::Null));
        assert!(matches!(found[19], Sql::Integer(7)));
        assert!(matches!(found[20], Sql::Integer(1)));
        assert!(text(&found, 3).starts_with("sha256:"));
    }

    #[test]
    fn the_context_keeps_twelve_keys_and_a_flag_is_no_number() {
        let mut fields = serde_json::Map::new();
        for number in 0..15 {
            fields.insert(format!("k{number}"), json!(number));
        }
        fields.insert("flag".into(), json!(true));
        let found = row(&json!({"kind": "x", "context": fields}), 1.0);
        let kept: Value = serde_json::from_str(&text(&found, 21)).unwrap();
        assert_eq!(kept.as_object().unwrap().len(), 12);
        let flagged = row(&json!({"kind": "x", "context": {"flag": true}}), 1.0);
        assert_eq!(text(&flagged, 21), r#"{"flag":""}"#);
    }
}
