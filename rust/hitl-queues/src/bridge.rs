//! What the queue asks of the window's part (check.py, tell.py), which another session owns and moves to `hitl`.
//!
//! One place for every such call, under the names the Python had: when `hitl` has a function for one, this file calls
//! it; what it lacks (the typed actions of the queue in the window) is asked of the Python that still runs. In the
//! tests the calls that reach chats and the window are replaced by recorders: they are not this part's to test.

use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Mutex;

// ---- the window's folders

pub fn window_queue_folder() -> PathBuf {
    hitl_host_core::home().join("queue")
}

pub fn window_done_folder() -> PathBuf {
    hitl_host_core::home().join("done")
}

/// The checks that wait in the window, in their order (check.queued): a check that is on the phone is shown there too
/// but it is the phone's, its own queue keeps it.
pub fn window_queued() -> Vec<Value> {
    let mut tickets: Vec<Value> = Vec::new();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(window_queue_folder()).into_iter().flatten().flatten()
        .map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|ext| ext == "json")).collect();
    paths.sort();
    for path in paths {
        if let Some(ticket) = hitl_host_core::read_json(&path) {
            if ticket.is_object() && hitl_host_core::truthy(ticket.get("id")) && ticket.get("screen") != Some(&json!("iphone")) {
                tickets.push(ticket);
            }
        }
    }
    let created = |ticket: &Value| ticket.get("createdAt").and_then(Value::as_f64).unwrap_or(0.0);
    tickets.sort_by(|left, right| {
        created(left).partial_cmp(&created(right)).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left["id"].as_str().unwrap_or("").cmp(right["id"].as_str().unwrap_or("")))
    });
    tickets
}

/// The id of the window's card that stands for one blocked phone check (check.automatic_action_id).
pub fn automatic_action_id(owner: &Value) -> String {
    use sha2::{Digest, Sha256};
    let mut sorted = serde_json::Map::new();
    let mut keys: Vec<&String> = owner.as_object().map(|map| map.keys().collect()).unwrap_or_default();
    keys.sort();
    for key in keys {
        sorted.insert(key.clone(), owner[key.as_str()].clone());
    }
    let digest = Sha256::digest(hitl_host_core::dumps_line(&Value::Object(sorted)).as_bytes());
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("iphone-action-{}", &hex[..24])
}

/// A stable phone-check identity has one card and one terminal receipt (check.submit_automatic_action). No
/// messenger: this action belongs to the queue, not to a new agent request.
pub fn submit_automatic_action(owner: &Value, ticket: &Value) -> bool {
    let ticket_id = automatic_action_id(owner);
    if window_done_folder().join(format!("{ticket_id}.json")).exists() {
        return false;
    }
    let path = window_queue_folder().join(format!("{ticket_id}.json"));
    if !path.exists() {
        let mut card = ticket.clone();
        card["id"] = json!(ticket_id);
        card["automaticAction"] = owner.clone();
        let _ = hitl_host_core::write_json(&path, &card);
    }
    true
}

/// Withdraws only a typed queue-owned action (check.resolve_automatic_action); its edits stay as technical evidence.
/// This is not a user verdict, approval or the public drop API. A live panel serializes it with delivered edits; an
/// older panel fails closed.
pub fn resolve_automatic_action(owner: &Value, reason: &str) -> bool {
    let ticket_id = automatic_action_id(owner);
    let path = window_queue_folder().join(format!("{ticket_id}.json"));
    let Some(ticket) = hitl_host_core::read_json(&path).filter(Value::is_object) else { return true };
    if ticket.get("kind") != Some(&json!("action")) || ticket.get("automaticAction") != Some(owner) || owner.get("type") != Some(&json!("iphone-blocked")) {
        return false;
    }
    let mut verdict = ticket.clone();
    verdict["screen"] = json!("mac");
    verdict["how"] = json!("automatic-resolved");
    verdict["technicalReason"] = json!(reason);
    verdict["results"] = json!([]);
    verdict["finishedAt"] = json!(crate::base::now());
    let window = hitl_host_core::home().join("window");
    let _ = std::fs::create_dir_all(&window);
    let Ok(lock) = crate::base::FileLock::try_exclusive(&window.join("panel.lock")) else { return false };
    let Some(_lock) = lock else {
        return matches!(drop_through_panel(&ticket_id, &verdict).as_str(), "dropped" | "missing");
    };
    let answers = window.join("answers.json");
    if answers.exists() {
        let saved = hitl_host_core::read_json(&answers);
        let Some(checks) = saved.as_ref().and_then(|saved| saved.get("checks")).and_then(Value::as_object) else { return false };
        let held = checks.get(&ticket_id).cloned().unwrap_or(json!({}));
        if !held.is_object() || !held.get("marks").unwrap_or(&json!([])).is_array() {
            return false;
        }
        verdict["results"] = held.get("marks").cloned().unwrap_or(json!([]));
    }
    if !hitl::queue::claim_done(&ticket_id, &verdict) {
        let previous = hitl_host_core::read_json(&window_done_folder().join(format!("{ticket_id}.json"))).filter(Value::is_object);
        match previous {
            Some(previous) if previous.get("how") == Some(&json!("automatic-resolved")) && previous.get("automaticAction") == Some(owner)
                && previous.get("results") == verdict.get("results") => verdict = previous, // recover a crash between claim and removal
            _ => return false, // a real answer, or newer edits, won; never overwrite it
        }
    }
    if !hitl::common::history().iter().any(|row| row.get("id") == Some(&json!(ticket_id))) {
        hitl::common::record(&verdict);
    }
    let _ = std::fs::remove_file(path);
    true
}

