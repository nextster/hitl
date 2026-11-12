//! What the phone is shown: the window's queue, each check as the app shows it, named as in the window; the files the
//! queue names, which are all the phone may ask for; and a press of the phone as the window takes it.

use crate::{codec, py, word};
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub const MAX_IMAGES: usize = 4;
/// What the window shows as a picture (check.PICTURES).
pub const PICTURES: [&str; 9] = [".bmp", ".gif", ".heic", ".jpeg", ".jpg", ".png", ".tif", ".tiff", ".webp"];

pub fn queue_folder() -> PathBuf {
    hitl_host_core::home().join("queue")
}

pub fn shots_folder() -> PathBuf {
    hitl_host_core::home().join("shots")
}

/// Whether a file is there. What is given as a link may be an address too long to be a file's name.
pub fn is_file(path: &Path) -> bool {
    std::fs::metadata(path).map(|meta| meta.is_file()).unwrap_or(false)
}

/// "web" for a page, "app" for another app's own address, "file" for a file on this Mac.
pub fn link_kind(link: &str) -> &'static str {
    if link.starts_with('/') || link.starts_with('~') {
        return "file";
    }
    match scheme(link) {
        Some(scheme) if scheme == "http" || scheme == "https" => "web",
        Some(_) => "app",
        None => "file",
    }
}

/// The scheme of an address, lower case: `[A-Za-z][A-Za-z0-9+.-]*:` at its start.
fn scheme(link: &str) -> Option<String> {
    let colon = link.find(':')?;
    let head = &link[..colon];
    let mut characters = head.chars();
    let first = characters.next()?;
    (first.is_ascii_alphabetic() && characters.all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))).then(|| head.to_ascii_lowercase())
}

/// What a link is called on its button, as the window calls it: a pull request by its number, a site by its host, a
/// file by its name.
pub fn link_name(link: &str) -> String {
    match link_kind(link) {
        "file" => py::name(link),
        "app" => link.split(':').next().unwrap_or("").to_string(),
        _ => {
            let rest = link.strip_prefix("https://").or_else(|| link.strip_prefix("http://"));
            let Some(rest) = rest else { return link.to_string() };
            let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
            let host = &rest[..host_end];
            if host.is_empty() {
                return link.to_string();
            }
            let after = &rest[host_end..];
            let path = &after[..after.find(['?', '#']).unwrap_or(after.len())];
            let parts: Vec<&str> = path.split('/').collect();
            if host == "github.com" {
                if let Some(at) = parts.iter().position(|part| *part == "pull").filter(|at| at + 1 < parts.len()) {
                    return format!("PR #{}", parts[at + 1]);
                }
            }
            host.to_string()
        }
    }
}

pub fn is_picture(link: &str) -> bool {
    if link_kind(link) != "file" {
        return false;
    }
    let file = py::expanded(link);
    PICTURES.contains(&py::suffix(&file).to_lowercase().as_str()) && is_file(Path::new(&file))
}

