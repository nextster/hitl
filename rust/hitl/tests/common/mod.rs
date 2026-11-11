//! What the command-level tests share: the program `hitl` run as hitl's core (`HITL_TOOL=hitl-host`) in a made-up
//! world of its own, the way `tests/check-tests.py` ran `cli.main` with a made-up home (`load`, `run_tool`, `finish`).
//!
//! Every test has its folders under the temporary folder, removed after it, and gives the program its whole
//! environment: nothing of whoever runs the tests reaches it (no window, no real home, no real `codex`, `claude` or
//! `opencode`, no socket of an app), and the process's own environment is never changed, so tests run side by side.
#![allow(dead_code)]

use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const ABOUT: &str = "Плагин Obsidian для Realize: делаю ему вход без пароля.";
pub const CHAT: &str = "r/66 · API для плагина";
/// How the messages and hints name the command: hitl hands its own path, and "hitl wait X" is in it.
pub const COMMAND: &str = "/made/up/scripts/hitl";

/// What a call of the program did.
#[derive(Debug)]
pub struct Out {
    pub code: i32,
    pub out: String,
    pub err: String,
}

impl Out {
    /// The ticket's number, as `check` prints it.
    pub fn id(&self) -> String {
        self.out.trim().to_string()
    }
}

/// A made-up world: a home of hitl, a home of the user, a folder to work in, and the environment of the chat
/// that calls the command.
pub struct World {
    pub root: PathBuf,
    /// `CONVEYOR_HOME`: the queue, the verdicts, the history.
    pub home: PathBuf,
    /// Where the command is run from: no repository, unless a test says otherwise.
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl World {
    /// The world of `load()`: a made-up home, no window, and a chat named by hand, "r/66 · API для плагина".
    pub fn new(name: &str) -> World {
        let base = std::env::temp_dir().canonicalize().unwrap_or_else(|_| std::env::temp_dir());
        let root = base.join(format!("hitl-it-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for part in ["hitl", "user", "tmp", "work", "claude-app", "claude-projects", "claude-live"] {
            std::fs::create_dir_all(root.join(part)).unwrap();
        }
        let home = root.join("hitl");
        let path = |part: &str| root.join(part).display().to_string();
        let mut env = BTreeMap::new();
        for (key, value) in [
            ("PATH", "/usr/bin:/bin".to_string()), ("HOME", path("user")), ("TMPDIR", path("tmp")),
            ("HITL_TOOL", "hitl-host".into()), ("HITL_COMMAND", COMMAND.into()),
            ("CONVEYOR_HOME", home.display().to_string()), ("HITL_HOME", home.display().to_string()),
            ("CONVEYOR_WINDOW", String::new()), ("CONVEYOR_NOTIFY", "0".into()), ("CONVEYOR_JOURNAL", "0".into()),
            ("CONVEYOR_NO_MESSENGER", "1".into()), ("CONVEYOR_CHAT", CHAT.into()),
            ("CONVEYOR_CARDS", home.join("vault/cards").display().to_string()),
            // No test hands a message to a real Codex, has a real `claude` send one, or reads a real app's records.
            ("CONVEYOR_CODEX", String::new()), ("CONVEYOR_CLAUDE", String::new()), ("CONVEYOR_OPENCODE", String::new()),
            ("CONVEYOR_CODEX_STATE", path("no-codex.sqlite")), ("CONVEYOR_OPENCODE_STATE", path("no-opencode.db")),
            ("CONVEYOR_CLAUDE_SESSIONS_DIR", path("claude-app")), ("CONVEYOR_CLAUDE_PROJECTS_DIR", path("claude-projects")),
            ("CONVEYOR_CLAUDE_LIVE_DIR", path("claude-live")), ("CONVEYOR_CLAUDE_APP_RUNNING", "0".into()),
            ("CONVEYOR_TELL_POLL", "0.01".into()), ("CONVEYOR_TELL_GRACE", "0.05".into()),
            ("CONVEYOR_TELL_RECEIPT_WAIT", "0.3".into()),
        ] {
            env.insert(key.to_string(), value);
        }
        World { cwd: root.join("work"), home, root, env }
    }

    pub fn set(&mut self, key: &str, value: impl AsRef<str>) {
        self.env.insert(key.to_string(), value.as_ref().to_string());
    }

    pub fn unset(&mut self, key: &str) {
        self.env.remove(key);
    }

    fn command(&self, arguments: &[&str], env: &BTreeMap<String, String>, cwd: &Path) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_hitl"));
        command.args(arguments).env_clear().envs(env).current_dir(cwd).stdin(Stdio::null());
        command
    }

    /// A call of the program, with the world's environment.
    pub fn run(&self, arguments: &[&str]) -> Out {
        self.run_in(arguments, &self.env, &self.cwd)
    }

    /// A call with some of the environment changed: `None` takes a name away.
    pub fn run_with(&self, arguments: &[&str], changes: &[(&str, Option<&str>)]) -> Out {
        self.run_in(arguments, &self.changed(changes), &self.cwd)
    }

    pub fn changed(&self, changes: &[(&str, Option<&str>)]) -> BTreeMap<String, String> {
        let mut env = self.env.clone();
        for (key, value) in changes {
            match value {
                Some(value) => env.insert(key.to_string(), value.to_string()),
                None => env.remove(*key),
            };
        }
        env
    }

    pub fn run_in(&self, arguments: &[&str], env: &BTreeMap<String, String>, cwd: &Path) -> Out {
        finished(self.command(arguments, env, cwd), None, arguments, true)
    }

    /// A call whose standard output goes into a file, as a shell's `> file` puts it.
    pub fn run_into_file(&self, arguments: &[&str], file: &Path) -> Out {
        let mut command = self.command(arguments, &self.env, &self.cwd);
        command.stdout(Stdio::from(std::fs::File::create(file).unwrap()));
        let mut done = finished(command, None, arguments, false);
        done.out = std::fs::read_to_string(file).unwrap_or_default();
        done
    }

    /// A call with something on its standard input.
    pub fn run_fed(&self, arguments: &[&str], input: &[u8]) -> Out {
        finished(self.command(arguments, &self.env, &self.cwd), Some(input), arguments, true)
    }

    /// `run_tool("--title", …)`: a check for the window, with its sentence about the task unless one is given.
    pub fn check(&self, arguments: &[&str]) -> Out {
        let words = with_about(arguments);
        self.run(&words.iter().map(String::as_str).collect::<Vec<_>>())
    }

    /// The same, with some of the environment changed.
    pub fn check_with(&self, arguments: &[&str], changes: &[(&str, Option<&str>)]) -> Out {
        let words = with_about(arguments);
        self.run_with(&words.iter().map(String::as_str).collect::<Vec<_>>(), changes)
    }

    /// A check with no sentence about the task: `run_tool(..., about=False)`.
    pub fn check_bare(&self, arguments: &[&str]) -> Out {
        let mut words = vec!["check"];
        words.extend_from_slice(arguments);
        self.run(&words)
    }

    /// The program's messenger of one check, run here to its end: `_tell ID`.
    pub fn messenger(&self, ticket: &str) -> i32 {
        self.run(&["_tell", ticket]).code
    }

    pub fn ticket_file(&self, id: &str) -> PathBuf {
        self.home.join("queue").join(format!("{id}.json"))
    }

    pub fn verdict_file(&self, id: &str) -> PathBuf {
        self.home.join("done").join(format!("{id}.json"))
    }

    pub fn record_file(&self, id: &str) -> PathBuf {
        self.home.join("tell").join(format!("{id}.json"))
    }

    pub fn ticket(&self, id: &str) -> Value {
        read(&self.ticket_file(id))
    }

    pub fn record(&self, id: &str) -> Value {
        read(&self.record_file(id))
    }

    /// The checks that wait in the queue's folder.
    pub fn queued(&self) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = std::fs::read_dir(self.home.join("queue")).map(|entries| entries.flatten().map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "json")).collect()).unwrap_or_default();
        found.sort();
        found
    }

    /// Every ended check, a line each.
    pub fn history(&self) -> Vec<Value> {
        std::fs::read_to_string(self.home.join("history.jsonl")).unwrap_or_default().lines()
            .filter_map(|line| serde_json::from_str(line).ok()).collect()
    }

    /// His verdict, written the way the window writes it: into `done/`, into the history, and the ticket taken away.
    pub fn finish(&self, id: &str, states: &[Option<&str>]) {
        self.finish_as(id, states, Answers::default())
    }

    pub fn finish_as(&self, id: &str, states: &[Option<&str>], answers: Answers) {
        let ticket = self.ticket(id);
        let items = ticket["items"].as_array().cloned().unwrap_or_default();
        let results: Vec<Value> = items.iter().zip(states).map(|(item, state)| {
            let label = item["label"].as_str().unwrap_or("").to_string();
            let find = |pairs: &[(&str, &str)]| pairs.iter().find(|(at, _)| *at == label).map(|(_, value)| value.to_string());
            json!({"label": label, "state": state, "choice": find(answers.choices), "note": find(answers.notes).unwrap_or_default(),
                   "question": if item.get("options").is_some() { json!(true) } else { Value::Null },
                   "shots": answers.shots.iter().find(|(at, _)| *at == label).map(|(_, shots)| json!(shots)).unwrap_or(Value::Null)})
        }).collect();
        let mut verdict = serde_json::Map::new();
        for key in ["id", "tag", "chat", "title", "kind", "project", "branch", "commit", "createdAt"] {
            verdict.insert(key.into(), ticket.get(key).cloned().unwrap_or(Value::Null));
        }
        verdict.insert("results".into(), Value::Array(results));
        verdict.insert("how".into(), json!(answers.how));
        verdict.insert("shownAt".into(), json!(1.0));
        verdict.insert("finishedAt".into(), json!(2.0));
        self.write_verdict(id, &Value::Object(verdict));
    }

    /// A verdict as given, into `done/` and the history, and the ticket taken away.
    pub fn write_verdict(&self, id: &str, verdict: &Value) {
        std::fs::create_dir_all(self.home.join("done")).unwrap();
        std::fs::write(self.verdict_file(id), verdict.to_string()).unwrap();
        let mut history = std::fs::OpenOptions::new().create(true).append(true).open(self.home.join("history.jsonl")).unwrap();
        writeln!(history, "{verdict}").unwrap();
        let _ = std::fs::remove_file(self.ticket_file(id));
    }

    /// Changes the verdict in `done/` alone, as a test of Python changed it after `finish`.
    pub fn change_verdict(&self, id: &str, change: impl FnOnce(&mut Value)) {
        let mut verdict = read(&self.verdict_file(id));
        change(&mut verdict);
        std::fs::write(self.verdict_file(id), verdict.to_string()).unwrap();
    }

    /// A file of the world with this text.
    pub fn file(&self, relative: &str, text: impl AsRef<[u8]>) -> PathBuf {
        let path = self.root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        path
    }

    /// A program of the world: a shell script with this text.
    pub fn program(&self, relative: &str, text: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = self.file(relative, text);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }
}

