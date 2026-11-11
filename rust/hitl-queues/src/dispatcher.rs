//! The dispatcher: the detached process that installs a phone's checks in their turn and reads their marks from both
//! screens (queues.py `Dispatcher`, `command_dispatch`).
//!
//! It is the only process that installs checks and reads their results, so a chat that stops waiting loses nothing:
//! its build is still installed in turn and its verdict is kept. Started by the first check, gone when the queue is
//! empty. What it keeps in memory (the last report, what the app said, what the app was told of the Mac) is shared by
//! three threads: the loop, the one that hurries his marks from the Mac to the phone, and the one that reads the app's
//! output.

use crate::base::{FileLock, STATE_VERSION, number, poll_seconds, text};
use crate::bridge;
use crate::device::{Device, Devicectl, Heard};
use crate::state::*;
use crate::verdict::{finish, for_the_phone, merge_report, said_on_the_mac};
use crate::base::now;
use hitl_host_core::{entry, root};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::os::unix::process::CommandExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A continuous device blocker becomes one persistent action in the Mac window.
pub const BLOCKED_ACTION_SECONDS: f64 = 60.0;
/// A probe is an agent's own run; a check waits for it, but not forever.
pub fn probe_grace_seconds() -> f64 {
    hitl_host_core::var("CONVEYOR_PROBE_GRACE_SECONDS").and_then(|text| text.trim().parse().ok()).unwrap_or(600.0)
}
pub const INSTALL_ATTEMPTS: i64 = 3;

#[cfg(test)]
static DEVICE_OVERRIDE: Mutex<Option<Arc<dyn Device>>> = Mutex::new(None);

/// A command that reads the phone by itself (release) reads this: in the tests a made-up one.
pub fn real_device(udid: &str) -> Arc<dyn Device> {
    #[cfg(test)]
    if let Some(device) = DEVICE_OVERRIDE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone() {
        return device;
    }
    Arc::new(Devicectl::new(udid))
}

/// The phone every command of a test reads, in place of a real one.
#[cfg(test)]
pub fn use_device(device: Option<Arc<dyn Device>>) {
    *DEVICE_OVERRIDE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = device;
}

fn percent_encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// What the app said of a check: (ended, the latest mark or move, the latest thing it did that the Mac asked for).
fn lateness(report: &Value) -> (bool, f64, f64) {
    let stamps = report.get("results").and_then(Value::as_array).into_iter().flatten()
        .filter(|entry| entry.is_object()).map(|entry| entry.get("at").and_then(Value::as_f64).unwrap_or(0.0));
    let latest = stamps.chain(std::iter::once(report.get("currentAt").and_then(Value::as_f64).unwrap_or(0.0))).fold(0.0, f64::max);
    let ran = report.get("ran").filter(|ran| ran.is_object()).map_or(0.0, |ran| ran.get("at").and_then(Value::as_f64).unwrap_or(0.0));
    (hitl_host_core::truthy(report.get("finishedAt")), latest, ran)
}

fn later(left: (bool, f64, f64), right: (bool, f64, f64)) -> std::cmp::Ordering {
    left.0.cmp(&right.0)
        .then(left.1.partial_cmp(&right.1).unwrap_or(std::cmp::Ordering::Equal))
        .then(left.2.partial_cmp(&right.2).unwrap_or(std::cmp::Ordering::Equal))
}

#[derive(Default)]
struct Noting {
    /// The phone's report as last read.
    report: Option<Value>,
    /// What the app last said on its output, the launch being attached to it.
    spoken: Option<Value>,
}

/// What the loop, the thread that hurries the marks and the thread that hears the app share.
pub struct Shared {
    queue: Queue,
    device: Arc<dyn Device>,
    /// The check that is on the phone, for the thread that hurries his marks from the Mac to it.
    on_the_phone: Mutex<Option<Value>>,
    noting: Mutex<Noting>,
    /// What of the Mac's marks the app has been told: (ticket id, what was told). One teller at a time.
    telling: Mutex<Option<(String, String)>>,
    /// When either screen last showed a sign of him: while he is at the check, both are looked at all the time.
    stirred: Mutex<f64>,
}

fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Shared {
    /// Takes in what the phone says of the check, from the app's output or from its file, and leaves it for the
    /// window. A read of the file that was already on its way when a later line came says less than the line, and is
    /// dropped.
    pub fn take(&self, active: &Value, report: Option<&Value>) {
        let Some(report) = report.filter(|report| report.get("id").is_some() && report.get("id") == active.get("id")) else { return };
        let mut noting = locked(&self.noting);
        let known = noting.report.clone().filter(|known| known.get("id") == active.get("id"));
        if let Some(known) = &known {
            if later(lateness(report), lateness(known)) == std::cmp::Ordering::Less {
                return;
            }
        }
        if known.as_ref() != Some(report) {
            *locked(&self.stirred) = now();
            let marks: Vec<Value> = merge_report(active, Some(report), None).iter().map(|entry| {
                json!({"label": entry["label"], "state": entry["state"], "note": entry["note"], "at": entry.get("at").cloned().unwrap_or(Value::Null)})
            }).collect();
            let mut said = json!({"id": active["id"], "results": marks, "current": report.get("current").cloned().unwrap_or(Value::Null),
                                  "currentAt": report.get("currentAt").cloned().unwrap_or(Value::Null), "readAt": now()});
            // The buttons the panel offers with an item, for the window to offer the same, and how the last one
            // pressed there went.
            for (key, is_kind) in [("offers", Value::is_array as fn(&Value) -> bool), ("ran", Value::is_object)] {
                if let Some(value) = report.get(key).filter(|value| is_kind(value)) {
                    said[key] = value.clone();
                }
            }
            let _ = hitl_host_core::write_json(&self.queue.ticket_folder(text(active, "id")).join("phone.json"), &said);
        }
        noting.report = Some(report.clone());
    }

    /// A report the app said on its output, the moment something changed on the phone; from the thread that reads the
    /// attached launch. It is in the window's hands at once, not at the loop's next pass.
    pub fn hear(&self, report: Value) {
        let active = locked(&self.on_the_phone).clone();
        if let Some(active) = &active {
            if report.get("id") != active.get("id") {
                return;
            }
        }
        locked(&self.noting).spoken = Some(report.clone());
        if let Some(active) = active {
            self.take(&active, Some(&report));
        }
    }

    /// Passes on to the app what he did in the window on the Mac, when that has changed since the app was last told.
    /// Returns what he did there.
    pub fn tell_phone(&self, active: &Value) -> Option<Value> {
        let mut told = locked(&self.telling);
        let mine = said_on_the_mac(&self.queue, active);
        if let Some(mine) = &mine {
            if hitl_host_core::truthy(active.get("overlay")) {
                let saying = for_the_phone(mine);
                let said = (text(active, "id").to_string(), hitl_host_core::dumps_line(&saying));
                let something = hitl_host_core::truthy(saying.get("results")) || hitl_host_core::truthy(saying.get("finished"))
                    || hitl_host_core::truthy(saying.get("current")) || hitl_host_core::truthy(saying.get("run"));
                if something && Some(&said) != told.as_ref() {
                    *locked(&self.stirred) = now();
                    if self.device.write_host(text(active, "bundleId"), &saying) {
                        *told = Some(said);
                    }
                }
            }
        }
        mine
    }

    /// Beside the loop, for as long as it runs: the moment the window on the Mac writes down a mark, a comment or a
    /// move, it goes to the phone, without waiting for the loop to come round. Measured on the phone by cable
    /// (2026-10-10): 0.57 s from the window's file to the app's when the loop did it, of which the write itself is 0.17 s.
    fn hurry(&self, stop: &AtomicBool) {
        let mut seen: Option<(String, std::time::SystemTime)> = None;
        while !stop.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(20));
            let Some(active) = locked(&self.on_the_phone).clone() else {
                seen = None;
                continue;
            };
            let id = text(&active, "id").to_string();
            let Ok(stamp) = std::fs::metadata(self.queue.ticket_folder(&id).join("mac.json")).and_then(|meta| meta.modified()) else { continue };
            if seen.as_ref() != Some(&(id.clone(), stamp)) {
                seen = Some((id, stamp));
                self.tell_phone(&active);
            }
        }
    }
}

#[cfg(test)]
impl Shared {
    /// The check the thread that hurries his marks sees as the one on the phone.
    pub fn on_the_phone(&self) -> Option<Value> {
        locked(&self.on_the_phone).clone()
    }
    /// When either screen last showed a sign of him (the tests move it back in time).
    pub fn set_stirred(&self, moment: f64) {
        *locked(&self.stirred) = moment;
    }
}

pub struct Dispatcher {
    shared: Arc<Shared>,
    notify: Box<dyn Fn(&str, &str) + Send + Sync>,
    sleep: Box<dyn Fn(f64) + Send + Sync>,
    release: Box<dyn Fn() + Send + Sync>,
    restart: Option<Box<dyn Fn() + Send + Sync>>,
    show: Box<dyn Fn(&Queue, &Value) + Send + Sync>,
    /// How long the loop has gone without reading the phone.
    waited: f64,
    /// When the window on the Mac was last looked after: that it is up and has this check.
    shown_at: f64,
    notified: HashSet<(String, String)>,
    actions_key: Option<String>,
    actions_checked_at: f64,
    pause: f64,
    code_stamp: f64,
    code_checked_at: std::time::Instant,
}