/// A text of a check as the app gets it: a link in it to a file of this Mac by the file's full path, which is what the
/// app asks this Mac for. The files are put into `files`.
pub fn text_for_phone(text: &Value, files: &mut Vec<String>) -> Value {
    let Value::String(text) = text else { return text.clone() };
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for link in hitl::common::find_links(text) {
        out.push_str(&text[last..link.start]);
        let (words, target) = (link.words.trim(), link.target.trim());
        if words.is_empty() || !(target.starts_with('/') || target.starts_with("~/")) {
            out.push_str(&text[link.start..link.end]);
        } else {
            files.push(py::expanded(target));
            out.push_str(&format!("[{words}]({})", files[files.len() - 1]));
        }
        last = link.end;
    }
    out.push_str(&text[last..]);
    Value::String(out)
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value.and_then(Value::as_array).map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

static LOOKUPS: AtomicU64 = AtomicU64::new(0);

/// How many times the labels of checks were asked of hitl: for this crate's own tests.
#[doc(hidden)]
pub fn lookups() -> u64 {
    LOOKUPS.load(Ordering::SeqCst)
}

/// The three seconds this moment is in: a label is reused within them.
fn interval() -> u64 {
    static BEGAN: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    BEGAN.get_or_init(Instant::now).elapsed().as_secs() / 3
}

/// The chat's name and its link as the window shows them. The fast loop that carries the queue to the phone reuses a
/// label for at most three seconds; a changed binding of the check, or another place its metadata is read from, is
/// looked up at once.
fn presentation(ticket: &Value, interval: u64) -> Value {
    static CACHE: Mutex<Option<(u64, HashMap<String, Value>)>> = Mutex::new(None);
    let identity: Map<String, Value> = ["agent", "chatIds", "chatRef", "t3Base", "cwd", "project", "chat"].iter()
        .map(|key| (key.to_string(), ticket.get(*key).cloned().unwrap_or(Value::Null))).collect();
    let environment: Map<String, Value> = ["CONVEYOR_HOME", "CONVEYOR_CLAUDE_SESSIONS_DIR", "CONVEYOR_CLAUDE_PROJECTS_DIR",
                                           "CONVEYOR_CODEX_STATE", "CONVEYOR_OPENCODE_STATE", "XDG_DATA_HOME", "T3CODE_HOME"].iter()
        .filter_map(|key| Some((key.to_string(), json!(std::env::var(key).ok()?)))).collect();
    let identity = word::sorted_dump(&json!([identity, environment]));
    let mut cache = CACHE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if cache.as_ref().is_none_or(|(at, _)| *at != interval) {
        *cache = Some((interval, HashMap::new()));
    }
    let known = &mut cache.as_mut().expect("a cache").1;
    if let Some(label) = known.get(&identity) {
        return label.clone();
    }
    LOOKUPS.fetch_add(1, Ordering::SeqCst);
    let label = hitl::names::presentation(ticket, &hitl::common::env()).unwrap_or_else(|_| {
        json!({"chat": ticket.get("chat").cloned().unwrap_or(Value::Null), "reference": "", "status": "unavailable"})
    });
    if known.len() < 256 {
        known.insert(identity, label.clone());
    }
    label
}

/// A check as the app shows it: its items with their pictures and what they offer to open, named as in the window.
pub fn shown(raw: &Value) -> Value {
    shown_at(raw, interval())
}

/// A check as the app shows it, its label reused within the three seconds `interval` numbers.
#[doc(hidden)]
pub fn shown_at(raw: &Value, interval: u64) -> Value {
    let label = presentation(raw, interval);
    let mut ticket = raw.as_object().cloned().unwrap_or_default();
    ticket.insert("chat".into(), label.get("chat").cloned().unwrap_or(Value::Null));
    // What opens the chat in its own app: the reference names a chat to hitl's own app, which is gone.
    let text_of = |value: Option<&Value>| value.and_then(Value::as_str).unwrap_or("").to_string();
    let mut opened = hitl::names::open_link(&text_of(label.get("reference")), &text_of(ticket.get("chatLink")));
    // An address that needs a command of the Mac (a chat in a terminal, say) has nothing to open it on the phone: its button would be dead.
    if hitl::chatbutton::needs_command(&hitl_host_core::env(), &opened) {
        opened.clear();
    }
    ticket.insert("chatLink".into(), if opened.is_empty() { Value::Null } else { json!(opened) });
    let ticket = Value::Object(ticket);
    let mut about_files = Vec::new();
    let about = text_for_phone(&py::or(ticket.get("about"), json!("")), &mut about_files);
    let mut items = Vec::new();
    for item in ticket.get("items").and_then(Value::as_array).cloned().unwrap_or_default() {
        let links = strings(item.get("links"));
        let mut pictures = strings(item.get("images"));
        pictures.extend(links.iter().filter(|link| is_picture(link)).map(|link| py::expanded(link)));
        pictures.truncate(MAX_IMAGES);
        let mut opens: Vec<Value> = links.iter().filter(|link| !is_picture(link)).map(|link| {
            let kind = link_kind(link);
            json!({"title": link_name(link), "link": if kind == "file" { py::expanded(link) } else { link.clone() }, "kind": kind})
        }).collect();
        if let Some(card) = ticket.get("card").filter(|card| card.is_object() && hitl_host_core::truthy(card.get("path"))) {
            let title = py::str_of(&py::or(card.get("title"), json!("карточка")));
            let mut characters = title.chars();
            let title: String = characters.next().map(|first| first.to_uppercase().chain(characters).collect()).unwrap_or_default();
            opens.push(json!({"title": title, "link": card["path"], "kind": "file"}));
        }
        if hitl_host_core::truthy(ticket.get("chatLink")) {
            opens.push(json!({"title": "Чат", "link": ticket["chatLink"], "kind": "app"}));
        }
        // The lines he copies with a click in the window, and with a tap in the app. A build of the app that knows no
        // such lines, one of a branch behind main, has each among the item's facts, so that an item that says
        // «запусти команду» has its command there too; the app that knows them leaves those out of the facts.
        let copies = strings(item.get("copies"));
        // The files its texts link to, with the ones the check's own line links to: he reads that line over every item.
        let mut files = about_files.clone();
        let text = text_for_phone(&py::get(&item, "text", json!("")), &mut files);
        let details = text_for_phone(item.get("details").unwrap_or(&Value::Null), &mut files);
        let mut facts: Vec<Value> = item.get("facts").and_then(Value::as_array).filter(|facts| !facts.is_empty()).cloned()
            .unwrap_or_default().iter().map(|fact| text_for_phone(fact, &mut files)).collect();
        facts.extend(copies.iter().map(|line| json!(line)));
        let mut one = Map::new();
        one.insert("label".into(), py::get(&item, "label", json!("")));
        one.insert("text".into(), text);
        one.insert("details".into(), details);
        one.insert("linked".into(), json!(files));
        one.insert("facts".into(), Value::Array(facts));
        one.insert("copies".into(), json!(copies));
        one.insert("pictures".into(), json!(pictures));
        one.insert("page".into(), item.get("page").cloned().unwrap_or(Value::Null));
        one.insert("options".into(), py::or(item.get("options"), json!([])));
        one.insert("many".into(), json!(hitl_host_core::truthy(item.get("many"))));
        one.insert("opens".into(), Value::Array(opens));
        // An item may be of a kind of its own, with the answers of that kind.
        for key in ["kind", "kindTitle", "answers"] {
            one.insert(key.into(), item.get(key).cloned().unwrap_or(Value::Null));
        }
        items.push(Value::Object(one));
    }
    let folder_name = py::or(ticket.get("cwd"), py::or(ticket.get("project"), json!("")));
    // The app the check came from stands before the project as its icon, on the phone as in the window («слева от
    // имени проекта только символом»). The phone has no way to this Mac's apps: the window keeps the icon of each as
    // a file, and the phone asks for it as for an item's picture. No file, and the app is said by its name.
    let agent = py::or(ticket.get("agent"), json!(""));
    let icon = agent.as_str().filter(|name| *name == "T3 Code" || (!name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric())))
        .map(|name| word::remote_folder().join("icons").join(format!("{name}.png")));
    let app_icon = icon.filter(|icon| is_file(icon)).map(|icon| icon.display().to_string()).unwrap_or_default();
    let mut one = Map::new();
    one.insert("id".into(), ticket.get("id").cloned().unwrap_or(Value::Null));
    one.insert("onPhone".into(), json!(ticket.get("screen").and_then(Value::as_str) == Some("iphone")));
    one.insert("device".into(), ticket.get("device").cloned().unwrap_or(Value::Null));
    one.insert("tag".into(), py::or(ticket.get("tag"), json!("")));
    one.insert("chat".into(), py::or(ticket.get("chat"), json!("")));
    one.insert("agent".into(), agent);
    one.insert("appIcon".into(), json!(app_icon));
    // Which of his projects the check is about: first on the phone too («на телефоне хотелось бы тот же функционал»).
    one.insert("projectName".into(), py::or(ticket.get("projectName"), json!("")));
    one.insert("folder".into(), json!(folder_name.as_str().filter(|name| !name.is_empty()).map(py::name).unwrap_or_default()));
    one.insert("branch".into(), py::or(ticket.get("branch"), json!("")));
    one.insert("title".into(), py::or(ticket.get("title"), json!("")));
    one.insert("about".into(), about);
    one.insert("kind".into(), py::or(ticket.get("kind"), json!("checklist")));
    one.insert("kindTitle".into(), py::or(ticket.get("kindTitle"), json!("")));
    one.insert("answers".into(), py::or(ticket.get("answers"), json!(hitl::submit::answers("checklist"))));
    one.insert("createdAt".into(), py::or(ticket.get("createdAt"), json!(0)));
    one.insert("items".into(), Value::Array(items));
    Value::Object(one)
}

