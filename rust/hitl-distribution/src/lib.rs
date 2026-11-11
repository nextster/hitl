//! hitl, the check window for his team, as hitl sees it: `hitl hitl build|release` (what was
//! `hitlrelease.py`) and the events of hitl kept by hitl's action journal (what was `hitlhooks.py journal`).
//!
//! A part of hitl's move to Rust (`docs/rust-move.md`): `hitl` asks `dispatch`. `_hitl panel` stays
//! hitl-phone's.

pub mod release;

pub use release::{HIS_PARTS, MACOS, README, RELEASES, TAP, build, formula, hitl_program};

use serde_json::{Map, Value};
use std::io::Read;

/// What an event of hitl may carry into the journal, besides its kind and moment: the names `journal.emit` took
/// them by in `hitlhooks.py`.
const FIELDS: [&str; 11] = ["phase", "outcome", "error", "actor", "source", "context", "request", "subject", "trace", "action", "parent"];

/// The event hitl's journal is given for one event of hitl, as `hitlhooks.py` handed it to `journal.emit`: its
/// kind, its moment (`at`) and the fields of `FIELDS` it has. None for what is not an event (no kind as text).
pub fn journal_event(event: &Value) -> Option<Value> {
    let kind = event.get("kind")?.as_str()?;
    let mut made = Map::new();
    made.insert("kind".into(), Value::String(kind.into()));
    if let Some(at) = event.get("at") {
        made.insert("at".into(), at.clone());
    }
    for key in FIELDS {
        if let Some(value) = event.get(key) {
            made.insert(key.into(), value.clone());
        }
    }
    Some(Value::Object(made))
}

/// One event of hitl written to hitl's action journal in this process: what `rust/hitl-host` sets as hitl's
/// journal (`hitl::common::set_journal`), so that no program is started for it.
pub fn journal(event: &Value) {
    if let Some(made) = journal_event(event) {
        hitl_host_core::journal::emit(&made);
    }
}

/// `_hitl journal`: the events of a run of hitl handed over on stdin, as JSON, by a program of hitl that has no
/// journal of its own in its process (one built before hitl kept them in it).
fn hooked_journal() -> i32 {
    let mut text = String::new();
    let _ = std::io::stdin().read_to_string(&mut text);
    if let Ok(Value::Array(events)) = serde_json::from_str::<Value>(&text) {
        events.iter().for_each(journal);
    }
    println!("{{}}");
    0
}

/// Answers `command` with its arguments, or None when this part does not have it.
pub fn dispatch(command: &str, rest: &[String]) -> Option<i32> {
    match command {
        "package" => Some(release::main(rest)),
        "_hitl" if rest.len() == 1 && rest[0] == "journal" => Some(hooked_journal()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_command_of_another_part_is_not_taken() {
        assert_eq!(dispatch("no-such-command", &[]), None);
        assert_eq!(dispatch("_hitl", &["panel".to_string()]), None, "the phone's process is hitl-phone's");
    }

    #[test]
    fn an_event_keeps_what_hitlhooks_handed_to_the_journal() {
        let event = json!({"kind": "hitl.queued", "at": 1700000000.5, "request": "1030-1", "source": "hitl", "outcome": "ok",
                           "context": {"items": 2}, "subject": "s", "error": "e", "phase": "begin", "words": "not kept", "pid": 3});
        assert_eq!(
            journal_event(&event),
            Some(json!({"kind": "hitl.queued", "at": 1700000000.5, "phase": "begin", "outcome": "ok", "error": "e", "source": "hitl",
                        "context": {"items": 2}, "request": "1030-1", "subject": "s"}))
        );
        assert_eq!(journal_event(&json!({"kind": 3})), None);
        assert_eq!(journal_event(&json!(["hitl.queued"])), None);
    }
}
