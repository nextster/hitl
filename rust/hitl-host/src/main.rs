//! `hitl`: hitl's own program, what the file `hitl` at the root builds and runs.
//!
//! His «да» of 2026-10-28: all of hitl moved to Rust, and his «полностью удалить питон»: there is no Python
//! behind it. Two programs: `hitl` (rust/hitl, for his colleagues) and this one on top of it, which adds what only
//! the reviewer has: the phone, the queues and merging. A part is a crate of its own
//! (`rust/conveyor-*`) with `dispatch(command, rest) -> Option<i32>`; this program asks them one after another and
//! refuses a command none of them has. `docs/rust-move.md` says how it is laid out.

mod check_chat;
mod checks;

use hitl_host_core::var;
use serde_json::json;
use std::io::Read;

type Part = fn(&str, &[String]) -> Option<i32>;

/// The parts that have commands, in the order they are asked. A command is in one part only.
const PARTS: [Part; 5] = [
    hitl_phone::dispatch,
    hitl_queues::dispatch,
    hitl_tools::dispatch,
    hitl_claude::dispatch,
    hitl_distribution::dispatch,
];

/// What `hitl`, `hitl help` and `hitl --help` print: every command, a line or a few each.
const USAGE: &str = include_str!("usage.txt");

/// A command none of the parts has.
fn refused(arguments: &[String]) -> i32 {
    hitl_host_core::say(&format!("hitl: нет команды «{}». Что есть: hitl --help", arguments.join(" ")));
    hitl_host_core::EXIT_REFUSED
}

/// 32 hex digits of the system's randomness, as Python's uuid4().hex was for a trace and an action.
fn random_id() -> String {
    let mut bytes = [0u8; 16];
    if std::fs::File::open("/dev/urandom").and_then(|mut file| file.read_exact(&mut bytes)).is_err() {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        bytes = (now ^ u128::from(std::process::id())).to_le_bytes();
    }
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Who called, as the journal names it: CONVEYOR_ACTOR, else a model when a chat's agent runs this, else the user.
fn actor() -> &'static str {
    match var("CONVEYOR_ACTOR").as_deref() {
        Some("user") => "user",
        Some("automation") => "automation",
        Some("model") => "model",
        _ if ["T3CODE_THREAD_ID", "CODEX_THREAD_ID", "CLAUDECODE", "OPENCODE"].iter().any(|key| var(key).is_some_and(|v| !v.is_empty())) => "model",
        _ => "user",
    }
}

/// Whether a call changes something, and so gets its span in the journal, as Python's `journal.cli_mutation` said:
/// reads, help and the journal's own commands leave no row.
fn mutation(arguments: &[String]) -> bool {
    if arguments.is_empty() || arguments.iter().any(|a| a == "--help" || a == "-h") {
        return false;
    }
    let (command, rest) = (arguments[0].as_str(), &arguments[1..]);
    let first = rest.first().map(String::as_str);
    match command {
        "drop" | "update" | "install" => true,
        "check" => rest != ["--presentation"],
        "iphone" => true,
        "phone" => matches!(first, Some("pair" | "relay")),
        _ => false,
    }
}

/// One command's span in the action journal, `cli.<command>` begin and end, as Python's `journal.span` wrote it: the
/// trace a parent handed down (or a new one), an action of its own, which the programs it starts inherit as their
/// parent action.
struct Span {
    kind: String,
    trace: String,
    action: String,
    parent: String,
    started: std::time::Instant,
}

impl Span {
    fn begin(command: &str) -> Span {
        let trace = var("CONVEYOR_TRACE_ID").filter(|v| !v.is_empty()).unwrap_or_else(random_id);
        let parent = var("CONVEYOR_PARENT_ACTION").unwrap_or_default();
        let span = Span { kind: format!("cli.{command}"), trace, action: random_id(), parent, started: std::time::Instant::now() };
        // The programs this one starts belong to this action (the process is still one thread here).
        unsafe {
            std::env::set_var("CONVEYOR_TRACE_ID", &span.trace);
            std::env::set_var("CONVEYOR_PARENT_ACTION", &span.action);
        }
        span.emit("begin", json!({}));
        span
    }

    fn end(&self, code: i32) {
        let outcome = match code { 75 => "queued", 0 => "ok", _ => "refused" };
        let ms = self.started.elapsed().as_secs_f64() * 1000.0;
        self.emit("end", json!({"outcome": outcome, "duration_ms": ms, "context": json!({"exit_code": code}).to_string()}));
    }

