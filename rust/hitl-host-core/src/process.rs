//! Programs hitl starts: with a timeout, with the purpose written to the journal, never started twice.
//!
//! The port of `src/hitl/processes.py`. Like it, a launch is attributed (what for, which executable, the pid
//! of the child and how it ended) and nothing else of it is kept; unlike it, there is no fallback to a library
//! call: a program is started once, and a failure is the caller's to see.

use crate::journal;
use serde_json::{Value, json};
use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// How one program is started. `Options::new("module.what_for")` names the purpose, which the journal keeps.
#[derive(Clone, Debug)]
pub struct Options {
    pub purpose: String,
    pub subject: String,
    pub request: String,
    pub input: Option<Vec<u8>>,
    pub timeout: Option<Duration>,
    pub cwd: Option<PathBuf>,
    /// Set (`Some`) or removed (`None`) on top of this process's environment.
    pub env: Vec<(String, Option<String>)>,
    /// Start with an empty environment before `env`.
    pub clear_env: bool,
    /// Take stdout and stderr; else they are inherited.
    pub capture: bool,
    /// Write the launch to the journal. Off for the hand-over to Python, which attributes what it starts itself.
    pub journal: bool,
}

impl Options {
    pub fn new(purpose: &str) -> Options {
        Options {
            purpose: purpose.chars().rev().take(80).collect::<Vec<_>>().into_iter().rev().collect(),
            subject: String::new(),
            request: String::new(),
            input: None,
            timeout: None,
            cwd: None,
            env: Vec::new(),
            clear_env: false,
            capture: true,
            journal: true,
        }
    }

    pub fn timeout(mut self, seconds: f64) -> Options {
        self.timeout = Some(Duration::from_secs_f64(seconds));
        self
    }

    pub fn input(mut self, bytes: &[u8]) -> Options {
        self.input = Some(bytes.to_vec());
        self
    }

    pub fn cwd(mut self, folder: &Path) -> Options {
        self.cwd = Some(folder.to_path_buf());
        self
    }

    pub fn env(mut self, name: &str, value: &str) -> Options {
        self.env.push((name.to_string(), Some(value.to_string())));
        self
    }

    pub fn env_remove(mut self, name: &str) -> Options {
        self.env.push((name.to_string(), None));
        self
    }

    pub fn inherit(mut self) -> Options {
        self.capture = false;
        self
    }

    pub fn unjournaled(mut self) -> Options {
        self.journal = false;
        self
    }
}

