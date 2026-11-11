//! What the suites of hitl-distribution share: made-up folders, and hitl run through its entry, the file `hitl`,
//! which builds and runs the program of rust/hitl-host (once per version, into the cache).
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

/// The project: the folder with the file `hitl`, two levels above this crate.
pub fn project() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// A folder of its own, removed when it goes out of scope.
pub struct Folder(pub PathBuf);

impl Folder {
    pub fn new(label: &str) -> Folder {
        static NUMBER: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("hitl-distribution-{label}-{}-{}", std::process::id(), NUMBER.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Folder(path)
    }

    pub fn join(&self, path: &str) -> PathBuf {
        self.0.join(path)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub struct Run {
    pub code: i32,
    pub out: String,
    pub err: String,
}

/// The environment hitl runs in here: a made-up home, state and hitl home in `folder`, no window, no chat app,
/// no notification and no journal (a test that wants one says so), with the build tools and the cache of the
/// program kept where the real home has them: a made-up `HOME` is only where the command looks for its own things.
pub fn environment(folder: &Path) -> BTreeMap<String, String> {
    let real = std::env::var("HOME").unwrap_or_default();
    let mut all: BTreeMap<String, String> = BTreeMap::new();
    for name in ["PATH", "LANG", "USER", "LOGNAME"] {
        if let Ok(value) = std::env::var(name) {
            all.insert(name.into(), value);
        }
    }
    let kept = |name: &str, fallback: String| std::env::var(name).ok().filter(|value| !value.is_empty()).unwrap_or(fallback);
    all.insert("XDG_CACHE_HOME".into(), kept("XDG_CACHE_HOME", format!("{real}/Library/Caches")));
    all.insert("CARGO_HOME".into(), kept("CARGO_HOME", format!("{real}/.cargo")));
    all.insert("RUSTUP_HOME".into(), kept("RUSTUP_HOME", format!("{real}/.rustup")));
    for (name, value) in [
        ("HOME", folder.join("home").display().to_string()),
        ("CONVEYOR_HOME", folder.join("state").display().to_string()),
        ("HITL_HOME", folder.join("hitl-home").display().to_string()),
        ("TMPDIR", folder.join("tmp").display().to_string()),
        ("CONVEYOR_WINDOW", String::new()),
        ("CONVEYOR_NOTIFY", "0".into()),
        ("CONVEYOR_JOURNAL", "0".into()),
        ("CONVEYOR_CODEX", String::new()),
        ("CONVEYOR_CLAUDE", String::new()),
        ("CONVEYOR_OPENCODE", String::new()),
        ("TZ", "UTC".into()),
    ] {
        all.insert(name.into(), value);
    }
    for made in ["home", "tmp"] {
        std::fs::create_dir_all(folder.join(made)).unwrap();
    }
    all
}

/// Runs a program with exactly this environment and no input.
pub fn run(program: &Path, arguments: &[&str], environment: &BTreeMap<String, String>, cwd: &Path) -> Run {
    let done = Command::new(program)
        .args(arguments)
        .env_clear()
        .envs(environment)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|error| panic!("{}: {error}", program.display()));
    Run {
        code: done.status.code().unwrap_or(-1),
        out: String::from_utf8_lossy(&done.stdout).to_string(),
        err: String::from_utf8_lossy(&done.stderr).to_string(),
    }
}

/// `hitl ARGS` through the project's entry.
pub fn hitl(arguments: &[&str], environment: &BTreeMap<String, String>, cwd: &Path) -> Run {
    let mut host = environment.clone();
    host.remove("HITL_HOME");
    run(&project().join("hitl"), arguments, &host, cwd)
}

pub fn git(folder: &Path, arguments: &[&str]) -> String {
    let done = Command::new("git")
        .arg("-C")
        .arg(folder)
        .args(["-c", "user.name=test", "-c", "user.email=test@example.test", "-c", "init.defaultBranch=main"])
        .args(arguments)
        .output()
        .expect("git runs");
    assert!(done.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&done.stderr));
    String::from_utf8_lossy(&done.stdout).trim().to_string()
}
