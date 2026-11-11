//! Serializes test runs that share this Mac.

use crate::base::{FileLock, list_mut, number, poll_seconds, process_alive, text, turns_root};
use hitl_host_core::process::{Options, run};
use hitl_host_core::{EXIT_BUSY, EXIT_INTERRUPTED, EXIT_OK, clock, minutes, now, read_json, say,
                    var, write_json};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const MAC_TESTS_FOLDER: &str = "mac-tests";
const CONVEYOR_TESTS_FOLDER: &str = "conveyor-tests";
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    /// Test runs of an iOS app on this Mac (Python's `MacTests`).
    MacTests,
    /// Full runs of `hitl test` (`ConveyorTests`).
    ConveyorTests,
}

fn number_setting(name: &str, default: f64) -> f64 {
    var(name).and_then(|text| text.trim().parse::<f64>().ok()).unwrap_or(default)
}

/// A run that holds the Mac this long has hung; the next one stops waiting for it.
pub fn mac_test_limit_seconds() -> f64 {
    number_setting("CONVEYOR_MAC_TEST_LIMIT_SECONDS", 1800.0)
}

// --------------------------------------------------------------------------
// Test runs on the Mac that no line knows of

/// The pids of `ps -axo pid=,comm=,args=` lines that are `xcodebuild test` runs for this Mac's iPad destination.
fn mac_test_pids(listing: &str) -> Vec<i64> {
    // Python's `line.split(None, 2)`: two words, then the rest of the line.
    fn split_three(line: &str) -> Option<(&str, &str, &str)> {
        let line = line.trim_start();
        let end = line.find(char::is_whitespace)?;
        let (first, rest) = (&line[..end], line[end..].trim_start());
        let end = rest.find(char::is_whitespace)?;
        let (second, third) = (&rest[..end], rest[end..].trim_start());
        if third.is_empty() { None } else { Some((first, second, third)) }
    }
    let mut found = Vec::new();
    for line in listing.lines() {
        let Some((pid, command, arguments)) = split_three(line) else { continue };
        // `\s(test|test-without-building)(\s|$)`: a word of its own after the program.
        let asks_for_test = arguments.split_whitespace().skip(1).any(|word| word == "test" || word == "test-without-building");
        if command.ends_with("xcodebuild") && arguments.contains("Designed for iPad") && asks_for_test {
            if let Ok(pid) = pid.parse::<i64>() {
                found.push(pid);
            }
        }
    }
    found
}

/// xcodebuild test runs for this Mac's iPad destination that are going on right now, whoever started them: one
/// started around the queue, or before the queue existed, kills and is killed like any other.
pub fn mac_test_processes() -> Vec<i64> {
    if var("CONVEYOR_MAC_TEST_SCAN").as_deref().unwrap_or("1") == "0" {
        return Vec::new();
    }
    let options = Options::new("queues.mac_test_processes").timeout(5.0);
    match run("ps", &["-axo", "pid=,comm=,args="], &options) {
        Ok(output) => mac_test_pids(&output.out()),
        Err(_) => Vec::new(),
    }
}

// --------------------------------------------------------------------------
// Notifications

#[cfg(test)]
pub static NOTES: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());

/// A macOS notification, through osascript (hitl::common::notify, which this crate cannot reach).
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_interrupt(_signal: libc::c_int) {
    INTERRUPTED.store(true, Ordering::SeqCst);
}

/// While this is alive a SIGINT (Ctrl-C) is not the end of the process but a flag the wait loop looks at, as
/// Python's KeyboardInterrupt was. A SIGINT that was ignored before (a job in the background) stays ignored.
pub(crate) struct Interrupts {
    previous: libc::sighandler_t,
}

impl Interrupts {
    pub(crate) fn catch() -> Interrupts {
        INTERRUPTED.store(false, Ordering::SeqCst);
        let previous = unsafe { libc::signal(libc::SIGINT, on_interrupt as extern "C" fn(libc::c_int) as libc::sighandler_t) };
        if previous == libc::SIG_IGN {
            unsafe { libc::signal(libc::SIGINT, libc::SIG_IGN) };
        }
        Interrupts { previous }
    }

