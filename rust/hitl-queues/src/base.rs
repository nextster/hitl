//! What every module of the queues shares: where the state lives, a file lock, whether a process is alive.
//!
//! The port of the top of `src/hitl/queues.py`. The state on disk is read by the Python that still runs and by
//! commands that are already waiting with old code, so its folders and its JSON stay as they were.

use hitl_host_core::{home, var};
use serde_json::Value;
use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

pub const STATE_VERSION: i64 = 1;

/// argparse's exit code for a command line it refuses.
pub const EXIT_USAGE: i32 = 2;

/// The moment now, in seconds. In the tests a clock they set stands in for it (Python's tests patched `queues.now`).
pub fn now() -> f64 {
    #[cfg(test)]
    if let Some(moment) = *testing::CLOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) {
        return moment;
    }
    hitl_host_core::now()
}

/// A line on stdout, as `print` writes it. In the tests it goes to the capture of the thread, when there is one.
pub fn print_line(text: &str) {
    #[cfg(test)]
    if testing::captured(text, false) {
        return;
    }
    use std::io::Write;
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{text}");
    let _ = out.flush();
}

/// A line on stderr (`common.say`). In the tests it goes to the capture of the thread, when there is one.
pub fn say(text: &str) {
    #[cfg(test)]
    if testing::captured(text, true) {
        return;
    }
    hitl_host_core::say(text);
}

/// `print` of the Python.
#[macro_export]
macro_rules! out {
    ($($argument:tt)*) => { $crate::base::print_line(&format!($($argument)*)) };
}

/// One folder per phone, named by its UDID.
pub fn state_root() -> PathBuf {
    home().join("iphone")
}

/// The lines beside the phones': test runs on the Mac, merges into main.
pub fn turns_root() -> PathBuf {
    home().join("turns")
}

/// `CONVEYOR_POLL_SECONDS`, 3 when not set.
pub fn poll_seconds() -> f64 {
    var("CONVEYOR_POLL_SECONDS").and_then(|text| text.trim().parse().ok()).unwrap_or(3.0)
}

/// Whether the process a state file names runs: a number that is not one, or 0, is no process.
pub fn process_alive(pid: Option<i64>) -> bool {
    let Some(pid) = pid.filter(|pid| *pid > 0 && *pid <= i32::MAX as i64) else { return false };
    if unsafe { libc::kill(pid as i32, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// `process_alive` of the `pid` field of a record.
pub fn record_alive(record: &Value) -> bool {
    process_alive(record.get("pid").and_then(Value::as_i64))
}

/// An advisory lock on a file (`flock`), held until this value is dropped. The file is created when it is not there.
pub struct FileLock {
    file: File,
}

impl FileLock {
    fn open(path: &Path, append: bool) -> std::io::Result<File> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        // Python opens "w" for the state lock and "a" for the dispatcher's; neither is read.
        let mut options = OpenOptions::new();
        options.create(true).write(true);
        if append {
            options.append(true);
        } else {
            options.truncate(true);
        }
        options.open(path)
    }

    /// Waits for the lock.
    pub fn exclusive(path: &Path) -> std::io::Result<FileLock> {
        let file = FileLock::open(path, false)?;
        while unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
            if std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                return Err(std::io::Error::last_os_error());
            }
        }
        Ok(FileLock { file })
    }

    /// The lock if it is free now; None when another process holds it.
    pub fn try_exclusive(path: &Path) -> std::io::Result<Option<FileLock>> {
        let file = FileLock::open(path, true)?;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            Ok(Some(FileLock { file }))
        } else {
            Ok(None)
        }
    }

    /// Whether another holder has the lock on a file that is there; false when the file is not.
    pub fn held_elsewhere(path: &Path) -> bool {
        if !path.exists() {
            return false;
        }
        matches!(FileLock::try_exclusive(path), Ok(None))
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}

/// A field of a state record as text, "" when it is missing or not text.
pub fn text<'a>(record: &'a Value, key: &str) -> &'a str {
    record.get(key).and_then(Value::as_str).unwrap_or("")
}

/// A field as a number; 0 when it is missing or not one.
pub fn number(record: &Value, key: &str) -> f64 {
    record.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

/// `record[key]` as a list that may be changed, made when it is missing.
pub fn list_mut<'a>(record: &'a mut Value, key: &str) -> &'a mut Vec<Value> {
    let map = record.as_object_mut().expect("a state record is an object");
    let slot = map.entry(key.to_string()).or_insert_with(|| Value::Array(Vec::new()));
    if !slot.is_array() {
        *slot = Value::Array(Vec::new());
    }
    slot.as_array_mut().expect("a list")
}

