//! The action journal for the code of hitl itself, as Python's `journal.py` was: `emit` writes one event, `span`
//! writes the begin and the end of an operation with its duration and outcome, `operation` wraps a function in one.
//!
//! In this process, through the writer of the library `hitl-journal` (its own queue and worker thread, never
//! waited for by the caller), into `CONVEYOR_HOME/journal/events.sqlite3`. On unless `CONVEYOR_JOURNAL=0`. Best
//! effort: a failure of any step never stops the work. An event is the general one (`rust/journal/src/general.rs`):
//! only codes, hashed references and bounded numbers, never arguments, environment values, input or output.
//!
//! What an operation inherits, as in Python: the trace of the one that runs it, its own action, the action it runs
//! under as parent; at the root the trace comes from `CONVEYOR_TRACE_ID` and the parent from `CONVEYOR_PARENT_ACTION`.

use hitl_journal::Writer;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::sync::Mutex;
use std::time::Instant;

/// None: no event yet, or closed. A writer that could not be made is not made again in this process.
static WRITER: Mutex<(Option<Writer>, bool)> = Mutex::new((None, false));

thread_local! {
    /// What the operations running on this thread, the innermost last, stand for: trace, action, actor.
    static RUNNING: RefCell<Vec<Map<String, Value>>> = const { RefCell::new(Vec::new()) };
}

pub fn enabled() -> bool {
    crate::var("CONVEYOR_JOURNAL").as_deref() != Some("0")
}

