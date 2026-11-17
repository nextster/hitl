//! The device, through devicectl (queues.py: `Device`): what the queue needs from the phone.
//!
//! `Device` is the trait the dispatcher is written against; `Devicectl` is the real one, which runs
//! `xcrun devicectl`. Tests replace the way a program is run and started (`Devicectl::with_runners`) or the
//! program itself (`Devicectl::with_program`), and never talk to a real phone.

use hitl_host_core::process::{Options, Output, run};
use hitl_host_core::{dumps_line, read_json};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// Where an app that understands --ios-run-checklist keeps the marks, inside its data container.
pub const REPORT_PATH: &str = "Library/Application Support/ios-run/check.json";
/// Where the app reads what was marked in the window on the Mac, beside its own file.
pub const HOST_PATH: &str = "Library/Application Support/ios-run/host.json";
/// What the app puts before its report when it says it on its output, a line at a time.
pub const REPORT_LINE: &str = "\x1eios-run-check ";
/// How long an attached launch is waited for to say that the app is up.
pub const LAUNCH_SECONDS: f64 = 20.0;

/// What is handed every report the app says on its output.
pub type Heard = Arc<dyn Fn(Value) + Send + Sync>;

/// What the queue needs from the phone. Tests replace it.
pub trait Device: Send + Sync {
    /// "unlocked" | "locked" | "unreachable" | "unknown"
    fn lock_state(&self) -> String;
    fn install(&self, app: &str) -> (bool, String);
    /// With `heard`, the launch stays attached to the app's output while the app runs, and every report line
    /// (REPORT_LINE prefix, JSON object) is handed to `heard` the moment it is said.
    fn launch(&self, bundle_id: &str, arguments: &[String], url: Option<&str>, heard: Option<Heard>, environment: &crate::private_launch_env::LaunchEnvironment) -> (bool, String);
    /// Whether a launch is still attached to the app's output.
    fn hears(&self) -> bool;
    /// Lets go of the app's output.
    fn detach(&self);
    /// The app's marks file, or None while it is absent or unreadable.
    fn read_report(&self, bundle_id: &str) -> Option<Value>;
    /// Puts what the Mac says beside the app's marks file; false when the phone did not take it.
    fn write_host(&self, bundle_id: &str, said: &Value) -> bool;
}

/// Runs a whole command (the program first) with the limit in seconds, and waits for it.
pub type Runner = Box<dyn Fn(&[String], f64, &crate::private_launch_env::LaunchEnvironment) -> std::io::Result<Output> + Send + Sync>;

/// A program started and left running, with its output (the program's stdout and stderr in one stream).
pub struct Spawned {
    pub child: Child,
    pub output: Box<dyn Read + Send>,
}

/// Starts a whole command (the program first) in a session of its own and leaves it running.
pub type Spawner = Box<dyn Fn(&[String], &crate::private_launch_env::LaunchEnvironment) -> std::io::Result<Spawned> + Send + Sync>;

pub struct Devicectl {
    udid: String,
    run: Runner,
    spawn: Spawner,
    launch_seconds: f64,
    /// The launch that stays attached to the app's output, while there is one.
    attached: Mutex<Option<Child>>,
}

impl Devicectl {
    pub fn new(udid: &str) -> Devicectl {
        Devicectl::with_program(udid, "xcrun")
    }

    /// The same with another program in place of `xcrun` (a made-up one, in tests).
    pub fn with_program(udid: &str, program: &str) -> Devicectl {
        let (for_run, for_spawn) = (program.to_string(), program.to_string());
        Devicectl::with_runners(
            udid,
            Box::new(move |command, limit, environment| run_command(&for_run, command, limit, environment)),
            Box::new(move |command, environment| spawn_command(&for_spawn, command, environment)),
        )
    }

    /// Both ways of starting a program replaced.
    pub fn with_runners(udid: &str, run: Runner, spawn: Spawner) -> Devicectl {
        Devicectl { udid: udid.to_string(), run, spawn, launch_seconds: LAUNCH_SECONDS, attached: Mutex::new(None) }
    }