    pub(crate) fn happened(&self) -> bool {
        INTERRUPTED.load(Ordering::SeqCst)
    }

    /// Sleeps `seconds`, or until a SIGINT; true when one came.
    pub(crate) fn sleep(&self, seconds: f64) -> bool {
        let end = std::time::Instant::now() + Duration::from_secs_f64(seconds.max(0.0));
        loop {
            if self.happened() {
                return true;
            }
            let left = end.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return false;
            }
            std::thread::sleep(left.min(Duration::from_millis(25)));
        }
    }
}

impl Drop for Interrupts {
    fn drop(&mut self) {
        unsafe { libc::signal(libc::SIGINT, self.previous) };
        INTERRUPTED.store(false, Ordering::SeqCst);
    }
}

// --------------------------------------------------------------------------
// The line

/// One line of turns, kept in `turns/<name>/state.json` under its lock.
pub struct Line {
    pub name: String,
    pub folder: PathBuf,
    /// A turn held longer than this without a sign of life has hung; the next one stops waiting for it.
    pub limit_seconds: f64,
    /// How many hold a turn at once. The second and later are kept in "also", which a line of one never writes, so
    /// the state of the lines that were there before reads as it did.
    pub slots: usize,
    /// What counts as a run going on outside the line (the Mac's line only): the pids that stand in its way.
    pub scan: Option<fn() -> Vec<i64>>,
    kind: Kind,
}

/// Test runs of an iOS app on this Mac ("Designed for iPad"), folder `mac-tests`. Every run installs the app first,
/// and the install ends every running copy with the same bundle identifier, so two runs at once kill each other's
/// test host.
pub fn mac_tests() -> Line {
    Line::new(MAC_TESTS_FOLDER, Kind::MacTests, mac_test_limit_seconds(), 1, Some(mac_test_processes))
}

/// Full runs of `hitl test`, folder `conveyor-tests`. A run starts every suite at once, some twenty processes:
/// the line takes the keeping to one at a time from the chats.
pub fn conveyor_tests() -> Line {
    let slots = var("CONVEYOR_TEST_SLOTS").and_then(|text| text.trim().parse::<i64>().ok()).unwrap_or(1).max(1) as usize;
    Line::new(CONVEYOR_TESTS_FOLDER, Kind::ConveyorTests, number_setting("CONVEYOR_TEST_LIMIT_SECONDS", 1800.0), slots, None)
}

fn pid_of(run: &Value) -> Option<i64> {
    run.get("pid").and_then(Value::as_i64)
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Object(map) => !map.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::String(text) => !text.is_empty(),
        Value::Number(number) => number.as_f64() != Some(0.0),
    }
}

/// When the holder of a turn was last seen at work: it began, or said so since.
fn alive_at(run: &Value) -> f64 {
    number(run, "since").max(number(run, "beat"))
}

/// The turns that are on: the running one, and the others of a line of several.
fn holders(state: &Value) -> Vec<Value> {
    let mut found = Vec::new();
    if let Some(running) = state.get("running").filter(|run| truthy(run)) {
        found.push(running.clone());
    }
    if let Some(also) = state.get("also").and_then(Value::as_array) {
        found.extend(also.iter().filter(|run| truthy(run)).cloned());
    }
    found
}

fn describe(run: &Value) -> String {
    let what = text(run, "what");
    let tag = text(run, "tag");
    format!("{}{}", if tag.is_empty() { "?" } else { tag }, if what.is_empty() { String::new() } else { format!(" · {what}") })
}

fn entry(pid: i64, tag: &str, what: &str) -> Value {
    json!({"pid": pid, "tag": tag, "what": what, "since": now()})
}

impl Line {
    fn new(name: &str, kind: Kind, limit_seconds: f64, slots: usize, scan: Option<fn() -> Vec<i64>>) -> Line {
        Line {
            name: name.to_string(),
            folder: turns_root().join(name),
            limit_seconds,
            slots,
            scan,
            kind,
        }
    }