/// The live panel is the only authority: it serializes this request with marks and comments on its main loop. Old or
/// unresponsive panels fail closed ("unknown").
fn drop_through_panel(ticket_id: &str, verdict: &Value) -> String {
    let token = format!("{:032x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()) ^ ((std::process::id() as u128) << 96));
    let folder = hitl_host_core::home().join("window").join("drops");
    let _ = std::fs::create_dir_all(&folder);
    let (request, reply) = (folder.join(format!("{token}.request.json")), folder.join(format!("{token}.reply.json")));
    let _ = hitl_host_core::write_json(&request, &json!({"token": token, "id": ticket_id, "deadline": crate::base::now() + 3.0, "verdict": verdict}));
    let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let mut outcome = "unknown".to_string();
    while std::time::Instant::now() < until {
        if let Some(response) = hitl_host_core::read_json(&reply).filter(Value::is_object) {
            if response.get("token") == Some(&json!(token)) && response.get("id") == Some(&json!(ticket_id)) {
                outcome = response.get("outcome").and_then(Value::as_str).unwrap_or("unknown").to_string();
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let _ = std::fs::remove_file(request);
    let _ = std::fs::remove_file(reply);
    outcome
}

/// The kind's title and answers as the window's card is made (check.KINDS, check.ANSWERS).
pub fn action_kind() -> (&'static str, Vec<&'static str>) {
    (hitl::submit::KINDS.iter().find(|(kind, _)| *kind == "action").map_or("действие", |(_, title)| title), hitl::submit::answers("action").to_vec())
}

// ---- bringing the window up

static BRINGING_UP: Mutex<Vec<std::thread::JoinHandle<()>>> = Mutex::new(Vec::new());
static RAISING: Mutex<()> = Mutex::new(());

/// The window up beside the queue, not by it: after a change of its source it is built first, most of a minute, and
/// twice that minute was the queue's, with nothing he marked on either screen reaching the other
/// («очень долгий коннект»). And a window that cannot be built or started must not stop the phone's queue.
pub fn ensure_window() {
    // No window is wanted, as in a run of the tests: then nothing is started that could outlive the made-up home it
    // was started in and find the real one.
    if hitl_host_core::var("CONVEYOR_WINDOW").as_deref() == Some("") {
        return;
    }
    let thread = std::thread::spawn(|| {
        // One at a time: the next look at the window finds the one before it still building.
        let Ok(_raising) = RAISING.try_lock() else { return };
        #[cfg(not(test))]
        {
            let _ = std::fs::create_dir_all(window_queue_folder());
            let _ = std::fs::create_dir_all(window_done_folder());
            let attempt = std::panic::catch_unwind(hitl::window::ensure_panel);
            if attempt.is_err() {
                println!("{} the window on the Mac did not come up", timestamp());
            }
        }
    });
    let mut running = BRINGING_UP.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    running.retain(|one| !one.is_finished());
    running.push(thread);
}

/// Waits for the threads that bring the window up: a run of the tests does, before it gives its made-up home back.
pub fn join_window_threads() {
    let threads: Vec<_> = std::mem::take(&mut *BRINGING_UP.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
    for thread in threads {
        let _ = thread.join();
    }
}

/// "2026-10-29T12:00:00" for the log lines the dispatcher writes.
pub fn timestamp() -> String {
    let (year, month, day, hour, minute, second) = hitl::common::local(hitl_host_core::now());
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}")
}

// ---- chats: what tell.py does for a check on the phone

/// Which chat queued a check, kept until it has read the verdict (tell.remember(ticket, verdict=PATH)): a check on the
/// phone says where its verdict will lie, the phone's queue keeps it and not the window, and the record is written
/// with it at once.
pub fn tell_remember(ticket: &Value, verdict: &std::path::Path) {
    #[cfg(not(test))]
    hitl::tell::remember_with(ticket, Some(verdict));
    #[cfg(test)]
    {
        let _ = verdict;
        recorded().push(("remember".into(), ticket["id"].as_str().unwrap_or("").to_string()));
    }
}

pub fn tell_printed(ticket_id: &str) {
    #[cfg(not(test))]
    hitl::tell::printed(ticket_id, true);
    #[cfg(test)]
    recorded().push(("printed".into(), ticket_id.to_string()));
}

/// The chats this process runs in, by the ids their apps gave (tell.chat_ids).
pub fn chat_ids() -> Vec<String> {
    #[cfg(not(test))]
    return hitl::tell::chat_ids(&hitl_host_core::env());
    #[cfg(test)]
    return Vec::new();
}

/// A chat nothing wakes: whom to tell (tell.addressee), and tell.start_phone.
pub fn addressee() -> Option<Value> {
    #[cfg(not(test))]
    return hitl::tell::addressee(&hitl_host_core::env());
    #[cfg(test)]
    return standins().addressee.clone();
}

/// tell.start_phone: whom to tell, and which phone's queue will have the verdict.
pub fn start_phone(ticket: &Value, who: &Value, udid: &str) -> bool {
    #[cfg(not(test))]
    {
        let mut ticket = ticket.clone();
        ticket["cwd"] = json!(std::env::current_dir().map(|path| path.display().to_string()).unwrap_or_default());
        let mut who = who.clone();
        who["phone"] = json!(udid);
        return hitl::tell::start(&ticket, &who);
    }
    #[cfg(test)]
    {
        let mut chat = standins();
        let accepted = chat.starts;
        chat.started.push((ticket["id"].as_str().unwrap_or("").to_string(), who.clone(), udid.to_string()));
        accepted
    }
}

pub fn tell_sweep() {
    #[cfg(not(test))]
    hitl::tell::sweep();
}

pub fn tell_left_waiting(ticket_id: &str) -> String {
    #[cfg(not(test))]
    return hitl::tell::left_waiting(ticket_id);
    #[cfg(test)]
    {
        let _ = ticket_id;
        standins().left_waiting.clone()
    }
}

pub fn tell_note(who: &Value, ticket_id: &str) -> String {
    #[cfg(not(test))]
    return hitl::tell::note(who, ticket_id);
    #[cfg(test)]
    return format!("tell {who} {ticket_id}");
}

/// The chat that gets its verdict by itself, if the check is told by a messenger (tell.comes_by_itself).
pub fn tell_comes_by_itself(ticket_id: &str) -> Option<Value> {
    #[cfg(not(test))]
    return hitl::tell::comes_by_itself(ticket_id);
    #[cfg(test)]
    {
        let _ = ticket_id;
        standins().by_itself.clone()
    }
}

/// An event for the journal (journal.emit).
pub fn emit(kind: &str, fields: Value) {
    hitl::common::emit(kind, fields);
}

#[cfg(test)]
static RECORDED: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// What the tests' stand-ins for the chats were asked, in order.
#[cfg(test)]
pub fn recorded() -> std::sync::MutexGuard<'static, Vec<(String, String)>> {
    RECORDED.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// What the stand-ins for tell.py answer in a test, and what they were asked: whom the chat that runs the command is
/// told by (nobody, unless a test says), whether `start_phone` takes it, and which chat gets its verdict by itself.
#[cfg(test)]
#[derive(Default)]
pub struct Standins {
    pub addressee: Option<Value>,
    pub starts: bool,
    pub by_itself: Option<Value>,
    pub left_waiting: String,
    /// (ticket, whom, phone) of every `start_phone`.
    pub started: Vec<(String, Value, String)>,
}

#[cfg(test)]
static STANDINS: Mutex<Option<Standins>> = Mutex::new(None);

#[cfg(test)]
pub struct StandinsGuard(std::sync::MutexGuard<'static, Option<Standins>>);

#[cfg(test)]
impl std::ops::Deref for StandinsGuard {
    type Target = Standins;
    fn deref(&self) -> &Standins {
        self.0.as_ref().expect("made on the way in")
    }
}

#[cfg(test)]
impl std::ops::DerefMut for StandinsGuard {
    fn deref_mut(&mut self) -> &mut Standins {
        self.0.as_mut().expect("made on the way in")
    }
}

#[cfg(test)]
pub fn standins() -> StandinsGuard {
    let mut guard = STANDINS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.get_or_insert_with(Standins::default);
    StandinsGuard(guard)
}

/// Gives the stand-ins back as they were at the start: a test run ends with it.
#[cfg(test)]
pub fn reset_standins() {
    *STANDINS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    recorded().clear();
}