/// The newest change of anything this process runs: the package's Python and the crates' sources.
fn code_time() -> f64 {
    fn newest(folder: &std::path::Path, extension: &str, deep: bool, found: &mut f64) {
        for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() && deep && entry.file_name() != "target" {
                newest(&path, extension, deep, found);
            } else if kind.is_file() && path.extension().is_some_and(|one| one == extension) {
                if let Ok(modified) = entry.metadata().and_then(|meta| meta.modified()) {
                    if let Ok(since) = modified.duration_since(std::time::UNIX_EPOCH) {
                        *found = found.max(since.as_secs_f64());
                    }
                }
            }
        }
    }
    let mut found = 0.0;
    let root = root();
    newest(&root.join("src").join("hitl"), "py", false, &mut found);
    for crate_folder in std::fs::read_dir(root.join("rust")).into_iter().flatten().flatten() {
        newest(&crate_folder.path().join("src"), "rs", true, &mut found);
    }
    found
}

impl Dispatcher {
    pub fn new(queue: Queue, device: Arc<dyn Device>) -> Dispatcher {
        Dispatcher {
            shared: Arc::new(Shared { queue, device, on_the_phone: Mutex::new(None), noting: Mutex::new(Noting::default()),
                                      telling: Mutex::new(None), stirred: Mutex::new(0.0) }),
            notify: Box::new(|title, body| hitl::common::notify(title, body)),
            sleep: Box::new(|seconds| std::thread::sleep(Duration::from_secs_f64(seconds.max(0.0)))),
            release: Box::new(|| {}),
            restart: None,
            show: Box::new(|queue, ticket| show_in_window(queue, ticket)),
            waited: 0.0,
            shown_at: 0.0,
            notified: HashSet::new(),
            actions_key: None,
            actions_checked_at: 0.0,
            pause: poll_seconds(),
            code_stamp: code_time(),
            code_checked_at: std::time::Instant::now(),
        }
    }

    pub fn with_notifier(mut self, notify: impl Fn(&str, &str) + Send + Sync + 'static) -> Dispatcher {
        self.notify = Box::new(notify);
        self
    }
    pub fn with_sleep(mut self, sleep: impl Fn(f64) + Send + Sync + 'static) -> Dispatcher {
        self.sleep = Box::new(sleep);
        self
    }
    pub fn with_release(mut self, release: impl Fn() + Send + Sync + 'static) -> Dispatcher {
        self.release = Box::new(release);
        self
    }
    pub fn with_restart(mut self, restart: impl Fn() + Send + Sync + 'static) -> Dispatcher {
        self.restart = Some(Box::new(restart));
        self
    }
    pub fn with_show(mut self, show: impl Fn(&Queue, &Value) + Send + Sync + 'static) -> Dispatcher {
        self.show = Box::new(show);
        self
    }