    /// Why the turn cannot start although the line is free, if anything outside the line is in the way.
    fn others_outside(&self) -> String {
        let outside = self.scan.map(|scan| scan()).unwrap_or_default();
        if outside.is_empty() {
            return String::new();
        }
        let pids: Vec<String> = outside.iter().map(|pid| pid.to_string()).collect();
        format!("Сейчас идёт прогон, запущенный мимо очереди (xcodebuild {}); жду его конца.", pids.join(", "))
    }

    /// Read, change and write back as one step, under the line's file lock; turns that are gone are dropped first.
    pub fn locked<R>(&self, change: impl FnOnce(&mut Value) -> R) -> R {
        let _ = std::fs::create_dir_all(&self.folder);
        let path = self.folder.join("state.json");
        let _lock = FileLock::exclusive(&self.folder.join("state.lock")).ok();
        let mut state = read_json(&path).filter(Value::is_object).unwrap_or_else(|| json!({}));
        {
            let map = state.as_object_mut().expect("a state is an object");
            map.entry("running").or_insert(Value::Null);
            map.entry("waiting").or_insert_with(|| json!([]));
        }
        // Turns that are gone hold nothing and wait for nothing. A turn is old by the last sign of life its holder
        // gave, not by when it began.
        let running = state["running"].clone();
        if truthy(&running) && (!process_alive(pid_of(&running)) || now() - alive_at(&running) > self.limit_seconds) {
            state["running"] = Value::Null;
        }
        let waiting: Vec<Value> = state["waiting"].as_array().cloned().unwrap_or_default().into_iter()
            .filter(|run| process_alive(pid_of(run))).collect();
        state["waiting"] = Value::Array(waiting);
        if self.slots > 1 {
            let also: Vec<Value> = state.get("also").and_then(Value::as_array).cloned().unwrap_or_default().into_iter()
                .filter(|run| process_alive(pid_of(run)) && now() - alive_at(run) <= self.limit_seconds).collect();
            state["also"] = Value::Array(also);
        }
        // `before` is taken after the defaults, as Python does, so a line that was never written gets its file.
        let before = state.clone();
        let result = change(&mut state);
        if state != before {
            let _ = write_json(&path, &state);
        }
        result
    }

    /// Takes the turn when it has come; else joins the line and says who is on and who stands ahead.
    pub fn take_turn(&self, pid: i64, tag: &str, what: &str) -> (bool, String) {
        self.locked(|state| {
            let line = list_mut(state, "waiting");
            if !line.iter().any(|run| pid_of(run) == Some(pid)) {
                line.push(entry(pid, tag, what));
            }
            let first = line.first().and_then(pid_of);
            let holders_now = holders(state);
            if holders_now.len() < self.slots && first == Some(pid) {
                let outside = self.others_outside();
                if !outside.is_empty() {
                    return (false, outside);
                }
                let taken = entry(pid, tag, what);
                if state["running"].is_null() {
                    state["running"] = taken;
                } else {
                    list_mut(state, "also").push(taken);
                }
                list_mut(state, "waiting").remove(0);
                return (true, String::new());
            }
            let mut parts = Vec::new();
            for running in &holders_now {
                let since = number(running, "since");
                parts.push(format!("Сейчас: {}, с {} ({}).", describe(running), clock(Some(since)), minutes(now() - since)));
            }
            let mut ahead = Vec::new();
            for run in list_mut(state, "waiting").iter() {
                if pid_of(run) == Some(pid) {
                    break;
                }
                ahead.push(describe(run));
            }
            if !ahead.is_empty() {
                parts.push(format!("Перед тобой: {}.", ahead.join(", ")));
            }
            (false, parts.join(" "))
        })
    }

    /// The holder says it is at work: the turn is not taken for a hung one while it does.
    pub fn beat(&self, pid: i64) {
        if !self.folder.join("state.json").exists() {
            return;
        }
        self.locked(|state| {
            let beat = json!(now());
            if state["running"].is_object() && pid_of(&state["running"]) == Some(pid) {
                state["running"]["beat"] = beat.clone();
            }
            for run in list_mut(state, "also").iter_mut() {
                if pid_of(run) == Some(pid) {
                    run["beat"] = beat.clone();
                }
            }
        })
    }