    /// How long an attached launch is waited for (a short one, in tests).
    pub fn with_launch_seconds(mut self, seconds: f64) -> Devicectl {
        self.launch_seconds = seconds;
        self
    }

    /// Runs one devicectl command. Its own options come first: whatever follows the bundle identifier of a launch
    /// goes to the app.
    fn devicectl(&self, options: &[&str], positional: &[&str], timeout: f64) -> (bool, Value, String) {
        self.devicectl_with_environment(options, positional, timeout, &Default::default())
    }

    fn devicectl_with_environment(&self, options: &[&str], positional: &[&str], timeout: f64, environment: &crate::private_launch_env::LaunchEnvironment) -> (bool, Value, String) {
        let folder = TempFolder::new("ios-run-device.");
        let output = folder.path().join("out.json");
        let mut command: Vec<String> = ["xcrun", "devicectl"].iter().map(|word| word.to_string()).collect();
        command.extend(options.iter().map(|word| word.to_string()));
        command.extend(["--timeout".to_string(), (timeout as i64).to_string(), "--json-output".to_string(),
                        output.to_string_lossy().to_string()]);
        command.extend(positional.iter().map(|word| word.to_string()));
        let done = match (self.run)(&command, timeout + 30.0, environment) {
            Ok(done) => done,
            Err(error) => return (false, json!({}), environment.redact(&error.to_string())),
        };
        if done.timed_out {
            return (false, json!({}), format!("devicectl timed out after {} seconds", timeout + 30.0));
        }
        let payload = read_json(&output).filter(Value::is_object).map(|v| environment.redact_value(&v)).unwrap_or_else(|| json!({}));
        let outcome = payload.pointer("/info/outcome").and_then(Value::as_str).unwrap_or("success");
        if done.code == Some(0) && outcome == "success" {
            return (true, payload, String::new());
        }
        let described = payload.pointer("/error/userInfo/NSLocalizedDescription/string").and_then(Value::as_str);
        let message = match described.filter(|text| !text.is_empty()) {
            Some(text) => text.to_string(),
            None => {
                let (err, out) = (done.err(), done.out());
                let text = if !err.is_empty() { err } else { out };
                match text.trim().lines().last() {
                    Some(line) => line.to_string(),
                    None => format!("devicectl exited with {}", exit_word(&done)),
                }
            }
        };
        (false, payload, if environment.is_empty() { message } else { "Private device launch failed; tool output suppressed.".to_string() })
    }
}

/// The exit status as Python says it: the code, or minus the signal.
fn exit_word(done: &Output) -> i64 {
    match (done.code, done.signal) {
        (Some(code), _) => code as i64,
        (None, Some(signal)) => -(signal as i64),
        _ => -1,
    }
}

fn run_command(program: &str, command: &[String], limit: f64, environment: &crate::private_launch_env::LaunchEnvironment) -> std::io::Result<Output> {
    let mut options = Options::new("queues.devicectl").timeout(limit);
    for (name, _) in std::env::vars_os().filter(|(n,_)| n.to_string_lossy().starts_with("DEVICECTL_CHILD_")) {
        options = options.env_remove(&name.to_string_lossy());
    }
    for (name, value) in environment.variables() { options = options.env(&format!("DEVICECTL_CHILD_{name}"), value); }
    run(program.to_string(), &command[1..], &options)
}

