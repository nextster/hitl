//! The queues and turns: hitl iphone, gate, and the phone's queue and the lines of turns behind them
//! (iphone.py, gate.py, queues.py).
//!
//! A part of hitl's move to Rust (`docs/rust-move.md`). `hitl` asks `dispatch` first: a command this part
//! has moved is answered here; a command it has not moved gets `None` and goes on to the next part, and at last to
//! Python. `queue`, `wait`, `drop`, `last-check` are the window's part (checks.py) and are not here.

pub mod base;
pub mod bridge;
pub mod commands;
pub mod device;
pub mod dispatcher;
pub mod gate;
pub mod iphone;
pub mod items;
pub mod lines;
pub mod private_launch_env;
pub mod state;
pub mod verdict;

#[cfg(test)]
#[path = "tests/mod.rs"]
mod queue_tests;

/// Answers `command` with its arguments, or None when this part does not have it (yet).
pub fn dispatch(command: &str, rest: &[String]) -> Option<i32> {
    // ios-run.sh looks for its gate under the name `ios-run-gate`: then every word is the gate's own.
    let called_by = hitl_host_core::var("CONVEYOR_ARGV0").unwrap_or_default();
    if std::path::Path::new(&called_by).file_name().is_some_and(|name| name == "ios-run-gate") {
        let all: Vec<String> = std::iter::once(command.to_string()).filter(|word| !word.is_empty()).chain(rest.iter().cloned()).collect();
        return Some(gate::main(&all));
    }
    match command {
        "iphone" => Some(iphone::main(rest)),
        "gate" => Some(gate::main(rest)),
        "_queues" => Some(commands::run(rest)),
        _ => None,
    }
}