    pub fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }
    pub fn queue(&self) -> &Queue {
        &self.shared.queue
    }
    pub fn device(&self) -> &Arc<dyn Device> {
        &self.shared.device
    }
    pub fn pause_seconds(&self) -> f64 {
        self.pause
    }
    /// How long the loop has gone without reading the phone (the tests move it by hand).
    #[cfg(test)]
    pub fn set_waited(&mut self, seconds: f64) {
        self.waited = seconds;
    }
    #[cfg(test)]
    pub fn add_waited(&mut self, seconds: f64) {
        self.waited += seconds;
    }

    /// A test's way to say the helper's sources changed: the stamp is old and the once-a-second look is due.
    #[cfg(test)]
    pub fn mark_code_changed(&mut self) {
        self.code_stamp -= 1.0;
        self.code_checked_at = std::time::Instant::now() - Duration::from_secs(2);
    }

    fn code_changed(&mut self) -> bool {
        // Looked at about once a second: the loop comes round fifty times in that.
        if self.code_checked_at.elapsed() < Duration::from_secs(1) {
            return false;
        }
        self.code_checked_at = std::time::Instant::now();
        code_time() != self.code_stamp
    }

    fn notify_once(&mut self, ticket_id: &str, kind: &str, title: &str, body: &str) {
        if self.notified.insert((ticket_id.to_string(), kind.to_string())) {
            (self.notify)(title, body);
        }
    }

    /// One pass. False when nothing is left to do. A patient pass is the loop's own: it looks at what was marked on
    /// the Mac every time, and reads the phone only when the time for it has come.
    pub fn step(&mut self, patient: bool) -> bool {
        let queue = self.shared.queue.clone();
        let (active, head, held, probe, name) = queue.locked(|state| {
            self.reconcile_actions(state);
            let active = state.get("active").filter(|active| active.is_object()).cloned();
            let allowed: Vec<Value> = state["queue"].as_array().into_iter().flatten().filter(|ticket| installable(state, ticket)).cloned().collect();
            let head = allowed.first().cloned();
            let held = !state["queue"].as_array().is_none_or(Vec::is_empty) && allowed.is_empty();
            let probe = live_probe(state);
            let name = Some(text(state, "name")).filter(|name| !name.is_empty()).unwrap_or("iPhone").to_string();
            (active, head, held, probe, name)
        });
        self.pause = poll_seconds();
        if let Some(active) = active {
            // A check nobody has touched for a quarter of an hour is read less often.
            let touched = number(&active, "installedAt").max(active.get("progress").map_or(0.0, |progress| number(progress, "updatedAt")));
            let quiet = now() - touched > 900.0;
            // He has just marked, written or moved on one of the screens: the other has to show it before he looks
            // up. One read of the phone takes a sixth of a second by cable, so while he is at it they go one after
            // another; a minute after his last move, once a second; after a quarter of an hour, once in ten.
            let stirred = *locked(&self.shared.stirred);
            let lively = now() - stirred.max(number(&active, "installedAt")) < 60.0;
            let mut every = if lively { 0.0 } else if quiet { poll_seconds().max(10.0) } else { poll_seconds().min(1.0) };
            // The app says every change on its output and the launch is attached to it: its file is read only to be
            // sure, and the phone is left in peace.
            let spoken_now = locked(&self.shared.noting).spoken.as_ref().is_some_and(|spoken| spoken.get("id") == active.get("id"));
            if spoken_now && self.shared.device.hears() {
                every = every.max(poll_seconds().min(2.0));
            }
            let read = !patient || self.waited >= every;
            if read {
                self.waited = 0.0;
            }
            *locked(&self.shared.on_the_phone) = Some(active.clone());
            self.watch(&active, read);
            // What he does in the window on the Mac is a file here: looked at often, it costs nothing.
            self.pause = poll_seconds().min(if lively { 0.02 } else if quiet { 2.0 } else { 0.5 });
            return true;
        }
        *locked(&self.shared.on_the_phone) = None;
        self.shared.device.detach();
        let Some(head) = head else {
            // A paused queue is not an empty one: stay, and look less often.
            if held {
                self.pause = poll_seconds().max(5.0);
            }
            return held;
        };
        if let Some(probe) = probe {
            let waited = now() - head.get("probeSeenAt").and_then(Value::as_f64).unwrap_or_else(now);
            if waited < probe_grace_seconds() {
                let seen = head.get("probeSeenAt").cloned().filter(|seen| hitl_host_core::truthy(Some(seen))).unwrap_or_else(|| json!(now()));
                let tag = Some(text(&probe, "tag")).filter(|tag| !tag.is_empty()).unwrap_or("?");
                self.remember(text(&head, "id"), &[("probeSeenAt", Some(seen)), ("waitingFor", Some(json!(format!("ждёт конца пробы чата {tag}"))))]);
                return true;
            }
            queue.locked(|state| {
                if hitl_host_core::truthy(state.get("probe")) {
                    state["probe"]["preemptedBy"] = json!(text(&head, "tag"));
                }
            });
        }
        self.install(&head, &name);
        true
    }

    pub(crate) fn action_owner(&self, ticket_id: &str) -> Value {
        json!({"type": "iphone-blocked", "udid": self.shared.queue.udid, "phoneCheck": ticket_id})
    }

    /// Under the phone lock: clean only this phone's typed actions, even after restart/cancel.
    ///
    /// A queued card survives unlocking until the install+launch outcome is known. A card waiting for somebody else's
    /// check/probe/pause is never actionable.
    fn reconcile_actions(&mut self, state: &mut Value) {
        let allowed: Vec<Value> = state["queue"].as_array().into_iter().flatten().filter(|ticket| installable(state, ticket)).cloned().collect();
        let probe = live_probe(state);
        let mut head = if state.get("active").is_none_or(Value::is_null) { allowed.first().cloned() } else { None };
        if let (Some(ticket), Some(_)) = (&head, &probe) {
            if now() - ticket.get("probeSeenAt").and_then(Value::as_f64).unwrap_or_else(now) < probe_grace_seconds() {
                head = None;
            }
        }
        let eligible = head.as_ref().map(|ticket| text(ticket, "id").to_string());
        let key = json!([eligible, state.get("active").and_then(|active| active.get("id")),
                         state["queue"].as_array().into_iter().flatten().map(|t| text(t, "id")).collect::<Vec<_>>(),
                         state["recent"].as_array().into_iter().flatten().map(|t| [text(t, "id"), text(t, "state")]).collect::<Vec<_>>()]).to_string();
        let elapsed = now() - self.actions_checked_at;
        if self.actions_key.as_deref() == Some(key.as_str()) && (0.0..poll_seconds()).contains(&elapsed) {
            return; // do not scan the Mac queue at the active checklist's 50 Hz refresh rate
        }
        self.actions_key = Some(key);
        self.actions_checked_at = now();
        if let Some(waiting) = state["queue"].as_array_mut() {
            for ticket in waiting {
                if Some(text(ticket, "id")) != eligible.as_deref() {
                    if let Some(map) = ticket.as_object_mut() {
                        map.remove("deviceBlockedSince");
                    }
                }
            }
        }
        for action in bridge::window_queued() {
            let owner = action.get("automaticAction").cloned().unwrap_or(json!({}));
            if text(&owner, "type") != "iphone-blocked" || text(&owner, "udid") != self.shared.queue.udid {
                continue;
            }
            let ticket_id = text(&owner, "phoneCheck").to_string();
            if Some(ticket_id.as_str()) == eligible.as_deref() {
                continue;
            }
            let active = state.get("active").cloned().unwrap_or(json!({}));
            let recent = state["recent"].as_array().into_iter().flatten().rev().find(|ticket| text(ticket, "id") == ticket_id).cloned().unwrap_or(json!({}));
            let reason = if text(&active, "id") == ticket_id {
                "installed-and-launched".to_string()
            } else {
                Some(text(&recent, "state")).filter(|state| !state.is_empty()).unwrap_or("no-longer-eligible").to_string()
            };
            bridge::resolve_automatic_action(&self.action_owner(&ticket_id), &reason);
        }
    }

    fn device_blocked(&mut self, head: &Value, lock: &str) {
        let queue = self.shared.queue.clone();
        let owner = self.action_owner(text(head, "id"));
        let shown = queue.locked(|state| {
            let allowed: Vec<Value> = state["queue"].as_array().into_iter().flatten().filter(|ticket| installable(state, ticket)).cloned().collect();
            if state.get("active").is_some_and(|active| !active.is_null()) || allowed.is_empty() || text(&allowed[0], "id") != text(head, "id") {
                return false;
            }
            let index = state["queue"].as_array().into_iter().flatten().position(|ticket| text(ticket, "id") == text(head, "id")).unwrap_or(0);
            let current = state["queue"][index].clone();
            if lock != "locked" && lock != "unreachable" {
                if let Some(map) = state["queue"][index].as_object_mut() {
                    map.remove("deviceBlockedSince");
                }
                return false;
            }
            let card_id = bridge::automatic_action_id(&owner);
            if hitl_host_core::truthy(current.get("deviceActionIssued")) && !window_ticket(&card_id).exists() {
                return false; // already answered/resolved; receipt retention is not a re-notification timer
            }
            let began = match current.get("deviceBlockedSince").and_then(Value::as_f64) {
                Some(began) => began,
                None => {
                    state["queue"][index]["deviceBlockedSince"] = json!(now());
                    now()
                }
            };
            if now() - began < BLOCKED_ACTION_SECONDS {
                return false;
            }
            // Submit captures the originating chat; the dispatcher is not the owner.
            let mut links: Vec<String> = Vec::new();
            let app = text(&current, "agent");
            let identifiers = current.get("chatIds").and_then(Value::as_array).cloned().unwrap_or_default();
            if matches!(app, "Codex" | "Claude" | "OpenCode") {
                if let Some(first) = identifiers.first().and_then(Value::as_str) {
                    links = vec![format!("conveyor://chat/{app}/{}", percent_encode(first))];
                }
            }
            if links.is_empty() && !text(&current, "chatLink").is_empty() {
                links = vec![text(&current, "chatLink").to_string()];
            }
            // Old tickets without a chat address still have an exact local queue receipt.
            if links.is_empty() {
                let receipt = queue.ticket_folder(text(head, "id")).join("waiting-for-phone.txt");
                links = vec![receipt.display().to_string()];
                let title = Some(text(&current, "title")).filter(|title| !title.is_empty()).unwrap_or(text(&current, "tag"));
                let _ = hitl_host_core::write_json(&receipt, &json!({
                    "Проверка": title, "Проект": text(&current, "project"), "Номер": text(head, "id"),
                    "Что сделать": "Разблокируй iPhone и подключи к Маку.",
                    "Что дальше": "Очередь сама установит и запустит сборку; нажимать «Сделал» не требуется."}));
            }
            let (kind_title, answers) = bridge::action_kind();
            let title: String = Some(text(&current, "title")).filter(|title| !title.is_empty()).unwrap_or(text(&current, "tag")).chars().take(95).collect();
            let project = text(&current, "project");
            let mut ticket = json!({});
            for key in ["tag", "chat", "chatLink", "agent", "chatIds", "project", "branch", "commit"] {
                ticket[key] = current.get(key).cloned().unwrap_or(Value::Null);
            }
            ticket["screen"] = json!("mac");
            ticket["projectName"] = json!(if project.is_empty() { String::new() } else { hitl_host_core::git::project_name(Some(project)) });
            ticket["cwd"] = json!(project);
            ticket["title"] = json!("iPhone: нужно действие");
            ticket["about"] = json!(format!("Проверка iPhone «{title}» ждёт телефон больше минуты."));
            ticket["kind"] = json!("action");
            ticket["kindTitle"] = json!(kind_title);
            ticket["answers"] = json!(answers);
            ticket["createdAt"] = json!(now());
            ticket["items"] = json!([{"label": "1", "text": "iPhone недоступен: разблокируй его и подключи к Маку",
                "details": "Сборка ждёт телефон. Действие снимется после установки и запуска сборки.", "links": links}]);
            let shown = bridge::submit_automatic_action(&owner, &ticket);
            state["queue"][index]["deviceActionIssued"] = json!(true);
            shown
        });
        if shown {
            bridge::ensure_window();
        }
    }

    /// Sets (or, with None, removes) fields of a waiting or active ticket.
    fn remember(&self, ticket_id: &str, fields: &[(&str, Option<Value>)]) {
        self.shared.queue.locked(|state| {
            let apply = |ticket: &mut Value| {
                if text(ticket, "id") == ticket_id {
                    for (key, value) in fields {
                        match value {
                            Some(value) => ticket[*key] = value.clone(),
                            None => {
                                if let Some(map) = ticket.as_object_mut() {
                                    map.remove(*key);
                                }
                            }
                        }
                    }
                }
            };
            if let Some(waiting) = state["queue"].as_array_mut() {
                waiting.iter_mut().for_each(apply);
            }
            if let Some(active) = state.get_mut("active").filter(|active| active.is_object()) {
                apply(active);
            }
        });
    }

    /// One look at the check that is on the phone, on both of its screens: reads the marks made on the phone, when it
    /// is asked to, and the ones made in the window on the Mac; tells each screen of the other; and ends the check
    /// when he ended it on either.
    pub fn watch(&mut self, active: &Value, read: bool) {
        if now() - self.shown_at >= 5.0 {
            self.shown_at = now();
            (self.show)(&self.shared.queue, active);
        }
        // First what he did on the Mac goes to the phone, then the phone is read: a mark that waited for the read that
        // happened to be going on was half a second late there.
        let mine = self.shared.tell_phone(active);
        {
            let mut noting = locked(&self.shared.noting);
            if noting.report.as_ref().and_then(|report| report.get("id")) != active.get("id") {
                noting.report = None;
            }
        }
        if hitl_host_core::truthy(active.get("overlay")) {
            let (spoken, report) = {
                let noting = locked(&self.shared.noting);
                (noting.spoken.clone(), noting.report.clone())
            };
            if spoken != report {
                self.shared.take(active, spoken.as_ref());
            }
            if read {
                let fresh = self.shared.device.read_report(text(active, "bundleId"));
                self.shared.take(active, fresh.as_ref());
            }
        }
        let report = locked(&self.shared.noting).report.clone();
        let marked = merge_report(active, report.as_ref(), mine.as_ref()).iter().filter(|entry| hitl_host_core::truthy(entry.get("state"))).count();
        let queue = self.shared.queue.clone();
        queue.locked(|state| {
            let Some(current) = state.get("active").filter(|current| current.is_object() && current.get("id") == active.get("id")).cloned() else { return };
            if current["progress"].get("marked").and_then(Value::as_i64) != Some(marked as i64) {
                state["active"]["progress"] = json!({"marked": marked, "updatedAt": now()});
            }
            let current = state["active"].clone();
            // Ended in the window, the check is the Mac's, though the app says so too: told of the end, the app ends
            // its own, and its word about that is heard before this loop comes round. Read the other way, a check he
            // sent back from the Mac with every item working was one he ended on the phone, his «ок».
            if mine.as_ref().is_some_and(|mine| hitl_host_core::truthy(mine.get("finish"))) {
                finish(&queue, state, current, "mac", report.as_ref(), None, "done");
            } else if report.as_ref().is_some_and(|report| hitl_host_core::truthy(report.get("finishedAt"))) {
                finish(&queue, state, current, "phone", report.as_ref(), None, "done");
            }
        });
    }

    fn install(&mut self, head: &Value, name: &str) {
        let _ = name;
        let queue = self.shared.queue.clone();
        let head_id = text(head, "id").to_string();
        let lock = self.shared.device.lock_state();
        if lock != "unlocked" {
            self.device_blocked(head, &lock);
            let waiting = match lock.as_str() {
                "locked" => "ждёт, пока телефон разблокируют",
                "unreachable" => "телефон не виден с Мака",
                _ => "не удалось определить состояние телефона",
            };
            self.remember(&head_id, &[("waitingFor", Some(json!(waiting)))]);
            return;
        }
        self.device_blocked(head, &lock);
        self.remember(&head_id, &[("waitingFor", Some(json!("ставится на телефон")))]);
        // A check set aside and put on the phone again starts from the new install: the report heard of the last one,
        // what its app said and what it was told of the Mac's marks are not this one's. Kept, they had the window show
        // a mark the phone no longer had and the app never told the Mac's (the audit of 2026-10-16, H07).
        {
            let mut noting = locked(&self.shared.noting);
            if noting.report.as_ref().and_then(|report| report.get("id")).and_then(Value::as_str) == Some(head_id.as_str()) {
                noting.report = None;
            }
            if noting.spoken.as_ref().and_then(|spoken| spoken.get("id")).and_then(Value::as_str) == Some(head_id.as_str()) {
                noting.spoken = None;
            }
        }
        {
            let mut told = locked(&self.shared.telling);
            if told.as_ref().is_some_and(|(id, _)| *id == head_id) {
                *told = None;
            }
        }
        let _ = std::fs::remove_file(queue.ticket_folder(&head_id).join("phone.json"));
        let (mut succeeded, mut message) = self.shared.device.install(text(head, "app"));
        if succeeded {
            // An app with the panel says its report on its output too: the launch stays to hear it.
            let heard: Option<Heard> = if hitl_host_core::truthy(head.get("overlay")) {
                let shared = self.shared.clone();
                Some(Arc::new(move |report| shared.hear(report)))
            } else {
                None
            };
            let arguments: Vec<String> = head["appArguments"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
            (succeeded, message) = self.shared.device.launch(text(head, "bundleId"), &arguments, head.get("payloadUrl").and_then(Value::as_str), heard);
        }
        let mut failed: Option<Value> = None;
        queue.locked(|state| {
            let Some(index) = state["queue"].as_array().into_iter().flatten().position(|ticket| text(ticket, "id") == head_id) else { return };
            if let Some(map) = state["queue"][index].as_object_mut() {
                map.remove("waitingFor");
                map.remove("probeSeenAt");
            }
            if !succeeded {
                let attempts = state["queue"][index].get("attempts").and_then(Value::as_i64).unwrap_or(0) + 1;
                state["queue"][index]["attempts"] = json!(attempts);
                let error = if message.is_empty() { "devicectl не смог поставить или запустить сборку".to_string() } else { message.clone() };
                state["queue"][index]["error"] = json!(error);
                if attempts < INSTALL_ATTEMPTS {
                    state["queue"][index]["waitingFor"] = json!(format!("не встала ({error}), попытка {}", attempts + 1));
                    return;
                }
                let mut ticket = crate::base::list_mut(state, "queue").remove(index);
                ticket["state"] = json!("failed");
                ticket["finishedAt"] = json!(now());
                let _ = std::fs::remove_dir_all(queue.ticket_folder(text(&ticket, "id")).join("app"));
                archive(state, ticket.clone());
                failed = Some(ticket);
            } else {
                let mut ticket = crate::base::list_mut(state, "queue").remove(index);
                ticket["state"] = json!("active");
                ticket["installedAt"] = json!(now());
                ticket["error"] = Value::Null;
                if let Some(map) = ticket.as_object_mut() {
                    for key in ["setAsideAt", "setAsideBy", "lostMarks"] {
                        map.remove(key);
                    }
                }
                state["active"] = ticket;
            }
            self.reconcile_actions(state);
        });
        if let Some(failed) = failed {
            self.notify_once(text(&failed, "id"), "failed", &format!("{}: сборка не встала", text(&failed, "tag")), text(&failed, "error"));
        } else if succeeded {
            // The same checklist comes up in the window on the Mac at once.
            self.shown_at = now();
            (self.show)(&queue, head);
        }
        // A build that got onto the phone is not announced on the Mac: the phone shows it, and he asked for these to
        // go on 2026-10-08 («можешь ещё эти уведы убрать»). Device blockers now have one persistent action in the Mac
        // window; a terminal install failure still has its notification.
    }

    pub fn run(&mut self) {
        let stop = Arc::new(AtomicBool::new(false));
        let shared = self.shared.clone();
        let flag = stop.clone();
        let hurrying = std::thread::spawn(move || shared.hurry(&flag));
        self.run_loop();
        stop.store(true, Ordering::SeqCst);
        let _ = hurrying.join();
        self.shared.device.detach();
    }

    fn run_loop(&mut self) {
        let mut idle = 0;
        loop {
            // The helper is edited while checks wait; a dispatcher that kept running the old code would ignore what
            // the new one wrote.
            if self.restart.is_some() && self.code_changed() {
                println!("{} the helper changed, restarting", bridge::timestamp());
                // The attached launch does not go over to the new process: left behind with nobody reading it, it
                // would pass the signal about that on to the app. The app's file is read instead from here on.
                self.shared.device.detach();
                (self.release)();
                if let Some(restart) = &self.restart {
                    restart();
                }
                return;
            }
            let busy = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.step(true))).unwrap_or_else(|_| {
                // The queue must outlive one bad pass.
                println!("{} step failed", bridge::timestamp());
                true
            });
            if busy {
                idle = 0;
            } else {
                idle += 1;
                // A check submitted a moment after the last one ended finds the dispatcher still here.
                if idle >= 3 {
                    let empty = self.shared.queue.locked(|state| {
                        state.get("active").is_none_or(Value::is_null) && state["queue"].as_array().is_none_or(Vec::is_empty)
                    });
                    if empty {
                        (self.release)();
                        return;
                    }
                    idle = 0;
                }
            }
            (self.sleep)(self.pause);
            self.waited += self.pause;
        }
    }
}