    fn emit(&self, phase: &str, extra: serde_json::Value) {
        let source = var("CONVEYOR_SOURCE").filter(|v| !v.is_empty()).unwrap_or_else(|| "cli".into());
        let mut event = json!({"kind": self.kind, "phase": phase, "actor": actor(), "source": source,
                               "trace": self.trace, "action": self.action, "parent": self.parent});
        if let (Some(map), Some(extra)) = (event.as_object_mut(), extra.as_object()) {
            map.extend(extra.clone());
        }
        hitl_host_core::journal::emit(&event);
    }
}

/// What only the phone's queue knows, handed to hitl's messenger so that it tells a chat his answer to a check on the
/// phone too (tell.py's `run_phone` and `message`): where a check stands in a phone's queue, and the phone's verdict.
fn phone() -> hitl::tell::Phone {
    hitl::tell::Phone {
        position: |udid, ticket_id| hitl_queues::commands::position_of(&hitl_queues::state::Queue::new(udid).load(), ticket_id),
        verdict_text: hitl_queues::verdict::verdict_text,
    }
}

/// hitl called by its name: every command is answered here, by hitl or by one of the parts.
fn hitl(arguments: Vec<String>) -> i32 {
    hitl::tell::set_phone(phone());
    let command = arguments.first().map(String::as_str).unwrap_or("");
    let rest = arguments.get(1..).unwrap_or(&[]);
    // ios-run.sh calls this program by the name `ios-run-gate`: then every word, `--help` too, is the gate's.
    let gate = var("CONVEYOR_CALLED_AS").or_else(|| var("CONVEYOR_ARGV0"))
        .is_some_and(|name| std::path::Path::new(&name).file_name().is_some_and(|name| name == "ios-run-gate"));
    if !gate && matches!(command, "" | "-h" | "--help" | "help") {
        print!("{USAGE}");
        return 0;
    }
    // The «Чат» button is a setting; the checks that carry its address find the file there, written on the first.
    if matches!(command, "check" | "iphone") {
        hitl_host_core::chatbutton::ensure_setting(&hitl::common::env());
    }
    // One check on either screen: the window's (hitl) or a phone's (hitl-queues), as Python's checks.py chose.
    match command {
        "queue" if rest.is_empty() => return checks::show(),
        "queue" if rest == ["--watch"] => return checks::watch(),
        "queue" if rest == ["--brief"] => return checks::brief(),
        "wait" if rest.len() <= 1 => return checks::wait(rest.first().map(String::as_str)),
        "drop" => return checks::drop(rest),
        "last-check" => return checks::last_check(rest),
        _ => {}
    }
    // The window's commands are hitl's, the one implementation of `hitl check` (his «да» of 2026-10-28).
    if matches!(command, "check" | "setup" | "--version" | "version") {
        return hitl::dispatch(arguments);
    }
    // The messenger of a check, the window's and the phone's: hitl's, with the phone handed in above.
    if command == "_tell" {
        return hitl::tell::main(rest);
    }
    // The hook of Claude Code at the start of a chat's turn: the answers the chat has not read, the phone's too.
    if command == "_remind" {
        return hitl::tell::remind();
    }
    if command == "chat" {
        return check_chat::dispatch(rest).unwrap_or_else(|| refused(&arguments));
    }
    for part in PARTS {
        if let Some(code) = part(command, rest) {
            return code;
        }
    }
    refused(&arguments)
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    // hitl's events go to the action journal in this process, as they happen: no program is started for them.
    hitl::common::set_journal(hitl_distribution::journal, hitl_host_core::journal::shutdown);
    let code = if hitl::common::is_host() {
        if mutation(&arguments) {
            let span = Span::begin(&arguments[0]);
            let code = hitl(arguments);
            span.end(code);
            code
        } else {
            hitl(arguments)
        }
    } else {
        hitl::dispatch(arguments)
    };
    hitl_host_core::journal::shutdown();
    hitl::common::exit(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_change_gets_a_span() {
        let words = |text: &str| text.split_whitespace().map(String::from).collect::<Vec<_>>();
        for yes in ["check --title t", "phone pair", "install", "iphone --check"] {
            assert!(mutation(&words(yes)), "{yes}");
        }
        for no in ["", "journal status", "check --presentation", "phone", "queue", "chat link", "chat reveal Codex x"] {
            assert!(!mutation(&words(no)), "{no}");
        }
    }

    #[test]
    fn no_part_answers_a_command_nobody_has() {
        assert!(PARTS.iter().all(|part| part("no-such-command", &[]).is_none()));
    }
}
