//! `QueueTestCase` and `FakeDevice` of the Python tests.
#![allow(dead_code)]

use crate::base::testing::{Sandbox, capture};
use crate::commands;
use crate::device::{Device, Heard};
use crate::dispatcher::{Dispatcher, use_device};
use crate::state::Queue;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub const UDID: &str = "UDID-TEST";

/// What the made-up phone has been asked, and what it answers.
pub struct FakeState {
    pub lock: String,
    pub installs: Vec<String>,
    /// (bundle id, arguments, url) of every launch.
    pub launches: Vec<(String, Vec<String>, Option<String>)>,
    pub launch_environments: Vec<std::collections::BTreeMap<String, String>>,
    pub report: Option<Value>,
    pub install_result: (bool, String),
    /// What the Mac has put beside the app's marks, and whether the phone takes it.
    pub told: Vec<Value>,
    pub takes_files: bool,
    /// What the queue gave to be told the app's lines by, and whether the launch is still attached.
    pub heard: Option<Heard>,
    pub attached: bool,
    /// What a launch answers, when a test makes it fail.
    pub launch_result: Option<(bool, String)>,
}

pub struct FakeDevice {
    pub state: Mutex<FakeState>,
}

impl FakeDevice {
    pub fn new() -> Arc<FakeDevice> {
        Arc::new(FakeDevice {
            state: Mutex::new(FakeState {
                lock: "unlocked".into(), installs: Vec::new(), launches: Vec::new(), launch_environments: Vec::new(), report: None,
                install_result: (true, String::new()), told: Vec::new(), takes_files: true, heard: None, attached: false, launch_result: None,
            }),
        })
    }

    pub fn with<R>(&self, change: impl FnOnce(&mut FakeState) -> R) -> R {
        change(&mut self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner()))
    }

    pub fn installs(&self) -> Vec<String> {
        self.with(|state| state.installs.clone())
    }

    pub fn launches(&self) -> Vec<(String, Vec<String>, Option<String>)> {
        self.with(|state| state.launches.clone())
    }
}

impl Device for FakeDevice {
    fn lock_state(&self) -> String {
        self.with(|state| state.lock.clone())
    }

    fn install(&self, app: &str) -> (bool, String) {
        self.with(|state| {
            state.installs.push(app.to_string());
            state.install_result.clone()
        })
    }

    fn launch(&self, bundle_id: &str, arguments: &[String], url: Option<&str>, heard: Option<Heard>, _environment: &crate::private_launch_env::LaunchEnvironment) -> (bool, String) {
        self.with(|state| {
            state.launch_environments.push(_environment.variables().clone());
            state.launches.push((bundle_id.to_string(), arguments.to_vec(), url.map(str::to_string)));
            state.attached = heard.is_some();
            state.heard = heard;
            state.launch_result.clone().unwrap_or((true, String::new()))
        })
    }

    fn hears(&self) -> bool {
        self.with(|state| state.attached)
    }

    fn detach(&self) {
        self.with(|state| state.attached = false);
    }

    fn read_report(&self, _bundle_id: &str) -> Option<Value> {
        self.with(|state| state.report.clone())
    }

    fn write_host(&self, _bundle_id: &str, said: &Value) -> bool {
        self.with(|state| {
            state.told.push(said.clone());
            state.takes_files
        })
    }
}

/// A made-up home, a built app, a phone that is made up and a dispatcher stepped by hand.
pub struct Case {
    pub sandbox: Sandbox,
    pub app: PathBuf,
    pub queue: Queue,
    pub device: Arc<FakeDevice>,
    pub dispatcher: Dispatcher,
    pub notes: Arc<Mutex<Vec<(String, String)>>>,
}

