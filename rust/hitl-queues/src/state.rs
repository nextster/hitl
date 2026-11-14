//! The state of one phone's queue and what is read out of it (queues.py: `Queue`, `find_ticket`, `archive`,
//! `occupancy_lines`, `is_green`, ...). The state is a JSON value, as on disk: the Python that still runs reads the same
//! files, and so do commands that are already waiting.

use crate::base::{FileLock, STATE_VERSION, list_mut, number, process_alive, state_root, text};
use crate::base::now;
use hitl_host_core::{clock, dumps, entry, home, items_word, minutes, plural, read_json, write_json};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;

pub const RECENT_LIMIT: usize = 30;

/// The order and the words of a verdict, by the numbers of the items.
pub const STATE_GROUPS: [(Option<&str>, &str); 4] =
    [(Some("ok"), "Работает"), (Some("problem"), "Не работает"), (Some("skipped"), "Пропущено"), (None, "Без отметки")];

/// The state of one phone, read and changed under a file lock.
#[derive(Clone, Debug)]
pub struct Queue {
    pub udid: String,
    pub folder: PathBuf,
    pub name: String,
    pub core_id: String,
}

impl Queue {
    pub fn new(udid: &str) -> Queue {
        Queue::named(udid, "", "")
    }

    pub fn named(udid: &str, name: &str, core_id: &str) -> Queue {
        Queue { udid: udid.to_string(), folder: state_root().join(udid), name: name.to_string(), core_id: core_id.to_string() }
    }

    pub fn state_path(&self) -> PathBuf {
        self.folder.join("state.json")
    }

    pub fn ticket_folder(&self, ticket_id: &str) -> PathBuf {
        self.folder.join("tickets").join(ticket_id)
    }

