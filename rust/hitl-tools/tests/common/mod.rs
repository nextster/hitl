//! What the suites of `hitl update` and `hitl install` share: made-up folders and the command run through the
//! entry, the file `hitl`, which builds and runs the program of rust/hitl-host (once per version, into a cache).
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

static NUMBER: AtomicU32 = AtomicU32::new(0);

/// The project: the folder with the file `hitl`, two levels above this crate.
pub fn root() -> PathBuf {
    std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).expect("the project")
}

/// A folder of its own, removed when it goes out of scope.
pub struct Temp(pub PathBuf);

impl Temp {
    pub fn new(label: &str) -> Temp {
        let path = std::fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("conveyor-{label}-{}-{}", std::process::id(), NUMBER.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Temp(path)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

pub fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

pub fn git(folder: &Path, arguments: &[&str]) -> String {
    let done = Command::new("git")
        .arg("-C")
        .arg(folder)
        .args(["-c", "user.name=test", "-c", "user.email=test@example.test"])
        .args(arguments)
        .output()
        .expect("git runs");
    assert!(done.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&done.stderr));
    String::from_utf8_lossy(&done.stdout).trim().to_string()
}

/// The environment the command is run in: this one without what the tools themselves set (`CONVEYOR_*`), with the
/// build tools and the cache of the program kept where the real home has them, since a made-up `HOME` is only the
/// place the command looks for links and settings.
pub fn environment() -> BTreeMap<String, String> {
    let mut all: BTreeMap<String, String> = std::env::vars().filter(|(key, _)| !key.starts_with("CONVEYOR_")).collect();
    let real = all.get("HOME").cloned().unwrap_or_default();
    all.entry("XDG_CACHE_HOME".into()).or_insert_with(|| format!("{real}/Library/Caches"));
    for (key, child) in [("RUSTUP_HOME", ".rustup"), ("CARGO_HOME", ".cargo")] {
        all.entry(key.into()).or_insert_with(|| format!("{real}/{child}"));
    }
    all
}

/// The command through the entry, with what it printed (both streams) and its exit code.
pub fn hitl(arguments: &[&str], environment: &BTreeMap<String, String>) -> (i32, String) {
    let done = conveyor_output(arguments, environment);
    (done.status.code().unwrap_or(-1), String::from_utf8_lossy(&done.stdout).to_string() + &String::from_utf8_lossy(&done.stderr))
}

pub fn conveyor_output(arguments: &[&str], environment: &BTreeMap<String, String>) -> Output {
    Command::new(root().join("hitl"))
        .args(arguments)
        .env_clear()
        .envs(environment)
        .stdin(Stdio::null())
        .output()
        .expect("the entry runs")
}
