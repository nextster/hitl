//! The queue of the window: what waits, a wait for one check, taking one back. The port of the queue part of
//! `src/hitl/check.py` and of the window's side of `src/hitl/checks.py` and `src/hitl/cli.py`.
//! OWNER: part B.
//!
//! hitl's own `hitl queue`, `wait`, `drop` and `last-check` add the phone and the lines of turns to what is
//! said here; this is the window's side alone, for hitl and for hitl alike.
#![allow(dead_code)]

use crate::common::{self, say, truthy};
use crate::verdict::{self, py_str};
use crate::{tell, window};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// Verdicts and the screenshots of their comments are kept two weeks.
pub const KEEP_DONE_SECONDS: f64 = 14.0 * 24.0 * 3600.0;
/// The check was dropped, or there is no such check.
pub const EXIT_NOT_CHECKED: i32 = 2;
/// The check is queued and not waited for.
pub const EXIT_QUEUED: i32 = 75;

// --------------------------------------------------------------------------
// Files

/// The files of a folder whose names end with `ending`, hidden ones too, as pathlib's glob finds them.
fn files_ending(folder: &Path, ending: &str) -> Vec<PathBuf> {
    common::sorted_dir(folder).into_iter()
        .filter(|path| path.file_name().is_some_and(|name| name.to_string_lossy().ends_with(ending)))
        .collect()
}

fn modified(path: &Path) -> Option<f64> {
    let time = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(time.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_secs_f64())
}

fn ticket_file(ticket_id: &str) -> PathBuf {
    window::queue_folder().join(format!("{ticket_id}.json"))
}

fn verdict_file(ticket_id: &str) -> PathBuf {
    window::done_folder().join(format!("{ticket_id}.json"))
}

/// What `read_json(...) or {}` gives a reader that asks it with `.get`.
fn object_or_empty(value: Option<Value>) -> Value {
    match value {
        Some(found @ Value::Object(_)) => found,
        _ => Value::Object(Map::new()),
    }
}

/// `value.get(key, default)` said as a text: the default only for a key that is not there.
fn text_or(value: &Value, key: &str, default: &str) -> String {
    match value.get(key) {
        None => default.to_string(),
        Some(found) => py_str(found),
    }
}

/// `value or ""` said as a text.
fn text_or_empty(value: Option<&Value>) -> String {
    if truthy(value) { py_str(value.unwrap_or(&Value::Null)) } else { String::new() }
}

/// A time as a moment the clock says: a number, "—" for none.
fn moment(value: Option<&Value>) -> Option<f64> {
    value.and_then(Value::as_f64)
}

fn print_out(text: &str) -> bool {
    let mut out = std::io::stdout().lock();
    writeln!(out, "{text}").and_then(|_| out.flush()).is_ok()
}

// --------------------------------------------------------------------------
// The queue

/// The folders made, and the messengers that were lost started again (check.prepare).
pub fn prepare() {
    for folder in [window::queue_folder(), window::done_folder(), window::window_folder()] {
        let _ = std::fs::create_dir_all(folder);
    }
    tell::sweep();
}

/// A ticket id from an id, a tag or nothing: the caller's own newest (check.find).
pub fn find(target: Option<&str>, own_tag: &str) -> Option<String> {
    let target = target.filter(|target| !target.is_empty());
    let tickets = window::queued();
    let mut finished = files_ending(&window::done_folder(), ".json");
    finished.sort_by(|one, other| modified(other).unwrap_or(0.0).total_cmp(&modified(one).unwrap_or(0.0)));
    if let Some(target) = target {
        if tickets.iter().any(|ticket| common::text_of(ticket, "id") == target) || verdict_file(target).exists() {
            return Some(target.to_string());
        }
    }
    let wanted = target.unwrap_or(own_tag);
    for ticket in tickets.iter().rev() {
        if ticket.get("tag").and_then(Value::as_str) == Some(wanted) {
            return ticket.get("id").filter(|id| !id.is_null()).map(py_str);
        }
    }
    for path in finished {
        let Some(verdict) = common::read_json(&path).filter(Value::is_object) else { continue };
        if verdict.get("tag").and_then(Value::as_str) == Some(wanted) {
            return verdict.get("id").filter(|id| !id.is_null()).map(py_str);
        }
    }
    None
}

/// A check of the window by its number or its chat; the caller's own newest when none is named (check.own).
pub fn own(target: Option<&str>) -> Option<String> {
    find(target, &common::chat_tag())
}

