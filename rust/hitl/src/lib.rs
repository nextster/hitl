//! hitl as a library: what hitl's own program builds on (`rust/hitl-host`), and the binary `hitl` for his team.
//!
//! Its words of 2026-10-28, from the chat that moves hitl to Rust: «rust/hitl — библиотека и бинарник hitl для
//! команды», so that what his team installs carries nothing of the rest of hitl. `dispatch` is hitl's commands;
//! the modules are what hitl calls the window and its answers by.

pub mod chatbutton;
pub mod claude;
pub mod codex;
pub mod common;
pub mod names;
pub mod opencode;
pub mod queue;
pub mod setup;
pub mod submit;
pub mod tell;
pub mod verdict;
pub mod window;

use std::io::Read;

const USAGE: &str = "hitl: a chat's plans, questions, results and requests, answered by a person in one window on the Mac.

Usage:
  hitl setup --name NAME [--she]
                         Once, after `brew install`: the name the verdicts call you by, and the skill
                         `hitl` put where Claude Code and Codex find it. Run it again after an upgrade.
  hitl check ...         A check in the window: a plan before big work, a question, a result,
                         something only the person can do, an explanation. `hitl check --help`.
  hitl queue             What waits in the window, and the answers the calling chat has not read.
  hitl wait [ID|TAG]     Wait for a check and print its verdict; the caller's own newest when none is
                         named. A chat of the Codex app does not wait: the verdict comes as a message.
  hitl drop [ID|TAG]     Take the caller's own check back. --note TEXT says why.
  hitl --version         Which build this is.

The window runs while checks wait and exits. Everything hitl keeps is in ~/.hitl.
";

pub fn version() -> String {
    let root = common::root();
    std::fs::read_to_string(root.join("EDITION")).map(|text| text.trim().to_string()).ok().filter(|text| !text.is_empty())
        .unwrap_or_else(|| format!("hitl {}", env!("CARGO_PKG_VERSION")))
}

fn refused(arguments: &[String]) -> i32 {
    common::say(&format!("{}: нет команды «{}». Что есть: {} --help", common::tool(), arguments.join(" "), common::tool()));
    common::EXIT_REFUSED
}

pub fn dispatch(arguments: Vec<String>) -> i32 {
    let Some(command) = arguments.first().cloned() else {
        print!("{USAGE}");
        return 0;
    };
    let rest: Vec<String> = arguments[1..].to_vec();
    match command.as_str() {
        "-h" | "--help" | "help" => {
            print!("{USAGE}");
            0
        }
        "--version" | "version" => {
            println!("{}", version());
            0
        }
        "setup" => setup::main(&rest),
        "check" if rest == ["--presentation"] => names::main(),
        "check" => submit::main(&rest),
        "queue" if rest.is_empty() => {
            queue::prepare();
            queue::show()
        }
        "wait" if rest.len() <= 1 => {
            queue::prepare();
            queue::wait(rest.first().map(String::as_str))
        }
        "drop" => {
            queue::prepare();
            queue::drop_command(&rest)
        }
        "last-check" => queue::last_check_command(&rest),
        "journal" if rest == ["_stream"] => {
            // The window's journal: hitl keeps one, hitl does not. Its pipe is read to the end and let go.
            let mut sink = Vec::new();
            let _ = std::io::stdin().read_to_end(&mut sink);
            0
        }
        "_tell" => tell::main(&rest),
        "_remind" => tell::remind(),
        // What hitl's Python asks of the window while it is not Rust itself (`hitlcore.py`); hitl's own Rust
        // calls the functions behind them: submit::parse_submission, window::ensure_panel, window::built_panel.
        "_check" if rest.first().map(String::as_str) == Some("parse") => parse(&rest[1..]),
        "_panel" if rest.is_empty() => {
            queue::prepare();
            window::ensure_panel();
            0
        }
        "_panel" if rest.len() == 2 && rest[0] == "build" => build_panel(&rest[1]),
        _ => refused(&arguments),
    }
}

/// `_check parse ARGS…`: the options of a check read and held to its limits, as `check` would, and nothing queued:
/// the ticket as JSON on one line, or the refusal and 64.
fn parse(arguments: &[String]) -> i32 {
    match submit::parse_submission(arguments) {
        Ok(ticket) => {
            println!("{ticket}");
            0
        }
        Err(message) => submit::refuse(&message),
    }
}

/// `_panel build FOLDER`: the window's program for the sources in that folder, built unless there is one, and the
/// others kept (`hitl update` builds main's before it swaps the folder in); its path, or what the compiler said
/// and 1.
fn build_panel(folder: &str) -> i32 {
    let mut sources: Vec<std::path::PathBuf> = std::fs::read_dir(folder).map(|entries| entries.flatten().map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "swift")).collect()).unwrap_or_default();
    sources.sort();
    match window::built_panel(&sources, false) {
        Ok(program) => {
            println!("{}", program.display());
            0
        }
        Err(said) => {
            common::say(&said);
            1
        }
    }
}