/// Every file the queue names: what the phone may ask for, and nothing else on this Mac.
pub fn files_of(checks: &[Value], marks: &[Value]) -> HashSet<String> {
    let mut allowed = HashSet::new();
    let mut add = |value: Option<&Value>| {
        if let Some(path) = value.and_then(Value::as_str).filter(|path| !path.is_empty()) {
            allowed.insert(path.to_string());
        }
    };
    for check in checks {
        add(check.get("appIcon"));
        for item in check.get("items").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]) {
            item.get("pictures").and_then(Value::as_array).into_iter().flatten().for_each(|path| add(Some(path)));
            add(item.get("page"));
            for entry in item.get("opens").and_then(Value::as_array).into_iter().flatten() {
                if entry.get("kind").and_then(Value::as_str) == Some("file") {
                    add(entry.get("link"));
                }
            }
            item.get("linked").and_then(Value::as_array).into_iter().flatten().for_each(|path| add(Some(path)));
        }
    }
    for mark in marks {
        mark.get("shots").and_then(Value::as_array).into_iter().flatten().for_each(|shot| add(Some(shot)));
    }
    allowed
}

/// Whether a process is there: a signal 0 is refused for one that is gone.
pub fn alive(pid: Option<&Value>) -> bool {
    let Some(pid) = pid.and_then(Value::as_i64).filter(|pid| *pid > 0 && *pid <= i64::from(i32::MAX)) else { return false };
    if unsafe { libc::kill(pid as libc::pid_t, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

/// Every check in the window's queue, the phone's own among them, by its number.
pub fn read_tickets() -> HashMap<String, Value> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(queue_folder()).map(|entries| {
        entries.flatten().map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|extension| extension == "json")).collect()
    }).unwrap_or_default();
    paths.sort();
    let mut found = HashMap::new();
    for path in paths {
        let Some(ticket) = hitl_host_core::read_json(&path) else { continue };
        let Some(id) = ticket.get("id").and_then(Value::as_str).filter(|id| !id.is_empty()).map(str::to_string) else { continue };
        if ticket.get("items").is_some_and(Value::is_array) {
            found.insert(id, ticket);
        }
    }
    found
}

