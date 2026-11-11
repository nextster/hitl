//! hitl stats: moved from `stats.py` (see `docs/rust-move.md`).
//!
//! The report itself is the library `hitl_stats` (`rust/stats`), which Python used to run as a program of its
//! own and relay through a pipe. Here it runs in this process and writes to stdout as it is. One thing is kept from
//! the old entry: `CONVEYOR_STATS_EXECUTABLE`, the explicit hand-over to another program (an absolute path to an
//! executable file), which the tests use and which refuses before any history is read.

use hitl_host_core::{EXIT_REFUSED, home, process, say, var};
use hitl_stats::{Arguments, Error};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|since| since.as_secs_f64()).unwrap_or(0.0)
}

/// How a lone surrogate is written to stdout: `CONVEYOR_STATS_STDOUT_ERRORS`, else the handler `PYTHONIOENCODING`
/// names after its colon (as Python's stdout would have had it), else surrogateescape, which never fails on one (Python's own is that in UTF-8 mode).
fn stdout_errors() -> String {
    if let Some(errors) = var("CONVEYOR_STATS_STDOUT_ERRORS").filter(|value| !value.is_empty()) {
        return errors;
    }
    var("PYTHONIOENCODING")
        .and_then(|value| value.split_once(':').map(|(_, errors)| errors.to_string()))
        .filter(|errors| !errors.is_empty())
        .unwrap_or_else(|| "surrogateescape".into())
}

fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path).map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

/// Whether the kernel can run the file: a `#!` line or a binary image. `execvp`, which `Command` uses, would give any
/// other file to the shell; the old entry's `os.execve` refused it, and so does this.
fn runnable_image(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 4];
    let Ok(mut file) = std::fs::File::open(path) else { return false };
    if file.read_exact(&mut head).is_err() {
        return false;
    }
    head.starts_with(b"#!")
        || matches!(
            head,
            [0xcf, 0xfa, 0xed, 0xfe] | [0xce, 0xfa, 0xed, 0xfe] | [0xca, 0xfe, 0xba, 0xbe] | [0xfe, 0xed, 0xfa, 0xce] | [0xfe, 0xed, 0xfa, 0xcf] | [0x7f, b'E', b'L', b'F']
        )
}

/// The explicit hand-over of `stats` and of `journal _stream`: another program (an absolute path to an executable
/// file) answers in this one's place, with the state folder as it is resolved here, and `extra` set on top of the
/// environment. Refuses, with exit 64 and the words `not_given` or `not_started`, before anything is read; there
/// is no second try and no fall back to this program.
pub(crate) fn hand_over_to(
    executable: &str,
    rest: &[String],
    purpose: Option<&str>,
    extra: &[(&str, String)],
    removed: &[&str],
    not_given: &str,
    not_started: &str,
) -> i32 {
    let path = Path::new(executable);
    if !path.is_absolute() || !is_executable(path) {
        say(not_given);
        return EXIT_REFUSED;
    }
    if !runnable_image(path) {
        say(not_started);
        return EXIT_REFUSED;
    }
    // `purpose` None: not written to the journal (the journal's own hand-over is not an event of it).
    let mut options = process::Options::new(purpose.unwrap_or("handover")).env("CONVEYOR_HOME", &home().display().to_string()).inherit();
    if purpose.is_none() {
        options = options.unjournaled();
    }
    for (name, value) in extra {
        options = options.env(name, value);
    }
    for name in removed {
        options = options.env_remove(name);
    }
    let _ = process::exec(executable.to_string(), rest, &options);
    say(not_started);
    EXIT_REFUSED
}

fn hand_over(executable: &str, rest: &[String]) -> i32 {
    hand_over_to(
        executable,
        rest,
        Some("stats.report"),
        &[("CONVEYOR_STATS_STDOUT_ERRORS", stdout_errors())],
        &["CONVEYOR_STATS_TRANSPORT"],
        "stats: нужен абсолютный путь к исполняемому Rust-отчёту; история не прочитана",
        "stats: Rust-отчёт не запустился; история не прочитана, повтора нет",
    )
}

fn report(rest: &[String]) -> Result<(), Error> {
    let mut out = std::io::stdout().lock();
    match hitl_stats::arguments(rest)? {
        Arguments::Help => {
            out.write_all(hitl_stats::USAGE.as_bytes()).map_err(|_| Error::Output)?;
        }
        Arguments::Run(options) => {
            let root = home();
            let errors = stdout_errors();
            if options.json {
                for row in hitl_stats::json_rows(&root, &options.days, now())? {
                    if hitl_stats::selected(&row, options.comments) {
                        let line = hitl_stats::json_line(&row, &errors)?;
                        out.write_all(&line).map_err(|_| Error::Output)?;
                    }
                }
            } else {
                let text = hitl_stats::report(&root, &options.days, options.comments, now)?;
                out.write_all(&hitl_stats::output_line(&text, &errors)?).map_err(|_| Error::Output)?;
            }
        }
    }
    out.flush().map_err(|_| Error::Output)
}

/// `hitl stats [--days N] [--comments] [--json]`: the code it ends with, as the report's own program did.
pub fn main(rest: &[String]) -> i32 {
    if let Some(executable) = var("CONVEYOR_STATS_EXECUTABLE").filter(|value| !value.is_empty()) {
        return hand_over(&executable, rest);
    }
    match report(rest) {
        Ok(()) => 0,
        Err(Error::Output) => {
            say("stats: stdout write failed");
            120
        }
        Err(Error::Usage) => {
            say(hitl_stats::REFUSAL);
            64
        }
        Err(Error::Data(message)) => {
            say(&format!("stats: {message}"));
            1
        }
    }
}

/// Answers `command` with its arguments, or None when this module does not have it (yet).
pub fn dispatch(command: &str, rest: &[String]) -> Option<i32> {
    (command == "stats").then(|| main(rest))
}