/// A launch that stays: in a session of its own, so that a signal that reaches this process does not reach the
/// launch, which passes every signal it catches on to the app. Its stdout and stderr are one stream.
fn spawn_command(program: &str, command: &[String], environment: &crate::private_launch_env::LaunchEnvironment) -> std::io::Result<Spawned> {
    let mut ends = [0i32; 2];
    if unsafe { libc::pipe(ends.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let (read, write) = unsafe { (OwnedFd::from_raw_fd(ends[0]), OwnedFd::from_raw_fd(ends[1])) };
    for end in [&read, &write] {
        unsafe { libc::fcntl(end.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) };
    }
    let mut process = Command::new(program);
    crate::private_launch_env::configure_child(&mut process, environment);
    process.args(&command[1..]).stdin(Stdio::null()).stdout(Stdio::from(write.try_clone()?)).stderr(Stdio::from(write));
    unsafe {
        process.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = process.spawn()?;
    drop(process);
    let purpose = Options::new("queues.devicectl.launch").purpose;
    if hitl_host_core::journal::enabled() {
        hitl_host_core::journal::emit(&json!({
            "kind": "native.process.launch", "source": "native.Conveyor", "phase": "event", "outcome": "spawned",
            "context": {"purpose": purpose, "executable": hitl_host_core::journal::reference(&program.to_string()),
                        "launcher_pid": std::process::id(), "launcher_ppid": unsafe { libc::getppid() },
                        "child_pid": child.id()},
        }));
    }
    Ok(Spawned { child, output: Box::new(std::fs::File::from(read)) })
}

/// A folder in the temporary place that is removed when this value is dropped.
struct TempFolder(PathBuf);

impl TempFolder {
    fn new(prefix: &str) -> TempFolder {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let path = std::env::temp_dir().join(format!("{prefix}{}.{nanos}.{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::create_dir_all(&path);
        TempFolder(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// What the reader of an attached launch's output tells the one that waits for it.
#[derive(Default)]
struct Watch {
    confirmed: bool,
    launched: bool,
    ended: bool,
    said: Vec<String>,
}

impl Device for Devicectl {
    /// Only a known lock state or a successful discovery absence is actionable. Timeouts, tool failures and
    /// malformed replies are unknown, not evidence that the reviewer needs to connect or unlock his phone.
    fn lock_state(&self) -> String {
        let (succeeded, payload, _) = self.devicectl(&["device", "info", "lockState", "--device", &self.udid], &[], 15.0);
        if let (true, Some(required)) = (succeeded, payload.pointer("/result/passcodeRequired").and_then(Value::as_bool)) {
            return if required { "locked" } else { "unlocked" }.to_string();
        }
        // `list devices` includes cached devices. Use its documented State predicate; an unfiltered list cannot
        // establish current visibility.
        if self.udid.is_empty() || !self.udid.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return "unknown".to_string();
        }
        let predicate = format!("(Identifier = '{0}' OR coreDeviceID = '{0}') AND State = 'available'", self.udid);
        let (discovered, listing, _) = self.devicectl(&["list", "devices", "--filter", &predicate], &[], 15.0);
        let empty = listing.pointer("/result/devices").and_then(Value::as_array).is_some_and(|devices| devices.is_empty());
        if discovered && empty { "unreachable" } else { "unknown" }.to_string()
    }

    fn install(&self, app: &str) -> (bool, String) {
        let (succeeded, _, message) = self.devicectl(&["device", "install", "app", "--device", &self.udid], &[app], 240.0);
        (succeeded, message)
    }

    fn launch(&self, bundle_id: &str, arguments: &[String], url: Option<&str>, heard: Option<Heard>, environment: &crate::private_launch_env::LaunchEnvironment) -> (bool, String) {
        let mut options = vec!["--device".to_string(), self.udid.clone(), "--terminate-existing".to_string()];
        if let Some(url) = url.filter(|url| !url.is_empty()) {
            options.extend(["--payload-url".to_string(), url.to_string()]);
        }
        // The launch ends the app's running copy, and with it whatever was attached to that copy.
        self.detach();
        let Some(heard) = heard else {
            let mut words = vec!["device", "process", "launch"];
            words.extend(options.iter().map(String::as_str));
            let mut positional = vec![bundle_id];
            positional.extend(arguments.iter().map(String::as_str));
            let (succeeded, _, message) = self.devicectl_with_environment(&words, &positional, 60.0, environment);
            return (succeeded, message);
        };
        let mut command: Vec<String> = ["xcrun", "devicectl", "device", "process", "launch"].iter().map(|w| w.to_string()).collect();
        command.extend(options);
        command.push("--console".to_string());
        command.push(bundle_id.to_string());
        command.extend(arguments.iter().cloned());
        let Spawned { mut child, output } = match (self.spawn)(&command, environment) {
            Ok(spawned) => spawned,
            Err(error) => return (false, environment.redact(&error.to_string())),
        };
        let watch = Arc::new((Mutex::new(Watch::default()), Condvar::new()));
        let reader = Arc::clone(&watch);
        let environment = environment.clone();
        std::thread::spawn(move || {
            let (state, signal) = &*reader;
            let mut lines = BufReader::new(output);
            let mut raw = Vec::new();
            loop {
                raw.clear();
                match lines.read_until(b'\n', &mut raw) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let text = String::from_utf8_lossy(&raw).trim_end_matches(['\r', '\n']).to_string();
                if let Some(at) = text.find(REPORT_LINE) {
                    {
                        state.lock().unwrap().launched = true;
                        signal.notify_all();
                    }
                    if let Ok(report @ Value::Object(_)) = serde_json::from_str::<Value>(&text[at + REPORT_LINE.len()..]) {
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| heard(environment.redact_value(&report))));
                    }
                } else {
                    let mut now = state.lock().unwrap();
                    if !now.launched {
                        now.said.push(if environment.is_empty() { text.clone() } else { "Private device launch output suppressed.".to_string() });
                        if text.starts_with("Launched application") {
                            now.launched = true;
                            now.confirmed = true;
                            signal.notify_all();
                        }
                    }
                }
            }
            let mut now = state.lock().unwrap();
            now.launched = true;
            now.ended = true;
            signal.notify_all();
        });
        // A launch that says nothing of itself and is still there after a while is taken as made: what it prints
        // is the tool's wording, not a promise.
        let (state, signal) = &*watch;
        let mut now = state.lock().unwrap();
        let limit = Instant::now() + Duration::from_secs_f64(self.launch_seconds.max(0.0));
        while !now.launched {
            let left = limit.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            now = signal.wait_timeout(now, left).unwrap().0;
        }
        // The output is over: the launch is gone, or about to be reaped.
        let over = now.ended;
        drop(now);
        let mut status = child.try_wait().ok().flatten();
        if over && status.is_none() {
            let until = Instant::now() + Duration::from_secs(5);
            while status.is_none() && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(5));
                status = child.try_wait().ok().flatten();
            }
        }
        let Some(status) = status else {
            *self.attached.lock().unwrap() = Some(child);
            return (true, String::new());
        };
        let said = state.lock().unwrap().said.clone();
        if status.success() || state.lock().unwrap().confirmed {
            return (true, String::new()); // launched, and the app is gone already: nothing to stay attached to
        }
        match said.iter().rev().find(|line| !line.trim().is_empty()) {
            Some(line) => (false, line.clone()),
            None => {
                use std::os::unix::process::ExitStatusExt;
                let code = status.code().map(i64::from).or(status.signal().map(|s| -(s as i64))).unwrap_or(-1);
                (false, format!("devicectl exited with {code}"))
            }
        }
    }

    fn hears(&self) -> bool {
        match self.attached.lock().unwrap().as_mut() {
            Some(child) => matches!(child.try_wait(), Ok(None)),
            None => false,
        }
    }

    /// Lets go of the app's output. The launch is killed outright: a signal it could catch it would pass on to the
    /// app, and the app is to stay as it is.
    fn detach(&self) {
        let taken = self.attached.lock().unwrap().take();
        let Some(mut child) = taken else { return };
        if matches!(child.try_wait(), Ok(None)) {
            unsafe { libc::killpg(child.id() as i32, libc::SIGKILL) };
            let until = Instant::now() + Duration::from_secs(2);
            while matches!(child.try_wait(), Ok(None)) && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    fn read_report(&self, bundle_id: &str) -> Option<Value> {
        let folder = TempFolder::new("ios-run-report.");
        let destination = folder.path().join("check.json");
        let (succeeded, _, _) = self.devicectl(
            &["device", "copy", "from", "--device", &self.udid, "--domain-type", "appDataContainer",
              "--domain-identifier", bundle_id, "--source", REPORT_PATH, "--destination", &destination.to_string_lossy()],
            &[], 15.0);
        if !succeeded {
            return None;
        }
        read_json(&destination).filter(Value::is_object)
    }

    fn write_host(&self, bundle_id: &str, said: &Value) -> bool {
        let folder = TempFolder::new("conveyor-host.");
        let source = folder.path().join("host.json");
        if std::fs::write(&source, dumps_line(said)).is_err() {
            return false;
        }
        let (succeeded, _, _) = self.devicectl(
            &["device", "copy", "to", "--device", &self.udid, "--domain-type", "appDataContainer",
              "--domain-identifier", bundle_id, "--source", &source.to_string_lossy(), "--destination", HOST_PATH],
            &[], 15.0);
        succeeded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::os::unix::fs::PermissionsExt;

    const UDID: &str = "00008110-0000000000000001";

    fn ok(code: i32) -> Output {
        Output { code: Some(code), ..Output::default() }
    }

    fn output_path(command: &[String]) -> PathBuf {
        let at = command.iter().position(|word| word == "--json-output").unwrap();
        PathBuf::from(&command[at + 1])
    }

    /// A device whose runs are answered one after another: (exit code, the JSON devicectl writes, if any).
    fn scripted(replies: Vec<(i32, Option<Value>)>) -> (Devicectl, Arc<Mutex<Vec<Vec<String>>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let (seen, queue) = (Arc::clone(&calls), Mutex::new(VecDeque::from(replies)));
        let run: Runner = Box::new(move |command, _, _| {
            seen.lock().unwrap().push(command.to_vec());
            let (code, payload) = queue.lock().unwrap().pop_front().expect("one reply a call");
            if let Some(payload) = payload {
                std::fs::write(output_path(command), payload.to_string()).unwrap();
            }
            Ok(ok(code))
        });
        let spawn: Spawner = Box::new(|_, _| Err(std::io::Error::other("no launch here")));
        (Devicectl::with_runners("UDID", run, spawn), calls)
    }

    #[test]
    fn lock_state_distinguishes_confirmed_absence_from_tool_errors() {
        let failed = || (1, Some(json!({"info": {"outcome": "failed"}})));
        let reply = |value: Value| (0, Some(value));
        let cases: Vec<(Vec<(i32, Option<Value>)>, &str)> = vec![
            (vec![reply(json!({"result": {"passcodeRequired": true}}))], "locked"),
            (vec![reply(json!({"result": {"passcodeRequired": false}}))], "unlocked"),
            (vec![failed(), reply(json!({"result": {"devices": []}}))], "unreachable"),
            (vec![failed(), failed()], "unknown"),
            (vec![failed(), reply(json!({"result": {"devices": [{"identifier": "UDID"}]}}))], "unknown"),
            (vec![reply(json!({})), reply(json!({}))], "unknown"),
            (vec![reply(json!({"result": []})), reply(json!({"result": []}))], "unknown"),
        ];
        for (replies, expected) in cases {
            let two = replies.len() == 2;
            let (device, calls) = scripted(replies);
            assert_eq!(device.lock_state(), expected);
            if two {
                let options = calls.lock().unwrap()[1].clone();
                assert!(options.contains(&"--filter".to_string()));
                assert!(options.last().is_none() || options.iter().any(|word| word.contains("State = 'available'")));
            }
        }
    }

    #[test]
    fn an_odd_udid_is_not_put_into_a_filter() {
        let run: Runner = Box::new(|_, _, _| Ok(ok(1)));
        let spawn: Spawner = Box::new(|_, _| Err(std::io::Error::other("no")));
        let device = Devicectl::with_runners("x' OR 1=1 --", run, spawn);
        assert_eq!(device.lock_state(), "unknown");
    }

    #[test]
    fn devicectl_options_come_before_the_app_arguments() {
        let (device, calls) = scripted(vec![(0, None)]);
        let arguments: Vec<String> = ["--ios-run-tag", "r/38", "--flag"].iter().map(|w| w.to_string()).collect();
        assert_eq!(device.launch("dev.example.demo", &arguments, Some("demo://x"), None, &Default::default()), (true, String::new()));
        let command = calls.lock().unwrap()[0].clone();
        let bundle = command.iter().position(|word| word == "dev.example.demo").unwrap();
        assert_eq!(command[bundle + 1..], ["--ios-run-tag", "r/38", "--flag"]);
        for option in ["--terminate-existing", "--payload-url", "--timeout", "--json-output"] {
            assert!(command.iter().position(|word| word == option).unwrap() < bundle, "{option}");
        }
    }

    #[test]
    fn a_failure_says_why() {
        let (device, _) = scripted(vec![(1, Some(json!({"error": {"userInfo": {"NSLocalizedDescription": {"string": "no tunnel"}}}})))]);
        assert_eq!(device.install("/tmp/App.app"), (false, "no tunnel".to_string()));
        let run: Runner = Box::new(|_, _, _| Ok(Output { code: Some(2), stderr: b"first\nlast line\n".to_vec(), ..Output::default() }));
        let spawn: Spawner = Box::new(|_, _| Err(std::io::Error::other("no")));
        assert_eq!(Devicectl::with_runners("UDID", run, spawn).install("a"), (false, "last line".to_string()));
        let run: Runner = Box::new(|_, _, _| Ok(ok(7)));
        let spawn: Spawner = Box::new(|_, _| Err(std::io::Error::other("no")));
        assert_eq!(Devicectl::with_runners("UDID", run, spawn).install("a"), (false, "devicectl exited with 7".to_string()));
        // An outcome other than success fails whatever the exit code says.
        let (device, _) = scripted(vec![(0, Some(json!({"info": {"outcome": "failed"}})))]);
        assert!(!device.install("a").0);
    }

    #[test]
    fn a_program_that_cannot_be_started_or_times_out_is_a_failure() {
        let run: Runner = Box::new(|_, _, _| Err(std::io::Error::other("cannot start")));
        let spawn: Spawner = Box::new(|_, _| Err(std::io::Error::other("no")));
        assert_eq!(Devicectl::with_runners("UDID", run, spawn).install("a"), (false, "cannot start".to_string()));
        let run: Runner = Box::new(|_, _, _| Ok(Output { timed_out: true, ..Output::default() }));
        let spawn: Spawner = Box::new(|_, _| Err(std::io::Error::other("no")));
        let (done, message) = Devicectl::with_runners("UDID", run, spawn).install("a");
        assert!(!done && message.contains("timed out"));
    }

    #[test]
    fn the_marks_file_is_read_and_the_hosts_word_is_written() {
        let copied = Arc::new(Mutex::new(String::new()));
        let seen = Arc::clone(&copied);
        let run: Runner = Box::new(move |command, _, _| {
            let at = |option: &str| command.iter().position(|word| word == option).map(|i| command[i + 1].clone());
            if command.contains(&"from".to_string()) {
                std::fs::write(at("--destination").unwrap(), "{\"id\": \"t1\"}").unwrap();
                assert_eq!(at("--source").unwrap(), REPORT_PATH);
            } else {
                assert_eq!(at("--destination").unwrap(), HOST_PATH);
                *seen.lock().unwrap() = std::fs::read_to_string(at("--source").unwrap()).unwrap();
            }
            Ok(ok(0))
        });
        let spawn: Spawner = Box::new(|_, _| Err(std::io::Error::other("no")));
        let device = Devicectl::with_runners("UDID", run, spawn);
        assert_eq!(device.read_report("dev.example.demo"), Some(json!({"id": "t1"})));
        assert!(device.write_host("dev.example.demo", &json!({"current": "ж", "n": 2})));
        assert_eq!(*copied.lock().unwrap(), "{\"current\": \"ж\", \"n\": 2}");
    }

    #[test]
    fn a_report_that_is_not_an_object_is_none() {
        let run: Runner = Box::new(|command, _, _| {
            let at = command.iter().position(|word| word == "--destination").unwrap();
            std::fs::write(&command[at + 1], "[1]").unwrap();
            Ok(ok(0))
        });
        let spawn: Spawner = Box::new(|_, _| Err(std::io::Error::other("no")));
        assert_eq!(Devicectl::with_runners("UDID", run, spawn).read_report("b"), None);
        let (device, _) = scripted(vec![(1, None)]);
        assert_eq!(device.read_report("b"), None);
    }

    // ---- The launch that stays attached, with a made-up xcrun in its place.

    /// A folder with an `xcrun` that writes its arguments into `command.txt` and then runs `body`.
    struct Fake {
        folder: PathBuf,
        device: Devicectl,
    }

    impl Fake {
        fn new(name: &str, body: &str, launch_seconds: f64) -> Fake {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let folder = std::env::temp_dir().join(format!("device-{name}-{}-{nanos}", std::process::id()));
            std::fs::create_dir_all(&folder).unwrap();
            let program = folder.join("xcrun");
            std::fs::write(&program, format!("#!/bin/sh\necho \"$@\" > '{}/command.txt'\n{body}\n", folder.display())).unwrap();
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
            let device = Devicectl::with_program(UDID, program.to_str().unwrap()).with_launch_seconds(launch_seconds);
            Fake { folder, device }
        }

        fn command(&self) -> String {
            std::fs::read_to_string(self.folder.join("command.txt")).unwrap().trim().to_string()
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            self.device.detach();
            let _ = std::fs::remove_dir_all(&self.folder);
        }
    }

    fn collector() -> (Heard, Arc<Mutex<Vec<Value>>>) {
        let heard = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&heard);
        (Arc::new(move |report| seen.lock().unwrap().push(report)), heard)
    }

    fn nothing() -> Heard {
        Arc::new(|_| {})
    }

    #[test]
    fn the_apps_lines_are_heard_and_the_launch_is_let_go_of_by_force() {
        let fake = Fake::new("heard", "\
echo '12:00:00  Acquired tunnel connection to device.'
echo 'Launched application with dev.example.demo bundle identifier.'
echo 'Waiting for the application to terminate…'
echo 'the app prints what it likes'
printf '\\036ios-run-check {\"id\": \"t1\", \"current\": \"2\"}\\n'
printf '\\036ios-run-check not json\\n'
printf '\\036ios-run-check {\"id\": \"t1\", \"current\": \"3\"}\\n'
exec sleep 60", 20.0);
        let (heard, reports) = collector();
        let began = Instant::now();
        let arguments = vec!["--ios-run-tag".to_string(), "r/38".to_string()];
        assert_eq!(fake.device.launch("dev.example.demo", &arguments, None, Some(heard), &Default::default()), (true, String::new()));
        assert!(began.elapsed() < Duration::from_secs(10));
        assert_eq!(fake.command(), format!("devicectl device process launch --device {UDID} --terminate-existing --console dev.example.demo --ios-run-tag r/38"));
        let until = Instant::now() + Duration::from_secs(5);
        while Instant::now() < until && reports.lock().unwrap().len() < 2 {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(*reports.lock().unwrap(), vec![json!({"id": "t1", "current": "2"}), json!({"id": "t1", "current": "3"})]);
        assert!(fake.device.hears());
        let pid = fake.device.attached.lock().unwrap().as_ref().unwrap().id() as i32;
        fake.device.detach();
        assert!(!fake.device.hears());
        // Killed outright, and reaped: nothing of the process is left.
        assert_ne!(unsafe { libc::kill(pid, 0) }, 0, "the launch is gone");
    }

    #[test]
    fn the_launch_is_in_a_session_of_its_own() {
        let fake = Fake::new("session", "exec sleep 60", 0.3);
        assert_eq!(fake.device.launch("dev.example.demo", &[], None, Some(nothing()), &Default::default()), (true, String::new()));
        let pid = fake.device.attached.lock().unwrap().as_ref().unwrap().id() as i32;
        assert_eq!(unsafe { libc::getsid(pid) }, pid);
        assert_eq!(unsafe { libc::getpgid(pid) }, pid);
    }

    #[test]
    fn a_launch_that_fails_says_why() {
        let fake = Fake::new("fails", "echo 'ERROR: The device is locked.'\nexit 1", 20.0);
        assert_eq!(fake.device.launch("dev.example.demo", &[], None, Some(nothing()), &Default::default()),
                   (false, "ERROR: The device is locked.".to_string()));
        assert!(!fake.device.hears());
    }

    #[test]
    fn a_launch_that_says_nothing_and_fails_says_its_code() {
        let fake = Fake::new("silent", "exit 3", 20.0);
        assert_eq!(fake.device.launch("dev.example.demo", &[], None, Some(nothing()), &Default::default()), (false, "devicectl exited with 3".to_string()));
    }

    #[test]
    fn an_app_that_is_gone_at_once_was_still_launched() {
        let fake = Fake::new("gone", "echo 'Launched application with dev.example.demo bundle identifier.'", 20.0);
        assert_eq!(fake.device.launch("dev.example.demo", &[], None, Some(nothing()), &Default::default()), (true, String::new()));
        // It may still be on its way out when "Launched application" is said: wait for it as the launch may.
        let attached = fake.device.attached.lock().unwrap().take();
        if let Some(mut child) = attached {
            child.wait().unwrap();
        }
        assert!(!fake.device.hears());
    }

    #[test]
    fn a_new_launch_lets_go_of_the_old_one() {
        let fake = Fake::new("again", "exec sleep 60", 0.3);
        assert!(fake.device.launch("dev.example.demo", &[], None, Some(nothing()), &Default::default()).0);
        let first = fake.device.attached.lock().unwrap().as_ref().unwrap().id() as i32;
        assert!(fake.device.launch("dev.example.demo", &[], None, Some(nothing()), &Default::default()).0);
        let second = fake.device.attached.lock().unwrap().as_ref().unwrap().id() as i32;
        assert_ne!(first, second);
        assert_ne!(unsafe { libc::kill(first, 0) }, 0, "the first launch is gone");
    }

    #[test]
    fn a_launch_that_cannot_be_started_says_why() {
        let fake = Fake::new("nostart", "exit 0", 1.0);
        let device = Devicectl::with_program(UDID, "/nonexistent/xcrun");
        let (done, message) = device.launch("dev.example.demo", &[], None, Some(nothing()), &Default::default());
        assert!(!done && !message.is_empty());
        drop(fake);
    }

    #[test]
    fn the_temporary_folders_are_removed() {
        let folders = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&folders);
        let run: Runner = Box::new(move |command, _, _| {
            let folder = output_path(command).parent().unwrap().to_path_buf();
            assert!(folder.is_dir());
            seen.lock().unwrap().push(folder);
            Ok(ok(0))
        });
        let spawn: Spawner = Box::new(|_, _| Err(std::io::Error::other("no")));
        let device = Devicectl::with_runners("UDID", run, spawn);
        device.install("a");
        device.write_host("b", &json!({}));
        device.read_report("b");
        let folders = folders.lock().unwrap();
        assert_eq!(folders.len(), 3);
        assert!(folders.iter().all(|folder| !folder.exists()));
    }
}