    pub fn leave(&self, pid: i64) {
        if !self.folder.join("state.json").exists() {
            return;
        }
        self.locked(|state| {
            if state["running"].is_object() && pid_of(&state["running"]) == Some(pid) {
                state["running"] = Value::Null;
            }
            list_mut(state, "waiting").retain(|run| pid_of(run) != Some(pid));
            if self.slots > 1 {
                list_mut(state, "also").retain(|run| pid_of(run) != Some(pid));
            }
        })
    }

    /// The line as `hitl queue` shows it, headed by `title`; an empty title is the line's own.
    pub fn status_lines(&self, title: &str) -> Vec<String> {
        let own;
        let title = if title.is_empty() {
            own = match self.kind {
                Kind::MacTests => "Тесты на Маке (по одному прогону)".to_string(),
                Kind::ConveyorTests if self.slots == 1 => "Полные тесты hitl (по одному прогону)".to_string(),
                Kind::ConveyorTests => format!("Полные тесты hitl (по {} сразу)", self.slots),
            };
            own.as_str()
        } else {
            title
        };
        self.turn_lines(title)
    }

    fn turn_lines(&self, title: &str) -> Vec<String> {
        if !self.folder.join("state.json").exists() {
            return Vec::new();
        }
        let (on, line) = self.locked(|state| (holders(state), list_mut(state, "waiting").clone()));
        if on.is_empty() && line.is_empty() {
            return Vec::new();
        }
        let mut lines = vec![title.to_string()];
        for running in &on {
            let since = number(running, "since");
            lines.push(format!("  Идёт: {}, с {} ({})", describe(running), clock(Some(since)), minutes(now() - since)));
            if let Some(progress) = self.progress_of(running) {
                lines.push(format!("    {}", hitl_host_core::progress::line(&progress, &["тесты"])));
            }
        }
        for (index, run) in line.iter().enumerate() {
            lines.push(format!("  {}. ждёт {} с {}", index + 1, describe(run), clock(Some(number(run, "since")))));
        }
        lines
    }

    /// What the holder of a turn said of how far it is, if it is the holder.
    fn progress_of(&self, running: &Value) -> Option<hitl_host_core::progress::Progress> {
        hitl_host_core::progress::read(&hitl_host_core::progress::file(&self.folder), pid_of(running)?)
    }

    pub fn brief_lines(&self, label: &str) -> Vec<String> {
        let mut lines = Vec::new();
        let pad = " ".repeat(label.chars().count());
        let (on, waiting) = if self.folder.join("state.json").exists() {
            self.locked(|state| (holders(state), list_mut(state, "waiting").clone()))
        } else {
            (Vec::new(), Vec::new())
        };
        let name = |run: &Value, count| short(text(run, "tag"), count);
        for running in &on {
            let mut parts = vec![name(running, 22)];
            if let Some(progress) = self.progress_of(running) {
                // A bar is the tests; a step without one is said by its name (main, CI, GitHub).
                parts.push(if progress.total > 0 {
                    let failed = if progress.failed > 0 { format!(" ✗{}", progress.failed) } else { String::new() };
                    format!("{} {}/{}{failed}", hitl_host_core::progress::bar(progress.done, progress.total, 6), progress.done, progress.total)

                } else {
                    String::new()
                });
            }
            parts.retain(|part| !part.is_empty());
            parts.push(minutes(now() - number(running, "since")));
            let head = if lines.is_empty() { label } else { pad.as_str() };
            lines.push(format!("{head} ▶ {}", parts.join(" · ")));
        }
        if !waiting.is_empty() {
            let names: Vec<String> = waiting.iter().map(|run| name(run, 14)).collect();
            let head = if lines.is_empty() { label } else { pad.as_str() };
            lines.push(format!("{head} ждут: {}", names.join(" · ")));
        }
        lines
    }