static RECENT: Mutex<Option<(Option<SystemTime>, Vec<Value>)>> = Mutex::new(None);

/// The last checks ended in the window; read again only when the history has grown.
fn recent_checks() -> Vec<Value> {
    let stamp = std::fs::metadata(hitl_host_core::history_file()).and_then(|meta| meta.modified()).ok();
    let mut recent = RECENT.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((known, found)) = recent.as_ref() {
        if *known == stamp {
            return found.clone();
        }
    }
    let history = hitl_host_core::history();
    let last = &history[history.len().saturating_sub(40)..];
    let ended: Vec<Value> = last.iter().filter(|entry| {
        hitl::common::screen_of(entry) == "mac" && entry.get("how").and_then(Value::as_str) != Some("dropped")
    }).map(|entry| json!({"tag": py::or(entry.get("tag"), json!("")), "title": py::or(entry.get("title"), json!("")),
                          "at": py::or(entry.get("finishedAt"), json!(0)), "summary": hitl::verdict::summary(entry)})).collect();
    let found = ended[ended.len().saturating_sub(5)..].to_vec();
    *recent = Some((stamp, found.clone()));
    found
}

fn as_int(value: Option<&Value>) -> i64 {
    match value {
        Some(Value::Number(number)) => number.as_i64().unwrap_or_else(|| number.as_f64().unwrap_or(0.0) as i64),
        Some(Value::Bool(flag)) => i64::from(*flag),
        Some(Value::String(text)) => text.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

fn created(ticket: &Value) -> f64 {
    ticket.get("createdAt").and_then(Value::as_f64).unwrap_or(0.0)
}

/// What the app shows now, the files it may ask for, and the checks themselves as they lie in the queue.
pub fn snapshot() -> (Value, HashSet<String>, Vec<Value>) {
    let raw = read_tickets();
    let state = hitl_host_core::read_json(&word::remote_folder().join("state.json"));
    let up = state.as_ref().is_some_and(|state| state.is_object() && alive(state.get("pid")));
    let state = if up { state.unwrap_or_else(|| json!({})) } else { json!({}) };
    // The window's own order; a check it has not seen yet stands after, oldest first.
    let mut order: Vec<String> = strings(state.get("order")).into_iter().filter(|id| raw.contains_key(id)).collect();
    let mut later: Vec<&String> = raw.keys().filter(|id| !order.contains(id)).collect();
    later.sort_by(|one, other| created(&raw[*one]).total_cmp(&created(&raw[*other])).then_with(|| one.cmp(other)));
    order.extend(later.into_iter().cloned());
    let kept: HashMap<String, i64> = state.get("kept").and_then(Value::as_array).into_iter().flatten().filter(|entry| entry.is_object())
        .filter_map(|entry| Some((entry.get("id")?.as_str()?.to_string(), as_int(entry.get("marked")))))
        .collect();
    let active = state.get("active").and_then(Value::as_str).filter(|id| raw.contains_key(*id)).map(str::to_string);
    let marks: Vec<Value> = if active.is_some() {
        state.get("marks").and_then(Value::as_array).into_iter().flatten().filter(|mark| mark.is_object()).cloned().collect()
    } else {
        Vec::new()
    };
    let mut checks = Vec::new();
    for id in &order {
        let mut one = shown(&raw[id]);
        let marked = if active.as_deref() == Some(id) {
            marks.iter().filter(|mark| hitl_host_core::truthy(mark.get("state"))).count() as i64
        } else {
            kept.get(id).copied().unwrap_or(0)
        };
        one["marked"] = json!(marked);
        checks.push(one);
    }
    let mut recent = recent_checks();
    recent.reverse();
    let flag = |key: &str| json!(hitl_host_core::truthy(state.get(key)));
    let mut said = Map::new();
    said.insert("mac".into(), json!(word::mac_name(false)));
    said.insert("window".into(), json!(up));
    said.insert("active".into(), json!(active));
    said.insert("index".into(), json!(if active.is_some() { as_int(state.get("index")) } else { 0 }));
    said.insert("marks".into(), Value::Array(marks.clone()));
    said.insert("canPostpone".into(), flag("canPostpone"));
    said.insert("canFinish".into(), flag("canFinish"));
    said.insert("editing".into(), flag("editing"));
    said.insert("checks".into(), Value::Array(checks.clone()));
    said.insert("recent".into(), Value::Array(recent));
    let allowed = files_of(&checks, &marks);
    (Value::Object(said), allowed, order.iter().map(|id| raw[id].clone()).collect())
}

// --------------------------------------------------------------------------
// A press on the phone, handed to the window

const WORDS: [&str; 8] = ["take", "go", "answer", "note", "values", "unshot", "postpone", "finish"];
static HANDED: AtomicU64 = AtomicU64::new(0);

fn nanoseconds() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_nanos()).unwrap_or(0)
}

