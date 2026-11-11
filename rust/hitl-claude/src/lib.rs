//! Delivers HITL decisions to the Claude Code approval classifier.

pub mod note;

/// Answers `command` with its arguments, or None when this part does not have it.
pub fn dispatch(command: &str, rest: &[String]) -> Option<i32> {
    match command {
        "note" => note::dispatch(rest),
        _ => None,
    }
}