/// The check itself, waiting or (unless `waiting`) ended (checks.in_window).
pub fn in_window(target: Option<&str>, waiting: bool) -> Option<Value> {
    let ticket_id = own(target)?;
    let mut ticket = common::read_json(&ticket_file(&ticket_id));
    if !ticket.as_ref().is_some_and(Value::is_object) && !waiting {
        ticket = common::read_json(&verdict_file(&ticket_id));
    }
    ticket.filter(Value::is_object)
}

/// Waits for one check and prints its verdict (check.wait_for).
pub fn wait_for(ticket_id: &str, acknowledge: bool) -> i32 {
    let mut position_said: Option<String> = None;
    loop {
        if let Some(verdict) = common::read_json(&verdict_file(ticket_id)).filter(Value::is_object) {
            // A verdict the chat did not get is not read: a pipe that closed leaves it to the next look.
            if !print_out(&verdict::verdict_text(&verdict, false)) {
                return 1;
            }
            tell::printed(ticket_id, acknowledge);
            return if verdict.get("how").and_then(Value::as_str) == Some("dropped") { EXIT_NOT_CHECKED } else { 0 };
        }
        let ids: Vec<String> = window::queued().iter().map(|ticket| common::text_of(ticket, "id").to_string()).collect();
        let Some(index) = ids.iter().position(|id| id == ticket_id) else {
            std::thread::sleep(Duration::from_secs(1)); // the window writes the verdict a moment before it removes the ticket
            if verdict_file(ticket_id).exists() {
                continue;
            }
            say(&format!("{}: проверки {ticket_id} нет ни в очереди, ни среди законченных.", common::tool()));
            return EXIT_NOT_CHECKED;
        };
        window::ensure_panel();
        let progress = object_or_empty(common::read_json(&window::progress_file()));
        let state = if progress.get("id").and_then(Value::as_str) == Some(ticket_id) {
            format!("в окне: отмечено {} из {}", window::answered(&progress), text_or(&progress, "total", "?"))
        } else {
            format!("в очереди, место {}", index + 1)
        };
        if let Some(who) = tell::comes_by_itself(ticket_id) {
            // A chat nothing wakes would stand here and ask again and again; its verdict comes to it as a message.
            say(&format!("==> Проверка {ticket_id}: {state}. {} её ещё не {}.", common::who(), common::did("закончил")));
            say(&tell::note(&who, ticket_id));
            return EXIT_QUEUED;
        }
        if position_said.as_deref() != Some(state.as_str()) {
            say(&format!("==> Проверка {ticket_id}: {state}. Жду, пока {} закончит её в окне на Маке.", common::who()));
            position_said = Some(state);
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

/// Old verdicts go, and the screenshots of their comments with them (check.clean_done).
pub fn clean_done() {
    let now = common::now();
    let old = files_ending(&window::done_folder(), ".json").into_iter().chain(files_ending(&window::shots_folder(), ".png"));
    for path in old {
        if modified(&path).is_some_and(|time| now - time > KEEP_DONE_SECONDS) {
            let _ = std::fs::remove_file(&path);
        }
    }
    tell::forget_old(KEEP_DONE_SECONDS);
}

/// What is in the window and what waits for it (check.queue_lines).
pub fn queue_lines() -> Vec<String> {
    let tickets = window::queued();
    let progress = object_or_empty(common::read_json(&window::progress_file()));
    let mut lines = vec!["Окно проверок на Маке".to_string()];
    if tickets.is_empty() {
        lines.push("  Сейчас: проверок нет".into());
    }
    for (number, ticket) in tickets.iter().enumerate() {
        let kind = verdict::kind_title(ticket.get("kind").and_then(Value::as_str));
        // Which of his projects, first, as in the window.
        let project = if truthy(ticket.get("projectName")) { format!("{} · ", py_str(&ticket["projectName"])) } else { String::new() };
        let created = match ticket.get("createdAt") {
            None => Some(common::now()),
            found => moment(found),
        };
        let tag = if truthy(ticket.get("tag")) { py_str(&ticket["tag"]) } else { "чат".into() };
        let line = format!("{project}{tag} «{}» · {kind} · с {}", text_or(ticket, "title", ""), common::clock(created));
        if progress.get("id").is_some() && progress.get("id") == ticket.get("id") {
            lines.push(format!("  Сейчас: {line} · отмечено {} из {}", window::answered(&progress), text_or(&progress, "total", "?")));
        } else {
            lines.push(format!("  {}: {line}", if number == 0 { "Сейчас" } else { "Ждёт" }));
        }
    }
    let history: Vec<Value> = common::history().into_iter().filter(|entry| common::screen_of(entry) == "mac").collect();
    let recent = &history[history.len().saturating_sub(5)..];
    if !recent.is_empty() {
        lines.push("  Недавние:".into());
        for verdict in recent.iter().rev() {
            let project = if truthy(verdict.get("projectName")) { format!("{} · ", py_str(&verdict["projectName"])) } else { String::new() };
            let tag = if truthy(verdict.get("tag")) { py_str(&verdict["tag"]) } else { "чат".into() };
            let finished = match verdict.get("finishedAt") {
                None => Some(common::now()),
                found => moment(found),
            };
            lines.push(format!("    {project}{tag} «{}» · {} · {}", text_or(verdict, "title", ""), common::clock(finished),
                               verdict::summary(verdict)));
        }
    }
    lines
}

/// `hitl queue`: the answers the calling chat has not read, then the window (checks.show, the window's side).
pub fn show() -> i32 {
    let unread = tell::reminders(&tell::chat_ids(&common::env()));
    if !unread.is_empty() {
        // First, what the person has answered to the calling chat and it has not read: the wake can be lost.
        print_out(&format!("{}\n", unread.join("\n")));
    }
    print_out(&queue_lines().join("\n"));
    0
}

/// `hitl wait [ID|TAG]` (checks.wait, the window's side).
pub fn wait(target: Option<&str>) -> i32 {
    let target = target.filter(|target| !target.is_empty());
    if let Some(ticket) = in_window(target, false) {
        return wait_for(common::text_of(&ticket, "id"), true);
    }
    let place = if common::is_host() { "ни в окне на Маке, ни на телефоне" } else { "в окне на Маке" };
    let said = match target {
        Some(_) => format!("такой проверки нет {place}."),
        None => "у этого чата нет проверки; назови её номер или метку чата.".to_string(),
    };
    say(&format!("{}: {said} Очередь: {}", common::tool(), common::command(&["queue"])));
    EXIT_NOT_CHECKED
}

// --------------------------------------------------------------------------
// Taking a check back

/// Whether the window keeps answers of the person for a check: an answer, a choice, a comment, a page, a screenshot.
fn has_saved_answers(ticket_id: &str) -> bool {
    let path = window::window_folder().join("answers.json");
    if !path.exists() {
        return false;
    }
    let saved = common::read_json(&path);
    let Some(checks) = saved.as_ref().and_then(|saved| saved.get("checks")).and_then(Value::as_object) else {
        return true; // Unknown saved state is not permission to discard it.
    };
    let Some(kept) = checks.get(ticket_id).filter(|kept| !kept.is_null()) else { return false };
    let Some(marks) = kept.get("marks").and_then(Value::as_array) else { return true };
    let set = |mark: &Value, key: &str| mark.get(key).is_some_and(|value| !value.is_null());
    let yes = |mark: &Value, key: &str| mark.get(key) == Some(&Value::Bool(true));
    marks.iter().any(|mark| {
        !mark.is_object() || set(mark, "state") || set(mark, "choice") || truthy(mark.get("choices")) || yes(mark, "other")
            || yes(mark, "unclear") || truthy(mark.get("note")) || set(mark, "values") || truthy(mark.get("shots"))
    })
}

/// A token as uuid4().hex writes one: 32 hexadecimal digits, of the version 4.
fn token() -> String {
    let mut bytes = [0u8; 16];
    let read = std::fs::File::open("/dev/urandom").and_then(|mut random| random.read_exact(&mut bytes));
    if read.is_err() {
        use sha2::Digest;
        let seed = format!("{}-{}-{:?}", common::now(), std::process::id(), Instant::now());
        bytes.copy_from_slice(&sha2::Sha256::digest(seed.as_bytes())[..16]);
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The live panel is the only authority while it runs: it serializes this request with marks and comments on its
/// main loop. An old or unresponsive panel fails closed (check._drop_through_panel).
fn drop_through_panel(ticket_id: &str, verdict: &Value) -> String {
    let token = token();
    let folder = window::window_folder().join("drops");
    let _ = std::fs::create_dir_all(&folder);
    let request = folder.join(format!("{token}.request.json"));
    let reply = folder.join(format!("{token}.reply.json"));
    let outcome = (|| {
        if common::write_json(&request, &json!({"token": token, "id": ticket_id, "deadline": common::now() + 3.0, "verdict": verdict})).is_err() {
            return "unknown".to_string();
        }
        let until = Instant::now() + Duration::from_secs(3);
        while Instant::now() < until {
            if let Some(response) = common::read_json(&reply).filter(Value::is_object) {
                if response.get("token").and_then(Value::as_str) == Some(token.as_str())
                    && response.get("id").and_then(Value::as_str) == Some(ticket_id) {
                    return text_or(&response, "outcome", "unknown");
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        "unknown".to_string()
    })();
    let _ = std::fs::remove_file(&request);
    let _ = std::fs::remove_file(&reply);
    outcome
}

/// Writes the one verdict of a check, unless one is there already: the window writes its own the same way, and the
/// first stands (check.claim_done).
pub fn claim_done(ticket_id: &str, verdict: &Value) -> bool {
    let folder = window::done_folder();
    let _ = std::fs::create_dir_all(&folder);
    let aside = folder.join(format!(".{ticket_id}.{}.claim", std::process::id()));
    if std::fs::write(&aside, common::dumps(verdict)).is_err() {
        let _ = std::fs::remove_file(&aside);
        return false;
    }
    let linked = std::fs::hard_link(&aside, verdict_file(ticket_id));
    let _ = std::fs::remove_file(&aside);
    linked.is_ok()
}

/// What a check taken back ends with: who asked and what, and no answer (check.drop).
fn dropped_verdict(ticket: &Value) -> Value {
    let mut verdict = Map::new();
    for key in ["id", "tag", "chat", "agent", "projectName", "title", "kind", "project", "branch", "commit", "createdAt"] {
        verdict.insert(key.into(), ticket.get(key).cloned().unwrap_or(Value::Null));
    }
    verdict.insert("screen".into(), json!("mac"));
    let results: Vec<Value> = common::list_of(ticket, "items").iter().map(|item| {
        let mut entry = Map::new();
        entry.insert("label".into(), item.get("label").cloned().unwrap_or(Value::Null));
        entry.insert("state".into(), Value::Null);
        entry.insert("choice".into(), Value::Null);
        entry.insert("note".into(), json!(""));
        entry.insert("text".into(), item.get("text").cloned().unwrap_or(Value::Null));
        if truthy(item.get("kind")) {
            entry.insert("kind".into(), item["kind"].clone());
        }
        Value::Object(entry)
    }).collect();
    verdict.insert("results".into(), Value::Array(results));
    verdict.insert("how".into(), json!("dropped"));
    verdict.insert("shownAt".into(), json!(0));
    verdict.insert("finishedAt".into(), json!(common::now()));
    Value::Object(verdict)
}

/// Takes a check of the window back, unless the person has answered it or holds answers to it (check.drop).
pub fn drop(ticket_id: &str) -> i32 {
    let tool = common::tool();
    let Some(ticket) = common::read_json(&ticket_file(ticket_id)).filter(Value::is_object) else {
        say(&format!("{tool}: в очереди такой проверки нет."));
        return EXIT_NOT_CHECKED;
    };
    let verdict = dropped_verdict(&ticket);
    let _ = std::fs::create_dir_all(window::window_folder());
    // The panel holds this lock for its whole lifetime, including startup and answer restoration. An offline drop
    // holds it through claim and unlink, so a panel cannot start accepting input between the state read and removal.
    let lock = match std::fs::OpenOptions::new().read(true).append(true).create(true).open(window::window_folder().join("panel.lock")) {
        Ok(lock) => lock,
        Err(error) => {
            say(&format!("{tool}: {error}"));
            return 1;
        }
    };
    let held = unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
    if !held {
        let error = std::io::Error::last_os_error();
        if error.kind() != ErrorKind::WouldBlock {
            say(&format!("{tool}: {error}"));
            return 1;
        }
        let outcome = drop_through_panel(ticket_id, &verdict);
        if outcome != "dropped" {
            if outcome == "protected" {
                say(&format!("{tool}: проверка уже содержит ответы или заметки {}; чат не может её снять.", common::whose()));
            } else {
                say(&format!("{tool}: панель не подтвердила снятие; автоматического удаления без её ответа нет. Проверь очередь."));
            }
            return 1;
        }
    } else {
        if has_saved_answers(ticket_id) {
            say(&format!("{tool}: проверка содержит сохранённые ответы или заметки {}; чат не может её снять.", common::whose()));
            return 1;
        }
        if !claim_done(ticket_id, &verdict) {
            say(&format!("{tool}: {} уже {} проверку {ticket_id} в окне: снимать нечего, {} ответ — {}", common::who(),
                         common::did("закончил"), common::his(), common::command(&["wait", ticket_id])));
            return 1;
        }
        let _ = std::fs::remove_file(ticket_file(ticket_id));
        common::record(&verdict);
    }
    std::mem::drop(lock);
    common::emit("hitl.dropped", json!({"request": ticket_id, "source": "hitl", "outcome": "dropped"}));
    say(&format!("==> Проверка {ticket_id} снята."));
    0
}

/// `hitl drop [ID|TAG] [--note TEXT] [--force]` (cli.drop and checks.drop, the window's side).
pub fn drop_command(arguments: &[String]) -> i32 {
    let (mut target, mut force): (Option<String>, bool) = (None, false);
    let mut position = 0;
    while position < arguments.len() {
        let argument = &arguments[position];
        if argument == "--force" {
            force = true;
        } else if argument == "--note" && position + 1 < arguments.len() {
            position += 1; // why it is taken back: the phone's queue keeps it, the window has nowhere to
        } else if argument.starts_with("--note=") {
        } else if !argument.starts_with('-') && target.is_none() {
            target = Some(argument.clone());
        } else {
            say(&format!("usage: {} drop [ID|TAG] [--note TEXT] [--force]", common::tool()));
            return common::EXIT_REFUSED;
        }
        position += 1;
    }
    drop_target(target.as_deref().filter(|target| !target.is_empty()), force)
}

/// A chat takes back only its own check; another chat's is the person's to drop, or the person's word to
/// (checks.drop, the window's side).
fn drop_target(target: Option<&str>, force: bool) -> i32 {
    if let Some(ticket) = in_window(target, true) {
        let asker = common::chat_title(&common::env());
        if !asker.is_empty() && !force && text_or_empty(ticket.get("chat")) != asker {
            let tag = if truthy(ticket.get("tag")) { py_str(&ticket["tag"]) } else { "чат".into() };
            say(&format!("{tag} «{}» — проверка другого чата: убрать её может {}. Если {} прямо {} это сделать тебе, повтори с --force.",
                         text_or(&ticket, "title", ""), common::who(), common::he("он", "она"), common::did("сказал")));
            return 1;
        }
        return drop(common::text_of(&ticket, "id"));
    }
    let place = if common::is_host() { "ни в окне на Маке, ни на телефоне" } else { "в окне на Маке" };
    let said = match target {
        Some(target) => format!("«{target}» не ждёт {place}."),
        None => "у этого чата нет ждущей проверки; назови её номер или метку чата.".to_string(),
    };
    say(&format!("{}: {said}", common::tool()));
    1
}

// --------------------------------------------------------------------------
// The last check of a branch

/// Whether an ended check was asked from a checkout of the repository; any, when none is named (common.of_repository).
fn of_repository(entry: &Value, repository: Option<&str>, known: &mut HashMap<String, String>) -> bool {
    let Some(repository) = repository else { return true };
    common::asked_in(entry, repository, known)
}

/// The check of a branch that the window ended last, as a gate reads it; of that kind only, when one is named, and
/// asked from a checkout of that repository, when one is named (check.last_check).
pub fn last_check(branch: &str, kind: Option<&str>, repository: Option<&str>) -> Option<Value> {
    let mut known = HashMap::new();
    let mut found = None;
    for entry in common::history() {
        if common::screen_of(&entry) == "mac" && entry.get("branch").and_then(Value::as_str) == Some(branch)
            && entry.get("how").and_then(Value::as_str) != Some("dropped")
            && kind.is_none_or(|kind| entry.get("kind").and_then(Value::as_str) == Some(kind))
            && of_repository(&entry, repository, &mut known) {
            found = Some(entry);
        }
    }
    let found = found?;
    let take = |key: &str| found.get(key).cloned().unwrap_or(Value::Null);
    Some(json!({"id": take("id"), "screen": "mac", "tag": take("tag"), "title": take("title"), "kind": take("kind"),
                "branch": take("branch"), "commit": take("commit"), "green": verdict::is_green(&found),
                "choices": verdict::answered_by_choice(&found), "summary": verdict::summary(&found), "when": take("finishedAt")}))
}

/// `hitl last-check --branch BRANCH [--screen mac|iphone]` (cli.last_check, the window's side): the JSON of the
/// check, `null` and 1 when there is none.
pub fn last_check_command(arguments: &[String]) -> i32 {
    let (mut branch, mut screen): (String, Option<String>) = (String::new(), None);
    let mut position = 0;
    while position < arguments.len() {
        let argument = arguments[position].as_str();
        if (argument == "--branch" || argument == "--screen") && position + 1 < arguments.len() {
            if argument == "--branch" {
                branch = arguments[position + 1].clone();
            } else {
                screen = Some(arguments[position + 1].clone());
            }
            position += 2;
        } else {
            branch.clear();
            break;
        }
    }
    if branch.is_empty() || !matches!(screen.as_deref(), None | Some("mac") | Some("iphone")) {
        say(&format!("usage: {} last-check --branch BRANCH [--screen mac|iphone]", common::tool()));
        return common::EXIT_REFUSED;
    }
    let mut found = None;
    if matches!(screen.as_deref(), None | Some("mac")) {
        if let Some(mut mac) = last_check(&branch, None, None) {
            let when = mac.get("when").cloned().unwrap_or(Value::Null);
            if let Some(map) = mac.as_object_mut() {
                map.insert("when".into(), json!(common::clock(when.as_f64())));
                map.insert("finishedAt".into(), when);
            }
            found = Some(mac);
        }
    }
    match found {
        Some(found) => {
            print_out(&common::dumps_line(&found));
            0
        }
        None => {
            print_out("null");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verdict::tests::place;

    fn put(path: &Path, value: &Value) {
        common::write_json(path, value).unwrap();
    }

    fn ticket(id: &str, tag: &str, created: f64) -> Value {
        json!({"id": id, "screen": "mac", "tag": tag, "chat": format!("{tag} · проба"), "agent": "Claude", "projectName": "hitl",
               "title": "Вход", "kind": "result", "kindTitle": "результат", "items": [{"label": "1", "text": "Пункт"},
               {"label": "2", "text": "Шаг", "kind": "action"}], "project": "", "branch": "", "commit": "", "createdAt": created})
    }

    #[test]
    fn saved_answers_keep_a_check_from_a_drop() {
        let place = place(true);
        let path = place.home.join("window/answers.json");
        assert!(!has_saved_answers("t"));
        for mark in [json!({"state": "ok"}), json!({"note": "unfinished"}), json!({"shots": [{"file": "private.png"}]}), json!({"values": []}),
                     json!({"other": true}), json!({"unclear": true}), json!({"choices": ["a"]}), json!("not a mark")] {
            let mut full = json!({"state": null, "note": ""});
            if let (Some(full), Some(more)) = (full.as_object_mut(), mark.as_object()) {
                full.extend(more.clone());
            } else {
                full = mark.clone();
            }
            put(&path, &json!({"checks": {"t": {"marks": [full]}}, "active": "t"}));
            assert!(has_saved_answers("t"), "{mark}");
        }
        put(&path, &json!({"checks": {"t": {"marks": [{"state": null, "note": "", "other": 1, "values": null, "shots": []}]}}}));
        assert!(!has_saved_answers("t"));
        put(&path, &json!({"checks": {"other": {"marks": [{"state": "ok"}]}}}));
        assert!(!has_saved_answers("t"));
        std::fs::write(&path, "broken JSON").unwrap();
        assert!(has_saved_answers("t"));
        put(&path, &json!({"checks": {"t": {"marks": "x"}}}));
        assert!(has_saved_answers("t"));
    }

    #[test]
    fn the_first_verdict_stands() {
        let _place = place(true);
        assert!(claim_done("t", &json!({"how": "done"})));
        assert!(!claim_done("t", &json!({"how": "dropped"})));
        assert_eq!(common::read_json(&verdict_file("t")).unwrap()["how"], "done");
        let hidden: Vec<PathBuf> = files_ending(&window::done_folder(), ".claim");
        assert!(hidden.is_empty(), "{hidden:?}");
    }

    #[test]
    fn a_busy_panel_lock_never_falls_back_to_unlink() {
        let place = place(true);
        put(&ticket_file("t"), &ticket("t", "r/66", 1.0));
        std::fs::create_dir_all(place.home.join("window")).unwrap();
        let held = std::fs::OpenOptions::new().write(true).create(true).truncate(false).open(place.home.join("window/panel.lock")).unwrap();
        assert_eq!(unsafe { libc::flock(held.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) }, 0);
        // flock is per open file: this process's second open is refused as another's would be.
        let started = Instant::now();
        assert_eq!(drop("t"), 1);
        assert!(started.elapsed() >= Duration::from_secs(3));
        assert!(ticket_file("t").exists());
        assert!(!verdict_file("t").exists());
        assert!(files_ending(&place.home.join("window/drops"), ".json").is_empty());
    }

    #[test]
    fn a_drop_ends_a_waiting_check_and_does_not_take_the_place_of_his_verdict() {
        let place = place(true);
        put(&ticket_file("t"), &ticket("t", "r/66", 1.0));
        assert_eq!(drop("t"), 0);
        assert!(!ticket_file("t").exists());
        let kept = common::history().pop().unwrap();
        assert_eq!((kept["how"].as_str(), kept["results"][0]["text"].as_str(), kept.get("agent").is_some()),
                   (Some("dropped"), Some("Пункт"), true));
        assert_eq!(drop("t"), EXIT_NOT_CHECKED);
        // The window linked his verdict in and has not yet taken the ticket away.
        put(&ticket_file("u"), &ticket("u", "r/66", 2.0));
        put(&verdict_file("u"), &json!({"id": "u", "how": "done", "results": [{"label": "1", "state": "ok", "note": ""}]}));
        let before = common::history().len();
        assert_eq!(drop("u"), 1);
        assert_eq!(common::read_json(&verdict_file("u")).unwrap()["how"], "done");
        assert_eq!(common::history().len(), before);
        assert!(files_ending(&place.home.join("done"), ".claim").is_empty());
        // Saved answers keep it, and a broken file of them too.
        put(&ticket_file("v"), &ticket("v", "r/66", 3.0));
        put(&place.home.join("window/answers.json"), &json!({"checks": {"v": {"marks": [{"state": "ok", "note": ""}]}}}));
        assert_eq!(drop("v"), 1);
        assert!(ticket_file("v").exists() && !verdict_file("v").exists());
    }

    #[test]
    fn a_check_is_found_by_its_number_its_tag_or_the_chat_s_own() {
        let _place = place(true);
        put(&verdict_file("old"), &json!({"id": "old", "tag": "r/7"}));
        assert_eq!(find(Some("old"), "r/66").as_deref(), Some("old"));
        assert_eq!(find(Some("r/7"), "r/66").as_deref(), Some("old"));
        assert_eq!(find(None, "r/66"), None);
        assert_eq!(find(Some("nothing"), "r/66"), None);
        put(&verdict_file("no-id"), &json!({"tag": "r/8"}));
        assert_eq!(find(Some("r/8"), "r/66"), None);
    }

    #[test]
    fn the_last_check_of_a_branch_is_what_a_landing_gate_reads() {
        let _place = place(true);
        let arguments = |words: &[&str]| words.iter().map(|word| word.to_string()).collect::<Vec<_>>();
        assert_eq!(last_check_command(&arguments(&["--branch", "feature"])), 1);
        assert_eq!(last_check_command(&arguments(&["--branch"])), common::EXIT_REFUSED);
        assert_eq!(last_check_command(&arguments(&["--branch", "feature", "--screen", "watch"])), common::EXIT_REFUSED);
        common::record(&json!({"id": "a", "screen": "mac", "tag": "r/66", "title": "Результат", "kind": "result", "branch": "feature",
                               "commit": "abc123", "how": "done", "finishedAt": 2.5, "results": [{"label": "1", "state": "ok"}]}));
        common::record(&json!({"id": "b", "screen": "mac", "kind": "result", "branch": "feature", "how": "dropped", "results": []}));
        let found = last_check("feature", None, None).unwrap();
        assert_eq!(common::dumps_line(&found),
                   "{\"id\": \"a\", \"screen\": \"mac\", \"tag\": \"r/66\", \"title\": \"Результат\", \"kind\": \"result\", \"branch\": \"feature\", \
                    \"commit\": \"abc123\", \"green\": true, \"choices\": [], \"summary\": \"ок 1\", \"when\": 2.5}");
        assert!(last_check("feature", Some("plan"), None).is_none());
        assert!(last_check("feature", Some("result"), Some("/no/such/repository")).is_none());
        assert_eq!(last_check_command(&arguments(&["--branch", "feature"])), 0);
        assert_eq!(last_check_command(&arguments(&["--branch", "feature", "--screen", "iphone"])), 1);
    }

    #[test]
    fn a_check_asked_from_a_worktree_removed_since_is_still_the_repository_s() {
        // On 2026-11-04 a restart wiped the temporary folder of a worktree, and the merge no longer found his «ок».
        let place = place(true);
        let app = place.home.join("app");
        let tree = place.home.join("tree");
        std::fs::create_dir_all(&app).unwrap();
        let run = |arguments: &[&str]| {
            let done = std::process::Command::new("git").args(["-c", "user.name=Fixture", "-c", "user.email=fixture@invalid"])
                .args(arguments).output().unwrap();
            assert!(done.status.success(), "{}", String::from_utf8_lossy(&done.stderr));
            String::from_utf8_lossy(&done.stdout).trim().to_string()
        };
        let (app_path, tree_path) = (app.to_str().unwrap(), tree.to_str().unwrap());
        run(&["init", "-q", app_path]);
        run(&["-C", app_path, "commit", "-q", "--allow-empty", "-m", "start"]);
        run(&["-C", app_path, "worktree", "add", "-q", "-b", "feature", tree_path]);
        run(&["-C", tree_path, "commit", "-q", "--allow-empty", "-m", "the change"]);
        let commit = run(&["-C", tree_path, "rev-parse", "HEAD"]);
        let repository = common::repository_of(app_path);
        let entry = |id: &str, name: &str, commit: &str| json!({"id": id, "screen": "mac", "kind": "result", "branch": "feature",
            "project": tree_path, "projectName": name, "commit": commit, "how": "done", "finishedAt": 2.5,
            "results": [{"label": "1", "state": "ok"}]});
        common::record(&entry("seen", "app", &commit));
        assert_eq!(last_check("feature", Some("result"), Some(&repository)).unwrap()["id"], "seen");
        run(&["-C", app_path, "worktree", "remove", tree_path]);
        assert!(!tree.exists());
        assert_eq!(last_check("feature", Some("result"), Some(&repository)).unwrap()["id"], "seen");
        // Not when the project is another one, or the commit is not the repository's.
        common::record(&entry("other", "other", &commit));
        assert!(last_check("feature", Some("result"), Some(&repository)).is_some_and(|found| found["id"] == "seen"));
        common::record(&entry("unknown", "app", "0123456789abcdef0123456789abcdef01234567"));
        assert!(last_check("feature", Some("result"), Some(&repository)).is_some_and(|found| found["id"] == "seen"));
    }

    #[test]
    fn old_verdicts_go_with_the_screenshots_of_their_comments() {
        let place = place(true);
        let old = SystemTime::now() - Duration::from_secs_f64(KEEP_DONE_SECONDS + 60.0);
        for name in ["done/old.json", "done/new.json", "shots/old.png", "shots/old.txt"] {
            let path = place.home.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "{}").unwrap();
            if name.contains("old") {
                std::fs::File::options().write(true).open(&path).unwrap().set_modified(old).unwrap();
            }
        }
        clean_done();
        let left = |folder: &str| common::sorted_dir(&place.home.join(folder)).iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().to_string()).collect::<Vec<_>>();
        assert_eq!((left("done"), left("shots")), (vec!["new.json".to_string()], vec!["old.txt".to_string()]));
    }

    #[test]
    fn a_verdict_that_is_there_is_printed_at_once() {
        let _place = place(true);
        put(&verdict_file("t"), &json!({"id": "t", "title": "Т", "how": "done", "results": [{"label": "1", "state": "ok"}]}));
        assert_eq!(wait_for("t", true), 0);
        put(&verdict_file("d"), &json!({"id": "d", "title": "Т", "how": "dropped", "results": []}));
        assert_eq!(wait_for("d", true), EXIT_NOT_CHECKED);
    }

    #[test]
    fn the_queue_says_the_project_first_and_the_recent_answers() {
        let _place = place(true);
        let mut first = ticket("t1", "r/66", 1.0);
        first["title"] = json!("Вход");
        put(&ticket_file("t1"), &first);
        let mut second = ticket("t2", "r/7", 2.0);
        second["projectName"] = json!("");
        second["kind"] = json!("action");
        put(&ticket_file("t2"), &second);
        put(&window::progress_file(), &json!({"id": "t2", "marked": 1, "total": 2}));
        common::record(&json!({"id": "v", "screen": "mac", "tag": "r/5", "title": "Плагин", "kind": "checklist", "how": "done", "finishedAt": 0,
                               "results": [{"label": "1", "state": "problem", "note": ""}, {"label": "2", "question": true, "state": "problem"}]}));
        common::record(&json!({"id": "p", "title": "Телефон", "how": "done", "finishedAt": 0}));
        let (one, two) = (common::clock(Some(1.0)), common::clock(Some(2.0)));
        assert_eq!(queue_lines(), [
            "Окно проверок на Маке".to_string(),
            format!("  Сейчас: hitl · r/66 «Вход» · результат · с {one}"),
            format!("  Сейчас: r/7 «Вход» · действие · с {two} · отмечено 1 из 2"),
            "  Недавние:".to_string(),
            "    r/5 «Плагин» · — · не работает 1, свой ответ 1".to_string(),
        ]);
    }

    #[test]
    fn the_drop_command_reads_its_words_as_python_did() {
        let _place = place(false);
        let arguments = |words: &[&str]| words.iter().map(|word| word.to_string()).collect::<Vec<_>>();
        for wrong in [&["--bogus"][..], &["a", "b"], &["--note"]] {
            assert_eq!(drop_command(&arguments(wrong)), common::EXIT_REFUSED, "{wrong:?}");
        }
        // Nothing waits: «не ждёт в окне на Маке», in the words of hitl.
        assert_eq!(drop_command(&arguments(&["nothing", "--note", "why", "--force"])), 1);
        assert_eq!(drop_command(&arguments(&["--note=why"])), 1);
    }
}
