//! What the holder of a turn in a line is doing now: its step and how many of its tests have run, in
//! `turns/<line>/progress.json` beside the line's state. The holder writes it whole, without the line's lock; a reader
//! believes it only for the pid that holds the turn, so what a run that ended left behind is never shown.

use crate::{now, read_json, write_json};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// The setting a parent test run passes to its suites: the file the run counts its tests into.
pub const FILE_SETTING: &str = "CONVEYOR_TEST_PROGRESS";

/// The steps of a merge, in their order: how `hitl queue` shows where a merge has got to.

/// The progress file of a line's folder.
pub fn file(line_folder: &Path) -> PathBuf {
    line_folder.join("progress.json")
}

/// What is known of the holder of a turn.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Progress {
    pub pid: i64,
    pub step: String,
    pub done: u64,
    pub total: u64,
    pub failed: u64,
}

/// The progress of `pid` in `path`, or None when the file is of another pid or there is none.
pub fn read(path: &Path, pid: i64) -> Option<Progress> {
    let value = read_json(path)?;
    let number = |key: &str| value.get(key).and_then(Value::as_u64).unwrap_or(0);
    let found = Progress {
        pid: value.get("pid").and_then(Value::as_i64).unwrap_or(0),
        step: value.get("step").and_then(Value::as_str).unwrap_or("").to_string(),
        done: number("done"),
        total: number("total"),
        failed: number("failed"),
    };
    (found.pid == pid).then_some(found)
}

/// Says the step `pid` is at; the count of tests starts again with a new step.
pub fn step(path: &Path, pid: i64, step: &str) {
    let _ = write_json(path, &json!({"pid": pid, "step": step, "done": 0, "total": 0, "failed": 0, "at": now()}));
}

/// Says how many tests have run of how many; the pid and the step stay what the holder of the turn wrote. A file of
/// nobody (a test run that is not a merge's, started by hand with the setting) is taken by `pid`.
pub fn count(path: &Path, pid: i64, done: u64, total: u64, failed: u64) {
    let mut value = read_json(path).filter(Value::is_object).unwrap_or_else(|| json!({"pid": pid, "step": "тесты"}));
    if let Some(map) = value.as_object_mut() {
        map.insert("done".into(), json!(done));
        map.insert("total".into(), json!(total));
        map.insert("failed".into(), json!(failed));
        map.insert("at".into(), json!(now()));
    }
    let _ = write_json(path, &value);
}

/// A bar of `width` cells for `done` of `total`.
pub fn bar(done: u64, total: u64, width: usize) -> String {
    let filled = if total == 0 { 0 } else { ((done.min(total) as f64 / total as f64) * width as f64).round() as usize };
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

/// The progress in one line: the steps passed, the one at work with its count, the ones ahead.
pub fn line(progress: &Progress, steps: &[&str]) -> String {
    let counted = if progress.total > 0 {
        let failed = if progress.failed > 0 { format!(", упало {}", progress.failed) } else { String::new() };
        format!(" {} {}/{}{failed}", bar(progress.done, progress.total, 12), progress.done, progress.total)
    } else {
        String::new()
    };
    let Some(at) = steps.iter().position(|step| *step == progress.step) else {
        return format!("{}{counted}", progress.step);
    };
    let mut parts: Vec<String> = steps[..at].iter().map(|step| format!("{step} ✓")).collect();
    parts.push(format!("{}{counted}", progress.step));
    parts.extend(steps[at + 1..].iter().map(|step| step.to_string()));
    parts.join(" → ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_and_a_count_are_read_back_for_the_holder_only() {
        let folder = std::env::temp_dir().join(format!("conveyor-progress-{}", std::process::id()));
        let path = file(&folder);
        step(&path, 7, "тесты");
        count(&path, 7, 3, 10, 1);
        assert_eq!(read(&path, 7), Some(Progress { pid: 7, step: "тесты".into(), done: 3, total: 10, failed: 1 }));
        assert_eq!(read(&path, 8), None, "a run that does not hold the turn is not shown");
        step(&path, 7, "CI");
        assert_eq!(read(&path, 7).map(|found| found.total), Some(0), "a new step counts again");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_line_shows_the_steps_passed_the_one_at_work_and_the_ones_ahead() {
        let progress = Progress { pid: 1, step: "тесты".into(), done: 6, total: 12, failed: 0 };
        assert_eq!(line(&progress, &["build", "тесты", "verify"]), "build ✓ → тесты ██████░░░░░░ 6/12 → verify");
        let building = Progress { step: "сборка".into(), ..Progress::default() };
        assert_eq!(line(&building, &["build", "тесты", "verify"]), "сборка");
        assert_eq!(bar(0, 0, 4), "░░░░");
    }
}