    pub fn load(&self) -> Value {
        let mut state = match read_json(&self.state_path()) {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        let mut default = |key: &str, value: Value| {
            state.entry(key.to_string()).or_insert(value);
        };
        default("version", json!(STATE_VERSION));
        default("udid", json!(self.udid));
        default("name", json!(self.name));
        default("active", Value::Null);
        default("queue", json!([]));
        default("recent", json!([]));
        default("probe", Value::Null);
        default("paused", Value::Null);
        default("counter", json!(0));
        default("coreId", json!(self.core_id));
        if !self.name.is_empty() {
            state.insert("name".into(), json!(self.name));
        }
        if !self.core_id.is_empty() {
            state.insert("coreId".into(), json!(self.core_id));
        }
        Value::Object(state)
    }

    pub fn save(&self, state: &Value) {
        let _ = std::fs::create_dir_all(&self.folder);
        let temporary = self.folder.join("state.json.tmp");
        if std::fs::write(&temporary, dumps(state)).is_ok() {
            let _ = std::fs::rename(&temporary, self.state_path());
        }
    }

    /// Read, change and write back as one step: the file is written only if the state changed.
    pub fn locked<R>(&self, change: impl FnOnce(&mut Value) -> R) -> R {
        let _ = std::fs::create_dir_all(&self.folder);
        let _lock = FileLock::exclusive(&self.folder.join("state.lock"));
        let mut state = self.load();
        let before = state.clone();
        let result = change(&mut state);
        crate::private_launch_env::prune(self, &state);
        if state != before {
            self.save(&state);
        }
        result
    }

    // ---- the dispatcher's own lock

    pub fn dispatcher_alive(&self) -> bool {
        FileLock::held_elsewhere(&self.folder.join("dispatcher.lock"))
    }

    /// Starts the dispatcher unless one is running. A dispatcher gives up its lock only while it holds the state lock
    /// and sees nothing to do, so a caller that adds work and calls this under the state lock is never left with a
    /// dispatcher that is about to exit.
    pub fn ensure_dispatcher(&self) {
        if hitl_host_core::var("CONVEYOR_NO_DISPATCHER").as_deref() == Some("1") || self.dispatcher_alive() {
            return;
        }
        let _ = std::fs::create_dir_all(&self.folder);
        let Ok(log) = std::fs::OpenOptions::new().create(true).append(true).open(self.folder.join("dispatcher.log")) else { return };
        let Ok(errors) = log.try_clone() else { return };
        let mut child = std::process::Command::new(entry());
        crate::private_launch_env::configure_child(&mut child, &Default::default());
        child.args(["_queues", "dispatch", "--udid", &self.udid])
            .stdin(std::process::Stdio::null()).stdout(log).stderr(errors);
        // Its own session: the chat's tool call may end, time out or be killed with its process group, and the queue
        // has to keep moving.
        unsafe {
            child.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        let _ = child.spawn();
    }
}

/// The probe that is going on, if its process lives; a dead one is taken off the state.
pub fn live_probe(state: &mut Value) -> Option<Value> {
    let probe = state.get("probe").cloned().filter(|probe| !probe.is_null() && hitl_host_core::truthy(Some(probe)));
    match probe {
        Some(probe) if process_alive(probe.get("pid").and_then(Value::as_i64)) => Some(probe),
        Some(_) => {
            state["probe"] = Value::Null;
            None
        }
        None => None,
    }
}

/// The state of every ticket there is to look at: the active one, the queue, the recent ones last to first.
fn candidates(state: &Value) -> Vec<&Value> {
    let mut found: Vec<&Value> = Vec::new();
    if let Some(active) = state.get("active").filter(|active| active.is_object()) {
        found.push(active);
    }
    found.extend(state["queue"].as_array().into_iter().flatten());
    found.extend(state["recent"].as_array().into_iter().flatten().rev());
    found
}

/// A ticket by id or by its chat tag; the latest one for a tag.
pub fn find_ticket(state: &Value, key: Option<&str>) -> Option<Value> {
    let all = candidates(state);
    let Some(key) = key.filter(|key| !key.is_empty()) else { return all.first().map(|ticket| (*ticket).clone()) };
    if let Some(ticket) = all.iter().find(|ticket| text(ticket, "id") == key) {
        return Some((*ticket).clone());
    }
    let lowered = key.to_lowercase();
    all.iter().find(|ticket| text(ticket, "tag").to_lowercase() == lowered).map(|ticket| (*ticket).clone())
}

pub fn archive(state: &mut Value, ticket: Value) {
    let recent = list_mut(state, "recent");
    recent.push(ticket);
    if recent.len() > RECENT_LIMIT {
        let extra = recent.len() - RECENT_LIMIT;
        recent.drain(..extra);
    }
}

/// Whether a waiting check may go on the phone now. During a pause only the chats the pause names get through.
pub fn installable(state: &Value, ticket: &Value) -> bool {
    let paused = state.get("paused").filter(|paused| hitl_host_core::truthy(Some(paused)));
    hitl_host_core::truthy(ticket.get("urgent")) || paused.is_none_or(|paused| {
        paused.get("only").and_then(Value::as_array).is_some_and(|only| only.iter().any(|tag| tag.as_str() == Some(text(ticket, "tag"))))
    })
}

/// The check as the window on the Mac reads it, among the window's own.
pub fn window_ticket(ticket_id: &str) -> PathBuf {
    home().join("queue").join(format!("{ticket_id}.json"))
}

/// Takes the check that is on the phone off it and puts it back at the head of the queue, to be installed again in
/// its turn. Its marks on the phone are lost with the build.
pub fn set_aside(state: &mut Value, reason: &str) -> Option<Value> {
    let mut active = state.get("active").filter(|active| active.is_object())?.clone();
    let lost = active.get("progress").and_then(|progress| progress.get("marked")).cloned().unwrap_or(json!(0));
    let map = active.as_object_mut().expect("a ticket");
    map.insert("state".into(), json!("queued"));
    map.insert("installedAt".into(), Value::Null);
    map.insert("lostMarks".into(), lost);
    map.insert("progress".into(), json!({"marked": 0, "updatedAt": null}));
    map.insert("setAsideAt".into(), json!(now()));
    map.insert("setAsideBy".into(), json!(reason));
    state["active"] = Value::Null;
    let _ = std::fs::remove_file(window_ticket(text(&active, "id")));
    list_mut(state, "queue").insert(0, active.clone());
    Some(active)
}

pub fn pause_text(state: &Value) -> String {
    let Some(paused) = state.get("paused").filter(|paused| hitl_host_core::truthy(Some(paused))) else { return String::new() };
    let only: Vec<&str> = paused.get("only").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
    let scope = if only.is_empty() { "на телефон ничего не ставится".to_string() } else { format!("на телефон идёт только {}", only.join(", ")) };
    format!("Очередь на паузе с {}: {scope}. Паузу поставил Проверяющий, он же её снимет.", clock(paused.get("since").and_then(Value::as_f64)))
}

pub fn describe(ticket: &Value) -> String {
    let title = text(ticket, "title");
    let title = if title.is_empty() { String::new() } else { format!(" «{title}»") };
    format!("{}{title}", text(ticket, "tag"))
}

pub fn progress_text(ticket: &Value) -> String {
    if !hitl_host_core::truthy(ticket.get("overlay")) {
        return "без панели в приложении: закончит команда hitl iphone --queue-release".to_string();
    }
    let marked = ticket.get("progress").and_then(|progress| progress.get("marked")).and_then(Value::as_i64).unwrap_or(0);
    let total = ticket.get("items").and_then(Value::as_array).map_or(0, Vec::len);
    format!("отмечено {marked} из {total}")
}

/// What holds the phone and who waits, for a chat that has to wait.
pub fn occupancy_lines(state: &mut Value, own_id: Option<&str>) -> Vec<String> {
    let mut lines = Vec::new();
    let name = Some(text(state, "name")).filter(|name| !name.is_empty()).unwrap_or("телефон").to_string();
    if let Some(active) = state.get("active").filter(|active| active.is_object()) {
        if own_id != Some(text(active, "id")) {
            lines.push(format!("{name} занят проверкой {}: на телефоне с {}, {}. Освободится, когда Проверяющий закончит её на телефоне.",
                describe(active), clock(active.get("installedAt").and_then(Value::as_f64)), progress_text(active)));
        }
    }
    if let Some(probe) = live_probe(state) {
        let tag = Some(text(&probe, "tag")).filter(|tag| !tag.is_empty()).unwrap_or("?").to_string();
        lines.push(format!("{name} занят пробой чата {tag} с {}.", clock(probe.get("since").and_then(Value::as_f64))));
    }
    let mut ahead = Vec::new();
    for ticket in state["queue"].as_array().into_iter().flatten() {
        if own_id.is_some_and(|own| own == text(ticket, "id")) {
            break;
        }
        ahead.push(text(ticket, "tag").to_string());
    }
    if !ahead.is_empty() {
        lines.push(format!("{}{}.", if own_id.is_some() { "В очереди раньше: " } else { "В очереди: " }, ahead.join(", ")));
    }
    if hitl_host_core::truthy(state.get("paused")) {
        lines.push(pause_text(state));
    }
    lines
}

/// One line for the queue's status: how many items got which mark.
pub fn summarize(results: &[Value]) -> String {
    let parts: Vec<String> = STATE_GROUPS.iter().filter_map(|(mark, title)| {
        let count = results.iter().filter(|result| result.get("state").and_then(Value::as_str) == *mark).count();
        (count > 0).then(|| format!("{} {count}", title.to_lowercase()))
    }).collect();
    if parts.is_empty() { "пунктов нет".to_string() } else { parts.join(", ") }
}

/// How many items got which mark; "none" counts those left without one.
pub fn count_marks(results: &[Value]) -> Value {
    let mut counts = Map::new();
    for (mark, _) in STATE_GROUPS {
        let count = results.iter().filter(|result| result.get("state").and_then(Value::as_str) == mark).count();
        counts.insert(mark.unwrap_or("none").to_string(), json!(count));
    }
    Value::Object(counts)
}

/// Whether a finished check is the owner's go-ahead: he ended it himself with every item marked as working, which is
/// «Готово» with nothing but «работает», on the phone or in the window on the Mac that shows the same check.
pub fn is_green(entry: &Value) -> bool {
    match entry.get("how").and_then(Value::as_str) {
        Some("mac") if entry.get("ended").and_then(Value::as_str) != Some("done") => return false,
        Some("mac") | Some("phone") => {}
        _ => return false,
    }
    if let Some(marks) = entry.get("marks").filter(|marks| marks.is_object()) {
        let count = |mark: &str| marks.get(mark).and_then(Value::as_i64).unwrap_or(0);
        return count("ok") > 0 && ["problem", "skipped", "none"].iter().all(|mark| count(mark) == 0);
    }
    // Checks logged before the marks were counted: their summary names every group that has an item in it, the
    // unmarked ones too.
    let summary = text(entry, "summary");
    summary.strip_prefix("работает ").is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
}

/// "owner/name" of the GitHub repository a checkout belongs to, or "".
pub fn repository_of(project: &str) -> String {
    let options = hitl_host_core::process::Options::new("the origin of a checkout, to know whether it is on GitHub").timeout(5.0);
    let Ok(origin) = hitl_host_core::process::run("git", &["-C", project, "remote", "get-url", "origin"], &options) else { return String::new() };
    if !origin.success() {
        return String::new();
    }
    github_repository(&String::from_utf8_lossy(&origin.stdout))
}

/// `github\.com[:/]+([^/\s]+)/([^/\s]+?)(?:\.git)?/?\s*$` of the Python, searched in the text of a remote's address.
fn github_repository(address: &str) -> String {
    let address = address.trim_end();
    let mut from = 0;
    while let Some(found) = address[from..].find("github.com") {
        let rest = address[from + found + "github.com".len()..].trim_start_matches([':', '/']);
        let consumed_separator = rest.len() < address[from + found + "github.com".len()..].len();
        from += found + 1;
        if !consumed_separator {
            continue;
        }
        let Some((owner, name)) = rest.split_once('/') else { continue };
        let name = name.strip_suffix('/').unwrap_or(name);
        let name = name.strip_suffix(".git").unwrap_or(name);
        if !owner.is_empty() && !owner.contains(char::is_whitespace) && !name.is_empty() && !name.contains(['/', ' ', '\t', '\n']) {
            return format!("{owner}/{name}");
        }
    }
    String::new()
}

/// Whether an ended check was asked from a checkout of the repository; any, when none is named.
fn of_repository(entry: &Value, repository: Option<&str>, known: &mut HashMap<String, String>) -> bool {
    let Some(repository) = repository else { return true };
    hitl::common::asked_in(entry, repository, known)
}

/// The check of a branch that left a phone last; of a build of that repository, when one is named.
pub fn last_check(branch: &str, repository: Option<&str>) -> Option<Value> {
    let mut found: Option<Value> = None;
    let mut known = HashMap::new();
    if branch.is_empty() {
        return None;
    }
    for entry in hitl_host_core::history() {
        if hitl::common::screen_of(&entry) == "iphone" && text(&entry, "branch") == branch
            && found.as_ref().is_none_or(|best| number(&entry, "finishedAt") >= number(best, "finishedAt"))
            && of_repository(&entry, repository, &mut known) {
            found = Some(entry);
        }
    }
    found
}

/// The text of the "waiting" notice for a chat; and some helpers for the commands: the plural of a number of marks.
pub fn marks_word(count: i64) -> &'static str {
    plural(count, "отметка", "отметки", "отметок")
}

pub fn items_text(ticket: &Value) -> String {
    items_word(ticket["items"].as_array().map_or(0, Vec::len) as i64)
}

pub fn held_for(seconds: f64) -> String {
    minutes(seconds)
}

pub fn write_state_json(path: &std::path::Path, value: &Value) {
    let _ = write_json(path, value);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::testing::Sandbox;

    #[test]
    fn a_state_that_is_not_there_has_its_defaults() {
        let _sandbox = Sandbox::new("state-defaults");
        let queue = Queue::named("UDID-TEST", "Test iPhone", "");
        let state = queue.load();
        assert_eq!(state["queue"], json!([]));
        assert_eq!(state["active"], Value::Null);
        assert_eq!(state["name"], "Test iPhone");
        assert_eq!(state["version"], 1);
    }

    #[test]
    fn locked_writes_only_what_changed() {
        let _sandbox = Sandbox::new("state-locked");
        let queue = Queue::new("UDID-TEST");
        queue.locked(|_| {});
        assert!(!queue.state_path().exists());
        queue.locked(|state| state["counter"] = json!(5));
        assert_eq!(queue.load()["counter"], 5);
    }

    #[test]
    fn green_is_the_owners_go_ahead_and_nothing_else() {
        let green = json!({"how": "phone", "marks": {"ok": 2, "problem": 0, "skipped": 0, "none": 0}});
        assert!(is_green(&green));
        assert!(!is_green(&json!({"how": "phone", "marks": {"ok": 2, "problem": 1, "skipped": 0, "none": 0}})));
        assert!(!is_green(&json!({"how": "command", "marks": {"ok": 2}})));
        assert!(is_green(&json!({"how": "mac", "ended": "done", "marks": {"ok": 1}})));
        assert!(!is_green(&json!({"how": "mac", "ended": "rework", "marks": {"ok": 1}})));
        assert!(is_green(&json!({"how": "phone", "summary": "работает 3"})));
        assert!(!is_green(&json!({"how": "phone", "summary": "работает 3, пропущено 1"})));
    }

    #[test]
    fn a_ticket_is_found_by_its_id_or_the_tag_of_its_chat() {
        let state = json!({"active": {"id": "a1", "tag": "R/38"}, "queue": [{"id": "q1", "tag": "r/40"}],
                           "recent": [{"id": "o1", "tag": "r/40"}, {"id": "o2", "tag": "r/50"}]});
        assert_eq!(find_ticket(&state, Some("q1")).unwrap()["id"], "q1");
        assert_eq!(find_ticket(&state, Some("r/38")).unwrap()["id"], "a1");
        assert_eq!(find_ticket(&state, Some("r/40")).unwrap()["id"], "q1");
        assert_eq!(find_ticket(&state, None).unwrap()["id"], "a1");
        assert!(find_ticket(&state, Some("none")).is_none());
    }

    #[test]
    fn what_holds_the_phone_is_told_to_a_waiting_chat() {
        let _sandbox = Sandbox::new("state-occupancy");
        let mut state = json!({"name": "Test iPhone", "active": {"id": "a1", "tag": "r/38", "title": "Бары", "items": [1, 2],
                               "overlay": true, "progress": {"marked": 1}, "installedAt": null},
                               "queue": [{"id": "q1", "tag": "r/40"}, {"id": "q2", "tag": "r/41"}], "recent": [], "probe": null, "paused": null});
        let lines = occupancy_lines(&mut state, Some("q2"));
        assert!(lines[0].starts_with("Test iPhone занят проверкой r/38 «Бары»: на телефоне с —, отмечено 1 из 2."), "{lines:?}");
        assert_eq!(lines[1], "В очереди раньше: r/40.");
    }
}