/// Puts the check that is on the phone into the window on the Mac as well: «было бы круто если бы проверка на
/// телефоне дублировалась плашкой на маке чтобы мне было проще писать комменты». What he marks there is kept in the
/// ticket's folder, `mac.json`; what the phone says is put beside it, `phone.json`, for the window to read.
pub fn show_in_window(queue: &Queue, ticket: &Value) {
    let path = window_ticket(text(ticket, "id"));
    if !path.exists() {
        let project = text(ticket, "project");
        let items: Vec<Value> = ticket["items"].as_array().into_iter().flatten().map(|item| json!({"label": item["label"], "text": item["text"]})).collect();
        let device = Some(queue.load()).and_then(|state| Some(text(&state, "name").to_string())).filter(|name| !name.is_empty()).unwrap_or_else(|| queue.udid.clone());
        let card = json!({
            "id": ticket["id"], "screen": "iphone", "device": device, "udid": queue.udid,
            "folder": queue.ticket_folder(text(ticket, "id")).display().to_string(), "tag": ticket["tag"],
            "chat": ticket.get("chat").cloned().filter(|chat| hitl_host_core::truthy(Some(chat))).unwrap_or(json!("")),
            "chatLink": ticket.get("chatLink").cloned().filter(|link| hitl_host_core::truthy(Some(link))).unwrap_or(json!("")),
            // The app the chat runs in: the window and the app on the phone say it by its icon, before the project.
            "agent": ticket.get("agent").cloned().filter(|agent| hitl_host_core::truthy(Some(agent))).unwrap_or(json!("")),
            "projectName": if project.is_empty() { String::new() } else { hitl_host_core::git::project_name(Some(project)) },
            "cwd": project, "title": ticket.get("title").cloned().filter(|title| hitl_host_core::truthy(Some(title))).unwrap_or(json!("")),
            "about": "", "kind": "checklist", "kindTitle": "на телефоне", "answers": ["работает", "не работает", "пропустить"],
            "project": project, "branch": ticket.get("branch").cloned().filter(|b| hitl_host_core::truthy(Some(b))).unwrap_or(json!("")),
            "commit": ticket.get("commit").cloned().filter(|c| hitl_host_core::truthy(Some(c))).unwrap_or(json!("")),
            "items": items,
            "createdAt": ticket.get("installedAt").cloned().filter(|at| hitl_host_core::truthy(Some(at))).unwrap_or_else(|| json!(now())),
        });
        let _ = hitl_host_core::write_json(&path, &card);
    }
    bridge::ensure_window();
}

/// The dispatcher of one phone, started by the first check and gone when the queue is empty.
pub fn command_dispatch(udid: &str) -> i32 {
    let queue = Queue::new(udid);
    let _ = std::fs::create_dir_all(&queue.folder);
    let Ok(Some(lock)) = FileLock::try_exclusive(&queue.folder.join("dispatcher.lock")) else { return 0 };
    unsafe { libc::signal(libc::SIGHUP, libc::SIG_IGN) };
    println!("{} dispatcher {} started", bridge::timestamp(), std::process::id());
    let held = Arc::new(Mutex::new(Some(lock)));
    let released = held.clone();
    let udid_again = udid.to_string();
    let mut dispatcher = Dispatcher::new(queue, real_device(udid))
        .with_release(move || {
            locked(&released).take();
        })
        .with_restart(move || {
            let error = std::process::Command::new(entry()).args(["_queues", "dispatch", "--udid", &udid_again]).exec();
            println!("{} the dispatcher did not restart: {error}", bridge::timestamp());
        });
    dispatcher.run();
    println!("{} dispatcher {} idle, exiting", bridge::timestamp(), std::process::id());
    drop(held);
    0
}

#[allow(dead_code)]
const _: i64 = STATE_VERSION;