fn random_hex() -> String {
    let mut bytes = [0u8; 16];
    // SAFETY: a buffer we own, of the length given.
    if unsafe { libc::getentropy(bytes.as_mut_ptr().cast(), bytes.len()) } != 0 {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        bytes = now.to_le_bytes();
    }
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `reference` of the journal: a code as it is, else a hash.
pub fn reference(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let plain = text.len() <= 128 && text.bytes().all(|c| c.is_ascii_alphanumeric() || b"_.:/@-".contains(&c));
    if plain && !text.starts_with(['/', '~']) {
        text.to_string()
    } else {
        format!("sha256:{}", &format!("{:x}", Sha256::digest(text.as_bytes()))[..24])
    }
}

/// The event with what its operation stands for put in where it names nothing of its own.
fn inherited(event: &Value) -> Value {
    let mut full = event.as_object().cloned().unwrap_or_default();
    RUNNING.with(|running| {
        if let Some(here) = running.borrow().last() {
            for (key, value) in here {
                let absent = match full.get(key) {
                    None | Some(Value::Null) => true,
                    Some(Value::String(text)) => text.is_empty() && key != "actor",
                    Some(_) => false,
                };
                if absent || (key == "actor" && !matches!(full.get(key).and_then(Value::as_str), Some("user" | "automation" | "model"))) {
                    full.insert(key.clone(), value.clone());
                }
            }
        }
    });
    Value::Object(full)
}

/// Writes one event; true when it went to the writer's queue (the queue may still drop it under pressure, which the
/// journal records as a loss).
pub fn emit(event: &Value) -> bool {
    if !enabled() {
        return false;
    }
    let full = inherited(event);
    let Ok(mut guard) = WRITER.lock() else { return false };
    if guard.0.is_none() && !guard.1 {
        match Writer::new(&crate::home(), false) {
            Ok(writer) => guard.0 = Some(writer),
            Err(_) => guard.1 = true,
        }
    }
    let Some(writer) = guard.0.as_mut() else { return false };
    writer.submit_event(&full);
    true
}

/// Lets the writer finish what it holds (two drains of 200 ms at most). Call before exit or exec.
pub fn shutdown() {
    let Ok(mut guard) = WRITER.lock() else { return };
    if let Some(writer) = guard.0.take() {
        writer.close();
    }
}

/// The action of the operation running on this thread, or "".
pub fn current_action() -> String {
    RUNNING.with(|running| running.borrow().last().and_then(|here| here.get("action")).and_then(Value::as_str).unwrap_or("").to_string())
}

/// The environment a child of this operation runs in: its trace and the action it runs under, so that the
/// operations of another process of hitl hang from this one.
pub fn child_environment() -> Vec<(String, String)> {
    let mut found = Vec::new();
    RUNNING.with(|running| {
        if let Some(here) = running.borrow().last() {
            let text = |key: &str| here.get(key).and_then(Value::as_str).unwrap_or("").to_string();
            if !text("trace").is_empty() {
                found.push(("CONVEYOR_TRACE_ID".into(), text("trace")));
                found.push(("CONVEYOR_PARENT_ACTION".into(), text("action")));
                if !text("actor").is_empty() {
                    found.push(("CONVEYOR_ACTOR".into(), text("actor")));
                }
            }
        }
    });
    found
}

/// How an operation ended, when it did not end well.
pub struct Failure {
    /// "error" or "uncertain" (the outcome of a native operation nobody can say the end of).
    pub outcome: &'static str,
    pub error: &'static str,
    /// The text of the error, of which only a fingerprint is kept.
    pub text: String,
}

impl Failure {
    pub fn refused(text: &str, uncertain: bool) -> Failure {
        Failure { outcome: if uncertain { "uncertain" } else { "error" }, error: "NativeRefused", text: text.to_string() }
    }
}

fn fingerprint(text: &str) -> String {
    let cut: String = text.chars().take(4096).collect();
    format!("{:x}", Sha256::digest(cut.as_bytes()))[..16].to_string()
}

/// An operation that has begun. Ends with `end`, or with an error if it is dropped without one (a panic).
pub struct Span {
    kind: String,
    fields: Map<String, Value>,
    started: Instant,
    open: bool,
}

/// Begins an operation: puts its trace, action and parent in force on this thread and writes the begin event.
pub fn span(kind: &str, fields: Value) -> Span {
    let fields = fields.as_object().cloned().unwrap_or_default();
    let inherited_here = RUNNING.with(|running| running.borrow().last().cloned()).unwrap_or_default();
    let text = |map: &Map<String, Value>, key: &str| map.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    let trace = Some(text(&inherited_here, "trace")).filter(|trace| !trace.is_empty())
        .or_else(|| Some(reference(&crate::var("CONVEYOR_TRACE_ID").unwrap_or_default())).filter(|trace| !trace.is_empty()))
        .unwrap_or_else(random_hex);
    let parent = Some(text(&inherited_here, "action")).filter(|action| !action.is_empty())
        .unwrap_or_else(|| reference(&crate::var("CONVEYOR_PARENT_ACTION").unwrap_or_default()));
    let actor = Some(fields.get("actor").and_then(Value::as_str).unwrap_or("").to_string()).filter(|actor| !actor.is_empty())
        .or_else(|| Some(text(&inherited_here, "actor")).filter(|actor| !actor.is_empty()))
        .unwrap_or_default();
    let mut now_in_force = inherited_here.clone();
    now_in_force.insert("trace".into(), json!(trace));
    now_in_force.insert("action".into(), json!(random_hex()));
    now_in_force.insert("parent".into(), json!(parent));
    if actor.is_empty() {
        now_in_force.remove("actor");
    } else {
        now_in_force.insert("actor".into(), json!(actor));
    }
    for key in ["request", "attempt", "subject"] {
        if let Some(Value::String(value)) = fields.get(key) {
            now_in_force.insert(key.into(), json!(reference(value)));
        }
    }
    RUNNING.with(|running| running.borrow_mut().push(now_in_force));
    let mut begin = fields.clone();
    begin.insert("kind".into(), json!(kind));
    begin.insert("phase".into(), json!("begin"));
    emit(&Value::Object(begin));
    Span { kind: kind.into(), fields, started: Instant::now(), open: true }
}

impl Span {
    /// Writes the end event: `result` may carry `outcome` ("ok" when absent), `error` and `context` (joined to the
    /// begin's).
    pub fn end(mut self, result: Value) {
        self.finish(result);
    }

    fn finish(&mut self, result: Value) {
        if !self.open {
            return;
        }
        self.open = false;
        let mut end = self.fields.clone();
        let mut context = end.get("context").and_then(Value::as_object).cloned().unwrap_or_default();
        if let Value::Object(result) = result {
            for (key, value) in result {
                if key == "context" {
                    if let Value::Object(more) = value {
                        context.extend(more);
                    }
                } else {
                    end.insert(key, value);
                }
            }
        }
        end.entry("outcome").or_insert_with(|| json!("ok"));
        end.insert("context".into(), Value::Object(context));
        end.insert("kind".into(), json!(self.kind));
        end.insert("phase".into(), json!("end"));
        end.insert("duration_ms".into(), json!(self.started.elapsed().as_secs_f64() * 1000.0));
        emit(&Value::Object(end));
        RUNNING.with(|running| {
            running.borrow_mut().pop();
        });
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        self.finish(json!({"outcome": "error", "error": "Panic"}));
    }
}

/// Runs `run` as the operation `kind` of `source` on `subject`: the begin, the end, the duration. `judge` says of the
/// result whether it is a failure (an operation that reports an error as a value).
pub fn operation<T>(kind: &str, source: &str, subject: &str, run: impl FnOnce() -> T, judge: impl FnOnce(&T) -> Option<Failure>) -> T {
    if !enabled() {
        return run();
    }
    let span = span(kind, json!({"source": source, "subject": subject}));
    let result = run();
    match judge(&result) {
        Some(failure) => span.end(json!({"outcome": failure.outcome, "error": failure.error,
                                         "context": {"error_fingerprint": fingerprint(&failure.text)}})),
        None => span.end(json!({})),
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn off() {
        // SAFETY: every test of this module wants the journal off, so none of them writes into a real home.
        unsafe { std::env::set_var("CONVEYOR_JOURNAL", "0") };
    }

    #[test]
    fn a_disabled_journal_runs_the_operation_alone() {
        off();
        assert!(!emit(&json!({"kind": "x", "context": {"a": "b"}})));
        assert_eq!(operation("native.test", "native.Test", "s", || 7, |_| None), 7);
    }

    #[test]
    fn inner_operations_hang_from_the_outer_one() {
        off();
        let outer = span("outer", json!({}));
        let (trace, action) = RUNNING.with(|r| {
            let r = r.borrow();
            let last = r.last().unwrap();
            (last["trace"].as_str().unwrap().to_string(), last["action"].as_str().unwrap().to_string())
        });
        let inner = span("inner", json!({}));
        RUNNING.with(|r| {
            let r = r.borrow();
            let last = r.last().unwrap();
            assert_eq!(last["trace"].as_str().unwrap(), trace);
            assert_eq!(last["parent"].as_str().unwrap(), action);
        });
        inner.end(json!({}));
        outer.end(json!({}));
        assert!(RUNNING.with(|r| r.borrow().is_empty()));
    }
}
