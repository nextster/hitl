//! hitl journal: moved from `journal.main` of `journal.py` (see `docs/rust-move.md`).
//!
//! The command: `status` (where the bounded action journal's SQLite file is and its retention policy) and the
//! internal `_stream`, the child the check window writes its events to. `_stream` is the library `hitl_journal`
//! (`rust/journal`) run in this process; `CONVEYOR_JOURNAL_EXECUTABLE`, the explicit opt-in to another program, still
//! hands the call over before a byte of input is read.

use hitl_host_core::{home, var};
use std::path::Path;

const DAYS: u32 = 14;
const MAX_BYTES: u64 = 128 * 1024 * 1024;

fn status() -> i32 {
    let path = home().join("journal").join("events.sqlite3");
    let text = |value: &str| serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into());
    println!(
        "{{\"database\": {}, \"exists\": {}, \"retentionDays\": {DAYS}, \"maxBytes\": {MAX_BYTES}, \"analysis\": {}, \"bestEffort\": true}}",
        text(&path.display().to_string()),
        Path::new(&path).exists(),
        text("sqlite3 -readonly DATABASE; table events, indexes trace/request"),
    );
    0
}

fn stream() -> i32 {
    if let Some(executable) = var("CONVEYOR_JOURNAL_EXECUTABLE").filter(|value| !value.is_empty()) {
        // Explicit opt-in, before reading a byte. An exec error never falls back and replays input another way.
        return crate::stats::hand_over_to(
            &executable,
            &[],
            None,
            &[],
            &[],
            "journal: нужен абсолютный путь к исполняемому Rust-журналу; ввод не прочитан",
            "journal: Rust-журнал не запустился; ввод не прочитан, повтора нет",
        );
    }
    // The state root is resolved here, once, as everywhere in hitl; the library reads it from the environment.
    // SAFETY: nothing else runs yet that reads the environment, and this is the only thread that writes it.
    unsafe { std::env::set_var("CONVEYOR_HOME", home()) };
    match hitl_journal::run() {
        Ok(()) => 0,
        Err(code) => code,
    }
}

/// `hitl journal [status]` and `hitl journal _stream`; any other call is refused with 64 and no words.
pub fn main(rest: &[String]) -> i32 {
    let words: Vec<&str> = rest.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["_stream"] => stream(),
        [] | ["status"] | ["--help"] | ["-h"] => status(),
        _ => 64,
    }
}

/// What an event of hitl may carry into the journal, by the names the journal takes them.
const HITL_FIELDS: [&str; 11] = ["phase", "outcome", "error", "actor", "source", "context", "request", "subject", "trace", "action", "parent"];

/// `hitl _hitl journal`: the events of one run of hitl on stdin as a JSON array, kept by the action journal
/// (`hitlhooks.py`). Not for a chat to call. Answers `{}`.
fn hitl_events() -> i32 {
    let mut text = String::new();
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut text);
    if let Ok(serde_json::Value::Array(events)) = serde_json::from_str::<serde_json::Value>(&text) {
        for event in events {
            let Some(kind) = event.get("kind").and_then(|kind| kind.as_str()) else { continue };
            let mut kept = serde_json::Map::new();
            kept.insert("kind".into(), kind.into());
            if let Some(at) = event.get("at") {
                kept.insert("occurred_at".into(), at.clone());
            }
            for key in HITL_FIELDS {
                if let Some(value) = event.get(key) {
                    kept.insert(key.into(), value.clone());
                }
            }
            hitl_host_core::journal::emit(&serde_json::Value::Object(kept));
        }
    }
    println!("{{}}");
    0
}

/// Answers `command` with its arguments, or None when this module does not have it (yet).
pub fn dispatch(command: &str, rest: &[String]) -> Option<i32> {
    match command {
        "journal" => Some(main(rest)),
        "_hitl" if rest.len() == 1 && rest[0] == "journal" => Some(hitl_events()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_journal_is_answered_and_other_arguments_are_refused() {
        assert_eq!(dispatch("stats", &[]), None);
        assert_eq!(main(&["unknown".into()]), 64);
        assert_eq!(main(&["status".into(), "more".into()]), 64);
    }
}