/// What a program did. `code` is None when a signal ended it or it was killed at its timeout.
#[derive(Clone, Debug, Default)]
pub struct Output {
    pub pid: u32,
    pub code: Option<i32>,
    pub signal: Option<i32>,
    pub timed_out: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Output {
    pub fn success(&self) -> bool {
        self.code == Some(0) && !self.timed_out
    }

    pub fn out(&self) -> String {
        String::from_utf8_lossy(&self.stdout).to_string()
    }

    pub fn err(&self) -> String {
        String::from_utf8_lossy(&self.stderr).to_string()
    }
}

fn command_of(program: &OsStr, arguments: &[OsString], options: &Options) -> Command {
    let mut command = Command::new(program);
    command.args(arguments);
    if let Some(folder) = &options.cwd {
        command.current_dir(folder);
    }
    if options.clear_env {
        command.env_clear();
    }
    for (name, value) in &options.env {
        match value {
            Some(value) => command.env(name, value),
            None => command.env_remove(name),
        };
    }
    command
}

/// The caller as the journal names it: the app and the id of the chat this process runs in, or "".
fn caller() -> String {
    for (app, key) in [("Codex", "CODEX_THREAD_ID"), ("Claude", "CLAUDE_CODE_HOST_SESSION_ID"),
                       ("Claude", "CLAUDE_CODE_SESSION_ID"), ("OpenCode", "OPENCODE_SESSION_ID")] {
        if let Some(id) = crate::var(key).filter(|id| !id.is_empty()) {
            return format!("{app}:{id}");
        }
    }
    String::new()
}

fn event(kind: &str, program: &OsStr, options: &Options, extra: Value) -> Value {
    let mut context = json!({
        "purpose": options.purpose, "executable": journal::reference(&program.to_string_lossy()),
        "launcher_pid": std::process::id(), "launcher_ppid": unsafe { libc::getppid() },
        "caller": journal::reference(&caller()),
    });
    let mut value = json!({"kind": kind, "source": "native.Conveyor", "phase": "event"});
    let map = value.as_object_mut().expect("object");
    if !options.subject.is_empty() {
        map.insert("subject".into(), json!(options.subject));
    }
    if !options.request.is_empty() {
        map.insert("request".into(), json!(options.request));
    }
    if let Some(extra) = extra.as_object() {
        for (key, item) in extra {
            if matches!(key.as_str(), "child_pid" | "exit_code") {
                context[key] = item.clone();
            } else {
                map.insert(key.clone(), item.clone());
            }
        }
    }
    map.insert("context".into(), context);
    value
}

fn launched(program: &OsStr, options: &Options, child: &Child) {
    if options.journal && journal::enabled() {
        journal::emit(&event("native.process.launch", program, options, json!({"outcome": "spawned", "child_pid": child.id()})));
    }
}

fn finished(program: &OsStr, options: &Options, output: &Output) {
    if options.journal && journal::enabled() {
        let outcome = if output.success() { "ok" } else { "error" };
        let mut extra = json!({"outcome": outcome, "child_pid": output.pid});
        if let Some(code) = output.code {
            extra["exit_code"] = json!(code);
        }
        journal::emit(&event("native.process.result", program, options, extra));
    }
}

/// Starts a program and waits for it. A timeout kills it and says so in `timed_out`; it is not started again.
/// Err only when it could not be started at all.
pub fn run<S: AsRef<OsStr>>(program: S, arguments: &[S], options: &Options) -> std::io::Result<Output> {
    let program = program.as_ref();
    let arguments: Vec<OsString> = arguments.iter().map(|word| word.as_ref().to_os_string()).collect();
    let mut command = command_of(program, &arguments, options);
    command.stdin(if options.input.is_some() { Stdio::piped() } else { Stdio::null() });
    if options.capture {
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
    }
    let mut child = command.spawn()?;
    launched(program, options, &child);
    if let (Some(bytes), Some(mut stdin)) = (options.input.clone(), child.stdin.take()) {
        std::thread::spawn(move || {
            let _ = stdin.write_all(&bytes);
        });
    }
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        pipe.map(|mut pipe| {
            std::thread::spawn(move || {
                let mut bytes = Vec::new();
                let _ = pipe.read_to_end(&mut bytes);
                bytes
            })
        })
    };
    let out = drain(child.stdout.take().map(|pipe| Box::new(pipe) as Box<dyn Read + Send>));
    let err = drain(child.stderr.take().map(|pipe| Box::new(pipe) as Box<dyn Read + Send>));
    let deadline = options.timeout.map(|limit| Instant::now() + limit);
    let mut timed_out = false;
    let status = loop {
        match child.try_wait()? {
            Some(status) => break status,
            None if deadline.is_some_and(|at| Instant::now() >= at) => {
                timed_out = true;
                let _ = child.kill();
                break child.wait()?;
            }
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    };
    let output = Output {
        pid: child.id(),
        code: if timed_out { None } else { status.code() },
        signal: status.signal(),
        timed_out,
        stdout: out.map(|reading| reading.join().unwrap_or_default()).unwrap_or_default(),
        stderr: err.map(|reading| reading.join().unwrap_or_default()).unwrap_or_default(),
    };
    finished(program, options, &output);
    Ok(output)
}

/// Starts a program with its stdout and stderr written to the given files and its stdin inherited, and waits for it.
/// `Output.stdout` and `.stderr` stay empty: the bytes are in the files. Err only when it could not be started.
pub fn run_to_files<S: AsRef<OsStr>>(program: S, arguments: &[S], stdout: std::fs::File, stderr: std::fs::File,
                                     options: &Options) -> std::io::Result<Output> {
    let program = program.as_ref();
    let arguments: Vec<OsString> = arguments.iter().map(|word| word.as_ref().to_os_string()).collect();
    let mut command = command_of(program, &arguments, options);
    command.stdin(Stdio::inherit()).stdout(Stdio::from(stdout)).stderr(Stdio::from(stderr));
    let mut child = command.spawn()?;
    launched(program, options, &child);
    let status = child.wait()?;
    let output = Output { pid: child.id(), code: status.code(), signal: status.signal(), timed_out: false,
                          stdout: Vec::new(), stderr: Vec::new() };
    finished(program, options, &output);
    Ok(output)
}

/// Starts a program and leaves it running: the child, with its launch written to the journal.
pub fn spawn<S: AsRef<OsStr>>(program: S, arguments: &[S], options: &Options) -> std::io::Result<Child> {
    let program = program.as_ref();
    let arguments: Vec<OsString> = arguments.iter().map(|word| word.as_ref().to_os_string()).collect();
    let mut command = command_of(program, &arguments, options);
    command.stdin(if options.input.is_some() { Stdio::piped() } else { Stdio::null() });
    if options.capture {
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
    }
    let child = command.spawn()?;
    launched(program, options, &child);
    Ok(child)
}

/// Replaces this process with a program. Only returns, with the error, when that failed. The intent is written to
/// the journal first and the journal's pipe is let go, since nothing runs after a successful exec.
pub fn exec<S: AsRef<OsStr>>(program: S, arguments: &[S], options: &Options) -> std::io::Error {
    let program = program.as_ref();
    let arguments: Vec<OsString> = arguments.iter().map(|word| word.as_ref().to_os_string()).collect();
    if options.journal && journal::enabled() {
        journal::emit(&event("native.process.launch", program, options, json!({"outcome": "exec_requested"})));
        journal::shutdown();
    }
    command_of(program, &arguments, options).exec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_and_code_are_taken() {
        let done = run("sh", &["-c", "echo out; echo err >&2; exit 3"], &Options::new("test.run")).unwrap();
        assert_eq!((done.out().as_str(), done.err().as_str(), done.code), ("out\n", "err\n", Some(3)));
        assert!(!done.success());
    }

    #[test]
    fn output_can_go_to_files_and_the_status_is_kept() {
        let folder = std::env::temp_dir().join(format!("hitl-host-core-files-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let (out, err) = (std::fs::File::create(folder.join("out")).unwrap(), std::fs::File::create(folder.join("err")).unwrap());
        let done = run_to_files("sh", &["-c", "echo out; echo err >&2; exit 4"], out, err, &Options::new("test.files")).unwrap();
        assert_eq!(done.code, Some(4));
        assert_eq!(std::fs::read_to_string(folder.join("out")).unwrap(), "out\n");
        assert_eq!(std::fs::read_to_string(folder.join("err")).unwrap(), "err\n");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn input_reaches_the_program() {
        let done = run("cat", &[], &Options::new("test.input").input(b"words")).unwrap();
        assert_eq!(done.out(), "words");
        assert!(done.success());
    }

    #[test]
    fn a_program_that_outlives_its_timeout_is_killed_once() {
        let began = Instant::now();
        let done = run("sleep", &["30"], &Options::new("test.timeout").timeout(0.2)).unwrap();
        assert!(done.timed_out && done.code.is_none() && !done.success());
        assert!(began.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn environment_is_set_and_removed() {
        let options = Options::new("test.env").env("MADE_UP_ONE", "1").env_remove("HOME");
        let done = run("sh", &["-c", "echo $MADE_UP_ONE:${HOME-gone}"], &options).unwrap();
        assert_eq!(done.out(), "1:gone\n");
    }

    #[test]
    fn a_missing_program_is_an_error_and_not_a_retry() {
        assert!(run("/nonexistent/program", &[], &Options::new("test.missing")).is_err());
    }

    #[test]
    fn the_purpose_keeps_its_end() {
        let long = "a".repeat(100) + "end";
        assert_eq!(Options::new(&long).purpose.len(), 80);
        assert!(Options::new(&long).purpose.ends_with("end"));
    }
}
