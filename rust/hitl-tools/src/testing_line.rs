//! The line of full runs of `hitl test` on this Mac: one run at a time, the next as soon as one ends.
//!
//! The port of `queues.Line` as `queues.ConveyorTests` uses it, writing the same state (`turns/conveyor-tests/`:
//! `state.json`, `state.lock`) as the Python does, so a Python run and a Rust one wait for each other. Until the
//! queues are moved this is a copy of the part of the line a test run needs; the moved queues take its place.

use hitl_host_core::{EXIT_BUSY, clock, dumps, home, minutes, now, say, var};
use serde_json::{Value, json};
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::time::Duration;

const FOLDER: &str = "conveyor-tests";

fn number_var(name: &str, default: f64) -> f64 {
    var(name).and_then(|value| value.trim().parse::<f64>().ok()).unwrap_or(default)
}

pub struct Line {
    folder: PathBuf,
    slots: usize,
    limit_seconds: f64,
}

fn process_alive(pid: &Value) -> bool {
    let Some(pid) = pid.as_i64().filter(|pid| *pid > 0) else { return false };
    // SAFETY: signal 0 only asks whether the process exists.
    let found = unsafe { libc::kill(pid as libc::pid_t, 0) };
    found == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

fn alive_at(run: &Value) -> f64 {
    let seen = |key: &str| run.get(key).and_then(Value::as_f64).unwrap_or(0.0);
    seen("since").max(seen("beat"))
}

fn holders(state: &Value) -> Vec<Value> {
    let mut all = Vec::new();
    if let Some(running) = state.get("running").filter(|running| !running.is_null()) {
        all.push(running.clone());
    }
    all.extend(state.get("also").and_then(Value::as_array).cloned().unwrap_or_default());
    all
}

fn describe(run: &Value) -> String {
    let tag = run.get("tag").and_then(Value::as_str).filter(|tag| !tag.is_empty()).unwrap_or("?");
    match run.get("what").and_then(Value::as_str).filter(|what| !what.is_empty()) {
        Some(what) => format!("{tag} · {what}"),
        None => tag.to_string(),
    }
}

impl Line {
    /// The line of full runs of the tests, in the made-up or the real home as `CONVEYOR_HOME` says.
    pub fn conveyor_tests() -> Line {
        let slots = var("CONVEYOR_TEST_SLOTS").and_then(|value| value.trim().parse::<usize>().ok()).unwrap_or(1).max(1);
        Line { folder: home().join("turns").join(FOLDER), slots, limit_seconds: number_var("CONVEYOR_TEST_LIMIT_SECONDS", 1800.0) }
    }

    /// The line's folder, where its holder says how far it is.
    pub fn folder(&self) -> PathBuf {
        self.folder.clone()
    }

    /// State under a lock, with the turns of dead and hung processes dropped; `change` may alter it, and what it
    /// returns is handed back.
    fn locked<T>(&self, change: impl FnOnce(&mut Value) -> T) -> std::io::Result<T> {
        std::fs::create_dir_all(&self.folder)?;
        let path = self.folder.join("state.json");
        let lock = std::fs::File::create(self.folder.join("state.lock"))?;
        // SAFETY: a plain flock on a file held open until the end of this call.
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) };
        let mut state = std::fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}));
        let object = state.as_object_mut().expect("object");
        object.entry("running").or_insert(Value::Null);
        object.entry("waiting").or_insert_with(|| json!([]));
        let before = state.clone();
        // Turns that are gone hold nothing and wait for nothing. A turn is old by the last sign of life its holder
        // gave, not by when it began: a merge that waits for CI an hour is not a hung one (the audit, H04).
        let running = state["running"].clone();
        if !running.is_null() && (!process_alive(&running["pid"]) || now() - alive_at(&running) > self.limit_seconds) {
            state["running"] = Value::Null;
        }
        let waiting: Vec<Value> =
            state["waiting"].as_array().cloned().unwrap_or_default().into_iter().filter(|run| process_alive(&run["pid"])).collect();
        state["waiting"] = Value::Array(waiting);
        if self.slots > 1 {
            let also: Vec<Value> = state.get("also").and_then(Value::as_array).cloned().unwrap_or_default().into_iter()
                .filter(|run| process_alive(&run["pid"]) && now() - alive_at(run) <= self.limit_seconds).collect();
            state["also"] = Value::Array(also);
        }
        let result = change(&mut state);
        if state != before {
            let temporary = self.folder.join("state.json.tmp");
            std::fs::write(&temporary, dumps(&state))?;
            std::fs::rename(&temporary, &path)?;
        }
        Ok(result)
    }

    /// Takes the turn when it has come. Otherwise joins the line and says who is on and who stands ahead.
    pub fn take_turn(&self, pid: u32, tag: &str, what: &str) -> (bool, String) {
        let pid_value = json!(pid);
        self.locked(|state| {
            let mut line = state["waiting"].as_array().cloned().unwrap_or_default();
            if !line.iter().any(|run| run["pid"] == pid_value) {
                line.push(json!({"pid": pid, "tag": tag, "what": what, "since": now()}));
            }
            state["waiting"] = Value::Array(line.clone());
            let held = holders(state);
            if held.len() < self.slots && line[0]["pid"] == pid_value {
                let taken = json!({"pid": pid, "tag": tag, "what": what, "since": now()});
                if state["running"].is_null() {
                    state["running"] = taken;
                } else {
                    let mut also = state.get("also").and_then(Value::as_array).cloned().unwrap_or_default();
                    also.push(taken);
                    state["also"] = Value::Array(also);
                }
                line.remove(0);
                state["waiting"] = Value::Array(line);
                return (true, String::new());
            }
            let mut parts = Vec::new();
            for running in &held {
                let since = running["since"].as_f64().unwrap_or(0.0);
                parts.push(format!("Сейчас: {}, с {} ({}).", describe(running), clock(Some(since)), minutes(now() - since)));
            }
            let ahead: Vec<String> = line.iter().take_while(|run| run["pid"] != pid_value).map(describe).collect();
            if !ahead.is_empty() {
                parts.push(format!("Перед тобой: {}.", ahead.join(", ")));
            }
            (false, parts.join(" "))
        })
        .unwrap_or((false, String::new()))
    }

    pub fn leave(&self, pid: u32) {
        if !self.folder.join("state.json").exists() {
            return;
        }
        let pid_value = json!(pid);
        let _ = self.locked(|state| {
            if state["running"]["pid"] == pid_value {
                state["running"] = Value::Null;
            }
            let waiting: Vec<Value> = state["waiting"].as_array().cloned().unwrap_or_default().into_iter().filter(|run| run["pid"] != pid_value).collect();
            state["waiting"] = Value::Array(waiting);
            if self.slots > 1 {
                let also: Vec<Value> = state.get("also").and_then(Value::as_array).cloned().unwrap_or_default().into_iter().filter(|run| run["pid"] != pid_value).collect();
                state["also"] = Value::Array(also);
            }
        });
    }

    /// Blocks until the turn comes, saying why it waits and for whom.
    pub fn wait_turn(&self, pid: u32, tag: &str, what: &str, no_wait: bool, why: &str, busy: &str) -> i32 {
        let started = now();
        let mut last: Option<String> = None;
        let poll = number_var("CONVEYOR_POLL_SECONDS", 3.0).min(2.0);
        loop {
            let (taken, status) = self.take_turn(pid, tag, what);
            if taken {
                if last.is_some() {
                    say(&format!("==> Очередь дошла до тебя: ждал {}.", minutes(now() - started)));
                }
                return 0;
            }
            if no_wait {
                self.leave(pid);
                say(&format!("==> {busy} {status}"));
                return EXIT_BUSY;
            }
            if last.as_deref() != Some(status.as_str()) {
                if last.is_none() {
                    say(&format!("==> {why}"));
                }
                say(&format!("==> Жду своей очереди. {status}"));
                last = Some(status);
            }
            std::thread::sleep(Duration::from_secs_f64(poll));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn made_up(slots: usize) -> (Line, PathBuf) {
        static NUMBER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let number = NUMBER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let top = std::fs::canonicalize(std::env::temp_dir()).unwrap().join(format!("conveyor-line-{}-{number}", std::process::id()));
        let _ = std::fs::remove_dir_all(&top);
        (Line { folder: top.join("turns/conveyor-tests"), slots, limit_seconds: 1800.0 }, top)
    }

    fn sleeper() -> std::process::Child {
        std::process::Command::new("/bin/sleep").arg("60").spawn().unwrap()
    }

    #[test]
    fn runs_take_turns_in_the_order_they_came() {
        let (line, top) = made_up(1);
        let (mut first, mut second) = (sleeper(), sleeper());
        assert!(line.take_turn(first.id(), "Ввод", "r").0);
        let (taken, said) = line.take_turn(second.id(), "Анимация", "r");
        assert!(!taken);
        assert!(said.starts_with("Сейчас: Ввод · r, с "), "{said}");
        line.leave(first.id());
        assert!(line.take_turn(second.id(), "Анимация", "r").0, "the next goes as soon as one ends");
        let state: Value = serde_json::from_str(&std::fs::read_to_string(line.folder.join("state.json")).unwrap()).unwrap();
        assert_eq!(state["waiting"], json!([]));
        assert_eq!(state["running"]["tag"], "Анимация");
        let _ = (first.kill(), second.kill());
        let _ = std::fs::remove_dir_all(top);
    }

    #[test]
    fn a_turn_of_a_dead_process_holds_nothing() {
        let (line, top) = made_up(1);
        let mut gone = sleeper();
        assert!(line.take_turn(gone.id(), "Ушёл", "").0);
        let _ = gone.kill();
        let _ = gone.wait();
        let mut next = sleeper();
        assert!(line.take_turn(next.id(), "Следом", "").0);
        let _ = next.kill();
        let _ = std::fs::remove_dir_all(top);
    }

    #[test]
    fn two_slots_hold_two_runs_and_the_third_waits() {
        let (line, top) = made_up(2);
        let mut runs: Vec<_> = (0..3).map(|_| sleeper()).collect();
        assert!(line.take_turn(runs[0].id(), "a", "").0 && line.take_turn(runs[1].id(), "b", "").0);
        let (taken, said) = line.take_turn(runs[2].id(), "c", "");
        assert!(!taken && said.contains("Сейчас: a") && said.contains("Сейчас: b"), "{said}");
        runs.iter_mut().for_each(|run| {
            let _ = run.kill();
        });
        let _ = std::fs::remove_dir_all(top);
    }
}