impl Case {
    pub fn new(name: &str) -> Case {
        let sandbox = Sandbox::new(name);
        let app = sandbox.folder.join("Build").join("Demo.app");
        std::fs::create_dir_all(&app).unwrap();
        // A thin 64-bit Mach-O header followed by the argument the app declares.
        let mut binary = vec![0xcf, 0xfa, 0xed, 0xfe];
        binary.extend(vec![0u8; 64]);
        binary.extend(b"--ios-run-checklist\0");
        std::fs::write(app.join("Demo"), binary).unwrap();
        std::fs::write(app.join("Info.plist"), "<plist/>").unwrap();
        let queue = Queue::named(UDID, "Test iPhone", "");
        let device = FakeDevice::new();
        // A command that reads the phone by itself reads this made-up one: none asks devicectl about a real phone.
        use_device(Some(device.clone()));
        let notes = Arc::new(Mutex::new(Vec::new()));
        let kept = notes.clone();
        let dispatcher = Dispatcher::new(queue.clone(), device.clone())
            .with_notifier(move |title, body| kept.lock().unwrap().push((title.to_string(), body.to_string())))
            .with_sleep(|_| {});
        Case { sandbox, app, queue, device, dispatcher, notes }
    }

    /// One call of `hitl _queues`: (exit code, stdout, stderr).
    pub fn run_cli(&self, arguments: &[&str]) -> (i32, String, String) {
        let arguments: Vec<String> = arguments.iter().map(|word| word.to_string()).collect();
        let (code, out, err) = capture(|| commands::run(&arguments));
        (code, out, err)
    }

    /// Queues a check without waiting for it; its number.
    pub fn submit(&mut self, tag: &str, items: &[&str]) -> String {
        self.submit_as(tag, items, "", tag, &[])
    }

    pub fn submit_as(&mut self, tag: &str, items: &[&str], title: &str, chat: &str, extra: &[&str]) -> String {
        self.sandbox.set("CONVEYOR_CHAT", chat);
        let app = self.app.display().to_string();
        let mut arguments: Vec<String> = ["check", "--udid", UDID, "--name", "Test iPhone", "--app", app.as_str(), "--bundle-id", "dev.example.demo",
                                          "--tag", tag, "--branch", "nextster/demo", "--no-wait"].iter().map(|word| word.to_string()).collect();
        if !title.is_empty() {
            arguments.extend(["--title".to_string(), title.to_string()]);
        }
        let default = ["Открой экран → он открылся"];
        for item in if items.is_empty() { &default[..] } else { items } {
            arguments.extend(["--item".to_string(), item.to_string()]);
        }
        arguments.extend(extra.iter().map(|word| word.to_string()));
        let (code, out, err) = capture(|| commands::run(&arguments));
        assert_eq!(code, 0, "{err}");
        out.trim().to_string()
    }

    pub fn state(&self) -> Value {
        self.queue.load()
    }

    /// A person at a terminal: no chat around the command.
    pub fn as_artem(&mut self) {
        self.sandbox.set("CONVEYOR_CHAT", "");
    }

    pub fn as_chat(&mut self, name: &str) {
        self.sandbox.set("CONVEYOR_CHAT", name);
    }

    /// What the app reports: the marks by label, as (state, note).
    pub fn report(&self, ticket_id: &str, marks: &[(&str, Option<&str>, Option<&str>)], finished: bool, comment: Option<&str>) {
        let results: Vec<Value> = marks.iter().map(|(label, state, note)| json!({"label": label, "state": state, "note": note})).collect();
        self.device.with(|state| {
            state.report = Some(json!({"id": ticket_id, "results": results, "comment": comment,
                                       "finishedAt": if finished { json!("2026-10-07T12:00:00Z") } else { Value::Null }}));
        });
    }

    pub fn step(&mut self) -> bool {
        self.dispatcher.step(false)
    }

    pub fn notes(&self) -> Vec<(String, String)> {
        self.notes.lock().unwrap().clone()
    }
}

impl Drop for Case {
    fn drop(&mut self) {
        use_device(None);
    }
}