/// Leaves one press where the window takes them from, in the order they came.
pub fn hand_to_window(press: &Value) -> std::io::Result<()> {
    let handed = HANDED.fetch_add(1, Ordering::SeqCst) + 1;
    let inbox = word::remote_folder().join("in");
    std::fs::create_dir_all(&inbox)?;
    let name = format!("{:020}-{handed:06}", nanoseconds());
    let temporary = inbox.join(format!(".{name}.tmp"));
    std::fs::write(&temporary, hitl_host_core::dumps_line(press))?;
    std::fs::rename(&temporary, inbox.join(format!("{name}.json")))
}

/// A press as the window takes it, with nothing in it but what a press may hold; None for anything else.
pub fn pressed(said: &Value) -> Option<Value> {
    let what = said.get("do").and_then(Value::as_str).filter(|what| WORDS.contains(what))?;
    let ticket = said.get("ticket").and_then(Value::as_str)?;
    let mut press = Map::new();
    press.insert("do".into(), json!(what));
    press.insert("ticket".into(), json!(ticket));
    if let Some(label) = said.get("label").and_then(Value::as_str) {
        press.insert("label".into(), json!(label));
    }
    // An answer by its row: the item's answers and options from 0, -1 his own answer, -2 «не понял».
    if let Some(number) = said.get("number").and_then(Value::as_i64).filter(|number| (-2..20).contains(number)) {
        press.insert("number".into(), json!(number));
    }
    if let Some(text) = said.get("text").and_then(Value::as_str) {
        press.insert("text".into(), json!(text.chars().take(4000).collect::<String>()));
    }
    if let Some(how) = said.get("how").and_then(Value::as_str).filter(|how| *how == "done" || *how == "rework") {
        press.insert("how".into(), json!(how));
    }
    if let Some(path) = said.get("path").and_then(Value::as_str) {
        press.insert("path".into(), json!(path));
    }
    if let Some(values) = said.get("values").and_then(Value::as_array) {
        let pairs: Vec<Value> = values.iter().take(12).filter_map(|pair| {
            let pair = pair.as_array().filter(|pair| pair.len() == 2)?;
            Some(json!([py::str_of(&pair[0]), py::str_of(&pair[1])]))
        }).collect();
        press.insert("values".into(), Value::Array(pairs));
    }
    // Pressed while the phone had no way to the Mac, and kept until it had: what he did on the Mac meanwhile stands.
    if said.get("late") == Some(&Value::Bool(true)) {
        press.insert("late".into(), json!(true));
    }
    Some(Value::Object(press))
}