#[cfg(test)]
pub mod testing {
    //! Tests change the process environment, which is shared: they take turns, each in a made-up home.
    use std::cell::RefCell;
    use std::sync::{Mutex, MutexGuard};

    static TURN: Mutex<()> = Mutex::new(());
    /// The clock the tests set; None is the real one.
    pub static CLOCK: Mutex<Option<f64>> = Mutex::new(None);

    thread_local! {
        static CAPTURE: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
    }

    /// Takes a line for the capture of this thread: true when there is one.
    pub fn captured(text: &str, error: bool) -> bool {
        CAPTURE.with(|capture| match capture.borrow_mut().as_mut() {
            Some((out, err)) => {
                let target = if error { err } else { out };
                target.push_str(text);
                target.push('\n');
                true
            }
            None => false,
        })
    }

    /// Runs `action` with what it prints and says taken: (its result, stdout, stderr).
    pub fn capture<R>(action: impl FnOnce() -> R) -> (R, String, String) {
        CAPTURE.with(|capture| *capture.borrow_mut() = Some((String::new(), String::new())));
        let result = action();
        let (out, err) = CAPTURE.with(|capture| capture.borrow_mut().take()).unwrap_or_default();
        (result, out, err)
    }

    /// Stops the clock at a moment (or lets it go again with None).
    pub fn set_clock(moment: Option<f64>) {
        *CLOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = moment;
    }

    /// A made-up `CONVEYOR_HOME` and the settings every test of the queues runs under, given back at the end.
    pub struct Sandbox {
        pub folder: std::path::PathBuf,
        saved: Vec<(String, Option<String>)>,
        _turn: MutexGuard<'static, ()>,
    }

    impl Sandbox {
        pub fn new(name: &str) -> Sandbox {
            let turn = TURN.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let folder = std::env::temp_dir().join(format!("hitl-queues-{name}-{}-{nanos}", std::process::id()));
            std::fs::create_dir_all(&folder).unwrap();
            let mut sandbox = Sandbox { folder: folder.clone(), saved: Vec::new(), _turn: turn };
            let home = folder.join("state");
            sandbox.set("HITL_HOME", home.to_str().unwrap());
            sandbox.set("CONVEYOR_HOME", home.to_str().unwrap());
            for (key, value) in [("CONVEYOR_NO_DISPATCHER", "1"), ("CONVEYOR_WINDOW", ""), ("CONVEYOR_NOTIFY", "0"),
                                 ("CONVEYOR_CHAT", ""), ("CONVEYOR_MAC_TEST_SCAN", "0"), ("CONVEYOR_NO_MESSENGER", "1")] {
                sandbox.set(key, value);
            }
            for key in ["CLAUDE_CODE_HOST_SESSION_ID", "CLAUDE_CODE_SESSION_ID", "CLAUDECODE", "CODEX_THREAD_ID", "OPENCODE",
                        "OPENCODE_SESSION_ID", "AI_AGENT"] {
                sandbox.unset(key);
            }
            sandbox
        }

        pub fn set(&mut self, key: &str, value: &str) {
            self.saved.push((key.to_string(), std::env::var(key).ok()));
            unsafe { std::env::set_var(key, value) };
        }

        pub fn unset(&mut self, key: &str) {
            self.saved.push((key.to_string(), std::env::var(key).ok()));
            unsafe { std::env::remove_var(key) };
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            for (key, value) in self.saved.drain(..).rev() {
                match value {
                    Some(value) => unsafe { std::env::set_var(&key, value) },
                    None => unsafe { std::env::remove_var(&key) },
                }
            }
            set_clock(None);
            crate::bridge::join_window_threads();
            crate::bridge::reset_standins();
            let _ = std::fs::remove_dir_all(&self.folder);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_process_is_alive_when_it_runs() {
        assert!(process_alive(Some(std::process::id() as i64)));
        assert!(!process_alive(None) && !process_alive(Some(0)) && !process_alive(Some(-4)));
    }

    #[test]
    fn a_lock_is_held_until_dropped() {
        let path = std::env::temp_dir().join(format!("queues-lock-{}", std::process::id()));
        let held = FileLock::try_exclusive(&path).unwrap().unwrap();
        // flock locks belong to the open file: a second open in this process is another holder.
        assert!(FileLock::held_elsewhere(&path));
        drop(held);
        assert!(!FileLock::held_elsewhere(&path));
        let _ = std::fs::remove_file(path);
    }
}