    /// Blocks until the turn comes, saying why it waits and for whom; the exit code of the command.
    pub fn wait_turn(&self, pid: i64, tag: &str, what: &str, no_wait: bool, why: &str, busy: &str) -> i32 {
        let interrupts = Interrupts::catch();
        let started = now();
        let mut last: Option<String> = None;
        loop {
            if interrupts.happened() {
                self.leave(pid);
                return EXIT_INTERRUPTED;
            }
            let (taken, status) = self.take_turn(pid, tag, what);
            if taken {
                if last.is_some() {
                    say(&format!("==> Очередь дошла до тебя: ждал {}.", minutes(now() - started)));
                }
                return EXIT_OK;
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
            if interrupts.sleep(poll_seconds().min(2.0)) {
                self.leave(pid);
                return EXIT_INTERRUPTED;
            }
        }
    }

}

/// A text cut to `count` characters, with an ellipsis when it was longer.
fn short(text: &str, count: usize) -> String {
    if text.chars().count() <= count {
        return text.to_string();
    }
    format!("{}…", text.chars().take(count - 1).collect::<String>().trim_end())
}

pub fn brief_lines() -> Vec<String> {
    let mut lines = Vec::new();
    lines.extend(conveyor_tests().brief_lines("тесты"));
    lines.extend(mac_tests().brief_lines("мак  "));
    lines
}

/// The lines beside the phones' for `hitl queue`: tests on the Mac and full runs (other_lines).
pub fn other_lines() -> Vec<String> {
    let mut lines = mac_tests().status_lines("");
    let found = conveyor_tests().status_lines("");
    if !lines.is_empty() && !found.is_empty() {
        lines.push(String::new());
    }
    lines.extend(found);
    lines
}

// The commands of `hitl _queues`: mac-begin, mac-end. Arguments as `commands.rs` parsed them.

fn tag_of(tag: Option<&str>) -> String {
    let tag = tag.unwrap_or("").trim();
    if tag.is_empty() { "?".to_string() } else { tag.to_string() }
}

pub fn command_mac_begin(pid: i64, tag: Option<&str>, what: Option<&str>, no_wait: bool) -> i32 {
    mac_tests().wait_turn(
        pid, &tag_of(tag), what.unwrap_or(""), no_wait,
        "Тесты на Маке идут по одному: каждый прогон ставит приложение, а установка завершает все его \
         запущенные копии, и два прогона сразу роняют друг друга.",
        "Мак занят другим прогоном тестов, твой не запущен.")
}

pub fn command_mac_end(pid: i64) -> i32 {
    mac_tests().leave(pid);
    EXIT_OK
}

#[cfg(test)]
fn state_text(line: &Line) -> String {
    read_json(&line.folder.join("state.json")).map(|state| hitl_host_core::dumps(&state)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::testing::Sandbox;
    use std::process::{Child, Command};

    /// Processes that are alive for as long as the test needs them; killed, and waited for, at the end.
    struct Runs(Vec<Child>);

    impl Runs {
        fn new(count: usize) -> Runs {
            Runs((0..count).map(|_| Command::new("/bin/sleep").arg("60").spawn().expect("sleep starts")).collect())
        }

        fn pid(&self, index: usize) -> i64 {
            self.0[index].id() as i64
        }

        /// The run ends, and is reaped: a zombie still counts as a process.
        fn kill(&mut self, index: usize) {
            let _ = self.0[index].kill();
            let _ = self.0[index].wait();
        }
    }

    impl Drop for Runs {
        fn drop(&mut self) {
            for run in &mut self.0 {
                let _ = run.kill();
                let _ = run.wait();
            }
        }
    }

    fn shown(lines: &[String]) -> String {
        lines.join("\n")
    }

    // ---- TestsOnTheMac

    #[test]
    fn runs_take_turns_in_the_order_they_came() {
        let _sandbox = Sandbox::new("turns-order");
        let runs = Runs::new(3);
        let (first, second, third) = (runs.pid(0), runs.pid(1), runs.pid(2));
        let tests = mac_tests();
        assert_eq!(tests.take_turn(first, "r/38", "весь план"), (true, String::new()));
        let (taken, status) = tests.take_turn(second, "r/14", "DockTests");
        assert!(!taken);
        // ^Сейчас: r/38 · весь план, с \d\d:\d\d \(меньше минуты\)\.$
        let shape = status.strip_prefix("Сейчас: r/38 · весь план, с ").and_then(|rest| rest.strip_suffix(" (меньше минуты)."));
        let clock = shape.expect(&status);
        let digits: Vec<char> = clock.chars().collect();
        assert!(digits.len() == 5 && digits[2] == ':' && digits.iter().enumerate().all(|(at, letter)| at == 2 || letter.is_ascii_digit()), "{status}");
        let (taken, status) = tests.take_turn(third, "r/37", "");
        assert!(!taken);
        assert!(status.contains("Перед тобой: r/14 · DockTests."), "{status}");

        // The run that ends gives the Mac to the one that came first, not to whoever asks first.
        tests.leave(first);
        assert!(!tests.take_turn(third, "r/37", "").0);
        assert!(tests.take_turn(second, "r/14", "DockTests").0);
        tests.leave(second);
        assert!(tests.take_turn(third, "r/37", "").0);
        assert!(tests.status_lines("")[1].starts_with("  Идёт: r/37, с"));
    }

    #[test]
    fn a_run_that_died_or_hung_holds_nothing() {
        let _sandbox = Sandbox::new("turns-died");
        let mut runs = Runs::new(3);
        let (first, second, third) = (runs.pid(0), runs.pid(1), runs.pid(2));
        let tests = mac_tests();
        assert!(tests.take_turn(first, "r/38", "").0);
        assert!(!tests.take_turn(second, "r/14", "").0);
        assert!(!tests.take_turn(third, "r/37", "").0);
        // The holder and the one next in line are killed: the third is neither blocked nor behind a ghost.
        runs.kill(0);
        runs.kill(1);
        assert!(tests.take_turn(third, "r/37", "").0);
        tests.leave(third);
        // A run that has held the Mac longer than any run takes is given up on.
        let fourth = std::process::id() as i64;
        assert!(tests.take_turn(fourth, "r/40", "").0);
        let limit = mac_test_limit_seconds();
        tests.locked(|state| {
            let since = state["running"]["since"].as_f64().unwrap();
            state["running"]["since"] = json!(since - limit - 1.0);
        });
        assert!(tests.take_turn(third, "r/37", "").0);
    }

    #[test]
    fn a_run_started_around_the_queue_is_waited_for_too() {
        let _sandbox = Sandbox::new("turns-around");
        let runs = Runs::new(1);
        let first = runs.pid(0);
        let mut tests = mac_tests();
        tests.scan = Some(|| vec![4242]);
        let (taken, status) = tests.take_turn(first, "r/38", "");
        assert!(!taken);
        assert!(status.contains("запущенный мимо очереди (xcodebuild 4242)"), "{status}");
        tests.scan = Some(|| Vec::new());
        assert!(tests.take_turn(first, "r/38", "").0);
    }

    #[test]
    fn the_command_waits_or_says_who_holds_the_mac() {
        let _sandbox = Sandbox::new("turns-command");
        let runs = Runs::new(2);
        let (first, second) = (runs.pid(0), runs.pid(1));
        assert_eq!(command_mac_begin(first, Some("r/38"), Some("весь план"), false), 0,
                   "a run on a free Mac starts without a word");
        assert_eq!(command_mac_begin(second, Some("r/14"), None, true), EXIT_BUSY);
        // Python read "Мак занят другим прогоном тестов, твой не запущен. Сейчас: r/38 · весь план" off stderr.
        let out = shown(&other_lines());
        assert!(out.contains("Тесты на Маке (по одному прогону)\n  Идёт: r/38 · весь план, с"), "{out}");
        assert!(!out.contains("ждёт r/14"), "a run that would not wait is not left in the line: {out}");
        assert_eq!(command_mac_end(first), 0);
        assert_eq!(command_mac_begin(second, Some("r/14"), None, true), 0);
    }

    // ---- FullTestRuns

    #[test]
    fn one_runs_at_a_time_by_default() {
        let _sandbox = Sandbox::new("turns-full-one");
        let runs = Runs::new(2);
        let line = conveyor_tests();
        assert_eq!(line.slots, 1);
        assert!(line.take_turn(runs.pid(0), "Панель", "").0);
        assert!(!line.take_turn(runs.pid(1), "Меню", "").0);
        assert!(shown(&other_lines()).contains("Полные тесты hitl (по одному прогону)"));
    }

    #[test]
    fn two_run_at_once_and_the_third_waits_its_turn() {
        let _sandbox = Sandbox::new("turns-full-two");
        let mut runs = Runs::new(4);
        let (first, second, third, fourth) = (runs.pid(0), runs.pid(1), runs.pid(2), runs.pid(3));
        let mut line = conveyor_tests();
        line.slots = 2;
        assert_eq!(line.take_turn(first, "Панель", "1015-a"), (true, String::new()));
        assert_eq!(line.take_turn(second, "Меню", "1015-b"), (true, String::new()));
        let (taken, status) = line.take_turn(third, "Ввод", "1015-c");
        assert!(!taken);
        assert!(status.contains("Сейчас: Панель · 1015-a"), "{status}");
        assert!(status.contains("Сейчас: Меню · 1015-b"), "{status}");
        assert!(!line.take_turn(fourth, "Анимация", "").0);
        // The second holder ends first: the run that came first of those waiting goes, not whoever asks first.
        line.leave(second);
        assert!(!line.take_turn(fourth, "Анимация", "").0);
        assert!(line.take_turn(third, "Ввод", "1015-c").0);
        // A holder that died holds nothing.
        runs.kill(0);
        assert!(line.take_turn(fourth, "Анимация", "").0);
        let text = shown(&line.status_lines(""));
        assert!(text.contains("Полные тесты hitl (по 2 сразу)"), "{text}");
        assert!(text.contains("Идёт: Ввод · 1015-c"), "{text}");
        assert!(text.contains("Идёт: Анимация"), "{text}");
    }


    // ---- what the commands and the wait do besides

    #[test]
    fn a_run_in_the_listing_is_a_test_run_for_the_mac_only_when_it_says_so() {
        let listing = "\
  101 xcodebuild /usr/bin/xcodebuild -destination platform=macOS,variant=Designed for iPad test\n\
  102 xcodebuild /usr/bin/xcodebuild -destination platform=macOS,variant=Designed for iPad test-without-building -x\n\
  103 xcodebuild /usr/bin/xcodebuild -destination platform=macOS,variant=Designed for iPad build\n\
  104 xcodebuild /usr/bin/xcodebuild -destination id=SIM test\n\
  105 vim vim test Designed for iPad\n\
  106 xcodebuild /usr/bin/xcodebuild -destination platform=macOS,variant=Designed for iPad -testing\n\
  junk\n";
        assert_eq!(mac_test_pids(listing), vec![101, 102]);
    }

    #[test]
    fn other_lines_keep_a_blank_line_between_the_lines() {
        let _sandbox = Sandbox::new("turns-other");
        let runs = Runs::new(2);
        assert!(other_lines().is_empty());
        assert!(mac_tests().take_turn(runs.pid(0), "a", "").0);
        assert!(conveyor_tests().take_turn(runs.pid(1), "b", "").0);
        let lines = other_lines();
        assert_eq!(lines.iter().filter(|line| line.is_empty()).count(), 1, "{lines:?}");
        assert_eq!(lines[0], "Тесты на Маке (по одному прогону)");
    }

    /// SIGINT as the test finds it is whatever its parent handed down: a shell's `&` (a chat that runs the tests in the
    /// background) hands down "ignore", and the wait then rightly ignores the interrupt as well. The test sets what it
    /// stands on, and puts back what it found.
    struct InterruptAsDefault(libc::sighandler_t);

    impl InterruptAsDefault {
        fn set() -> InterruptAsDefault {
            InterruptAsDefault(unsafe { libc::signal(libc::SIGINT, libc::SIG_DFL) })
        }
    }

    impl Drop for InterruptAsDefault {
        fn drop(&mut self) {
            unsafe { libc::signal(libc::SIGINT, self.0) };
        }
    }

    #[test]
    fn a_waiting_run_that_is_interrupted_leaves_the_line_with_130() {
        let mut sandbox = Sandbox::new("turns-interrupt");
        let _default = InterruptAsDefault::set();
        sandbox.set("CONVEYOR_POLL_SECONDS", "0.05");
        let runs = Runs::new(2);
        let line = mac_tests();
        assert!(line.take_turn(runs.pid(0), "r/38", "").0);
        let waiting = runs.pid(1);
        let watching = mac_tests();
        let sender = std::thread::spawn(move || {
            // Only once the run stands in the line: it catches the interrupt before it asks for a turn, and an
            // interrupt that came earlier would end the whole test process.
            let began = std::time::Instant::now();
            while !shown(&watching.status_lines("")).contains("ждёт r/14") {
                assert!(began.elapsed() < Duration::from_secs(60), "the run never stood in the line");
                std::thread::sleep(Duration::from_millis(10));
            }
            unsafe { libc::raise(libc::SIGINT) };
        });
        assert_eq!(line.wait_turn(waiting, "r/14", "", false, "почему", "занято"), 130);
        sender.join().unwrap();
        let out = shown(&line.status_lines(""));
        assert!(!out.contains("ждёт r/14"), "{out}");
        assert!(out.contains("Идёт: r/38"), "{out}");
        // The previous handler is back: no flag is left over for the next wait.
        assert!(!INTERRUPTED.load(Ordering::SeqCst));
    }

    #[test]
    fn a_waiting_run_in_the_background_keeps_ignoring_the_interrupt() {
        let mut sandbox = Sandbox::new("turns-ignored");
        sandbox.set("CONVEYOR_POLL_SECONDS", "0.05");
        let ignored = InterruptAsDefault::set();
        unsafe { libc::signal(libc::SIGINT, libc::SIG_IGN) };
        let interrupts = Interrupts::catch();
        unsafe { libc::raise(libc::SIGINT) };
        assert!(!interrupts.sleep(0.1), "a job in the background does not hear Ctrl-C");
        drop(interrupts);
        assert_eq!(unsafe { libc::signal(libc::SIGINT, libc::SIG_IGN) }, libc::SIG_IGN, "and it is still ignored after");
        drop(ignored);
    }

    #[test]
    fn a_waiting_run_goes_when_its_turn_comes() {
        let mut sandbox = Sandbox::new("turns-wait");
        sandbox.set("CONVEYOR_POLL_SECONDS", "0.05");
        let runs = Runs::new(2);
        let (first, second) = (runs.pid(0), runs.pid(1));
        let line = mac_tests();
        assert!(line.take_turn(first, "r/38", "").0);
        let folder = line.folder.clone();
        let leaver = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            // The holder of the turn ends: the same as `mac-end`, from the other side of the lock.
            let line = Line::new(MAC_TESTS_FOLDER, Kind::MacTests, 1800.0, 1, None);
            assert_eq!(line.folder, folder);
            line.leave(first);
        });
        assert_eq!(line.wait_turn(second, "r/14", "", false, "почему", "занято"), 0);
        leaver.join().unwrap();
        assert_eq!(state_text(&line).contains(&format!("\"pid\": {second}")), true);
    }

    #[test]
    fn the_turn_of_a_missing_state_is_nothing_to_leave_or_beat() {
        let _sandbox = Sandbox::new("turns-missing");
        let line = mac_tests();
        line.leave(1);
        line.beat(1);
        assert!(line.status_lines("").is_empty());
        assert!(!line.folder.join("state.json").exists(), "a command that holds nothing writes nothing");
    }
}