/// A picture he added to a comment on the phone: kept with the window's own screenshots, and handed to it.
pub fn keep_shot(said: &Value) -> Option<Value> {
    let data = said.get("data").and_then(Value::as_str).unwrap_or("");
    let picture = codec::unbase64(data)?;
    let ticket = said.get("ticket").and_then(Value::as_str)?;
    let label = said.get("label").and_then(Value::as_str)?;
    if !picture.starts_with(b"\x89PNG") || ticket.is_empty() || ticket.contains(['/', '\\', '\0']) {
        return None;
    }
    if !queue_folder().join(format!("{ticket}.json")).exists() {
        return None;
    }
    std::fs::create_dir_all(shots_folder()).ok()?;
    let file = shots_folder().join(format!("{ticket}-phone-{}.png", nanoseconds()));
    std::fs::write(&file, &picture).ok()?;
    Some(json!({"do": "shot", "ticket": ticket, "label": label, "path": file.display().to_string()}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_is_named_as_the_window_names_it() {
        assert_eq!(link_name("https://github.com/o/r/pull/12"), "PR #12");
        assert_eq!(link_name("https://github.com/o/r/pull"), "github.com");
        assert_eq!(link_name("https://realize.day/t/team/320?x=1"), "realize.day");
        assert_eq!(link_name("obsidian://open?path=a"), "obsidian");
        assert_eq!(link_name("/Users/a/plan.md"), "plan.md");
        assert_eq!((link_kind("HTTPS://x"), link_name("HTTPS://x.test/a")), ("web", "HTTPS://x.test/a".to_string()));
        assert_eq!((link_kind("~/a"), link_kind("relative/file"), link_kind("1http://x")), ("file", "file", "file"));
    }

    #[test]
    fn a_press_keeps_only_what_a_press_may_hold() {
        let said = json!({"do": "answer", "ticket": "t", "label": "1", "number": 1, "anything": "else", "late": "yes"});
        assert_eq!(pressed(&said), Some(json!({"do": "answer", "ticket": "t", "label": "1", "number": 1})));
        assert_eq!(pressed(&json!({"do": "rm -rf", "ticket": "t"})), None);
        assert_eq!(pressed(&json!({"do": "go", "ticket": 5})), None);
        let values = json!({"do": "values", "ticket": "t", "values": [["ширина", 480], ["a"], ["поля", "10"]], "number": 20, "late": true});
        assert_eq!(pressed(&values), Some(json!({"do": "values", "ticket": "t", "values": [["ширина", "480"], ["поля", "10"]], "late": true})));
        let long = json!({"do": "note", "ticket": "t", "text": "ж".repeat(5000), "number": true, "how": "later"});
        assert_eq!(pressed(&long).unwrap()["text"].as_str().unwrap().chars().count(), 4000);
        assert!(pressed(&long).unwrap().get("number").is_none());
    }

    #[test]
    fn a_link_to_a_file_goes_by_its_full_path() {
        let mut files = Vec::new();
        let text = json!("См. [план](~/a/план.md), [ PR ]( https://x.test/1 ), [](~/b) и массив[0]");
        let home = std::env::var("HOME").unwrap();
        assert_eq!(text_for_phone(&text, &mut files),
                   json!(format!("См. [план]({home}/a/план.md), [ PR ]( https://x.test/1 ), [](~/b) и массив[0]")));
        assert_eq!(files, vec![format!("{home}/a/план.md")]);
        assert_eq!(text_for_phone(&json!(null), &mut files), json!(null));
    }
}