/// What he answered besides the marks: how he ended the check, his comments, his choices, his screenshots.
pub struct Answers<'a> {
    pub how: &'a str,
    pub notes: &'a [(&'a str, &'a str)],
    pub choices: &'a [(&'a str, &'a str)],
    pub shots: &'a [(&'a str, &'a [&'a str])],
}

impl Default for Answers<'_> {
    fn default() -> Self {
        Answers { how: "done", notes: &[], choices: &[], shots: &[] }
    }
}

fn with_about(arguments: &[&str]) -> Vec<String> {
    let mut words = vec!["check".to_string()];
    if arguments.contains(&"--check") && !arguments.contains(&"--about") {
        words.push("--about".into());
        words.push(ABOUT.into());
    }
    words.extend(arguments.iter().map(|word| word.to_string()));
    words
}

/// Runs a call to its end, a minute at most: a check that was taken by mistake would wait for a window that never
/// comes, and fails the test instead of holding it.
fn finished(mut command: Command, input: Option<&[u8]>, arguments: &[&str], pipe_out: bool) -> Out {
    use std::io::Read;
    use std::time::{Duration, Instant};
    if input.is_some() {
        command.stdin(Stdio::piped());
    }
    if pipe_out {
        command.stdout(Stdio::piped());
    }
    let mut child = command.stderr(Stdio::piped()).spawn().expect("the program runs");
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        stdin.write_all(input).unwrap();
    }
    let reader = |stream: Option<Box<dyn Read + Send>>| std::thread::spawn(move || {
        let mut data = Vec::new();
        if let Some(mut stream) = stream {
            let _ = stream.read_to_end(&mut data);
        }
        String::from_utf8_lossy(&data).into_owned()
    });
    let out = reader(child.stdout.take().map(|stream| Box::new(stream) as Box<dyn Read + Send>));
    let err = reader(child.stderr.take().map(|stream| Box::new(stream) as Box<dyn Read + Send>));
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("hitl {arguments:?} did not end in a minute");
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    Out { code: status.code().unwrap_or(-1), out: out.join().unwrap(), err: err.join().unwrap() }
}

pub fn read(path: &Path) -> Value {
    std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or(Value::Null)
}

/// `git` in a folder of a test, as a made-up person, with nothing of the user's own settings.
pub fn git(world: &World, arguments: &[&str]) -> String {
    let done = Command::new("git").args(["-c", "user.name=Fixture", "-c", "user.email=fixture@invalid", "-c", "init.defaultBranch=main"])
        .args(arguments).env_clear().envs(&world.env).current_dir(&world.root).output().expect("git runs");
    assert!(done.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&done.stderr));
    String::from_utf8_lossy(&done.stdout).trim().to_string()
}

pub fn contains(text: &str, part: &str) {
    assert!(text.contains(part), "«{part}» is not in:\n{text}");
}

pub fn lacks(text: &str, part: &str) {
    assert!(!text.contains(part), "«{part}» is in:\n{text}");
}
