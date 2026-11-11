//! What every part of hitl shares: where its state lives, who answers, who asks, how it speaks.
//!
//! The port of what `src/hitl/common.py` gives a check. hitl runs in two ways: as `hitl`, installed by his
//! team (its home `~/.hitl`, the name `hitl setup` kept), and as the core of hitl's own `hitl check`,
//! when hitl runs it with `HITL_TOOL=hitl-host` (its home `~/.conveyor`, the reviewer, and what only hitl has
//! asked of hitl through `HITL_HOOK`).

use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::ffi::CString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const EXIT_REFUSED: i32 = 64;
pub const EXIT_BUSY: i32 = 75;

pub type Env = BTreeMap<String, String>;

/// The process's environment, as a map the functions that take one are given.
pub fn env() -> Env {
    std::env::vars().collect()
}

pub fn var(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

// --------------------------------------------------------------------------
// Which command this is

/// Whether this runs as the core of hitl's own command on the reviewer's Mac.
pub fn is_host() -> bool {
    var("HITL_TOOL").as_deref() == Some("hitl-host")
}

/// The command's name, as a chat and the person who answers read it.
pub fn tool() -> &'static str {
    "hitl"
}

fn user_home() -> PathBuf {
    PathBuf::from(var("HOME").unwrap_or_else(|| "/".into()))
}

/// Where everything is kept: the checks that wait, their verdicts, the history.
pub fn home() -> PathBuf {
    let names = if is_host() { ["CONVEYOR_HOME", "HITL_HOME"] } else { ["HITL_HOME", "CONVEYOR_HOME"] };
    if let Some(path) = names.iter().find_map(|key| var(key).filter(|path| !path.is_empty())) {
        return PathBuf::from(path);
    }
    let home = user_home();
    let current = home.join(".hitl");
    let legacy = home.join(".conveyor");
    if is_host() && !current.exists() && legacy.is_dir() { legacy } else { current }
}

/// The program the window calls back (`check --presentation`, `journal _stream`): hitl's own entry when
/// hitl runs this, else this program by the path that outlives an upgrade (the formula's `HITL_ROOT`).
pub fn entry() -> PathBuf {
    if let Some(named) = var("HITL_ENTRY").filter(|value| !value.is_empty()) {
        return PathBuf::from(named);
    }
    if let Some(root) = var("HITL_ROOT").filter(|value| !value.is_empty()) {
        let found = Path::new(&root).join("hitl");
        if found.is_file() {
            return found;
        }
    }
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("hitl"))
}

/// Where the program's own files stand: the window's source or program and the skill. `HITL_ROOT` from the
/// formula, else beside the program.
pub fn root() -> PathBuf {
    if let Some(root) = var("HITL_ROOT").filter(|value| !value.is_empty()) {
        return PathBuf::from(root);
    }
    let program = std::env::current_exe().ok().and_then(|path| path.canonicalize().ok());
    program.and_then(|path| path.parent().map(Path::to_path_buf)).unwrap_or_else(|| PathBuf::from("."))
}

/// How a chat calls the command, as hints and messages say it: hitl hands its own (`HITL_COMMAND`), else `hitl`
/// when that is this program on the PATH.
pub fn command(words: &[&str]) -> String {
    let base = match var("HITL_COMMAND").filter(|value| !value.is_empty()) {
        Some(named) => named,
        None => {
            let ours = entry().canonicalize().ok();
            let found = which("hitl").and_then(|path| path.canonicalize().ok());
            if found.is_some() && found == ours { "hitl".to_string() } else { entry().display().to_string() }
        }
    };
    std::iter::once(base).chain(words.iter().map(|word| word.to_string())).collect::<Vec<_>>().join(" ")
}

pub fn which(name: &str) -> Option<PathBuf> {
    let path = var("PATH")?;
    path.split(':').filter(|part| !part.is_empty()).map(|part| Path::new(part).join(name)).find(|candidate| {
        std::fs::metadata(candidate).map(|meta| meta.is_file()).unwrap_or(false)
    })
}

// --------------------------------------------------------------------------
// Who answers

#[derive(Clone, Debug, PartialEq)]
pub struct Person {
    pub name: String,
    pub genitive: String,
    pub instrumental: String,
    pub she: bool,
}

/// Who answers the checks, as the texts name him or her: the reviewer on his Mac, else whom `hitl setup` kept.
pub fn person() -> Person {
    let kept = read_json(&home().join("person.json")).unwrap_or(Value::Null);
    let text = |key: &str| kept.get(key).and_then(Value::as_str).map(str::trim).unwrap_or("").to_string();
    let default = if is_host() { "Проверяющий" } else { "Человек" };
    let name = Some(text("name")).filter(|name| !name.is_empty()).unwrap_or_else(|| default.into());
    let or_name = |value: String, fallback: &str| {
        if !value.is_empty() { value } else if name == "Проверяющий" { fallback.into() } else { name.clone() }
    };
    Person {
        genitive: or_name(text("genitive"), "Проверяющего"),
        instrumental: or_name(text("instrumental"), "Проверяющим"),
        she: kept.get("she").and_then(Value::as_bool).unwrap_or(false),
        name,
    }
}

/// «Проверяющий ответил».
pub fn who() -> String {
    person().name
}

/// «ответ Проверяющего».
pub fn whose() -> String {
    person().genitive
}

/// «Завершено Проверяющим».
pub fn by_whom() -> String {
    person().instrumental
}

/// The word that agrees with who answers: «ему» or «ей».
pub fn he(masculine: &str, feminine: &str) -> String {
    if person().she { feminine.to_string() } else { masculine.to_string() }
}

/// A verb in the past, or «сам», as it agrees with who answers: «закончил» or «закончила».
pub fn did(masculine: &str) -> String {
    he(masculine, &format!("{masculine}а"))
}

/// «его ответ» or «её ответ».
pub fn his() -> String {
    he("его", "её")
}

// --------------------------------------------------------------------------
// How it speaks

pub fn say(message: &str) {
    let mut error = std::io::stderr();
    let _ = writeln!(error, "{message}");
    let _ = error.flush();
}

pub fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_secs_f64()).unwrap_or(0.0)
}

/// The local time of a moment: year, month (1-12), day, hour, minute, second.
pub fn local(timestamp: f64) -> (i32, u32, u32, u32, u32, u32) {
    let seconds = timestamp.floor() as libc::time_t;
    let mut parts: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&seconds, &mut parts) };
    (parts.tm_year + 1900, parts.tm_mon as u32 + 1, parts.tm_mday as u32, parts.tm_hour as u32, parts.tm_min as u32,
     parts.tm_sec as u32)
}

/// «14:05» today, «03.10 14:05» another day, «—» for no moment.
pub fn clock(timestamp: Option<f64>) -> String {
    let Some(moment) = timestamp.filter(|value| *value != 0.0) else { return "—".into() };
    let (year, month, day, hour, minute, _) = local(moment);
    let (this_year, this_month, today, _, _, _) = local(now());
    if (year, month, day) == (this_year, this_month, today) {
        format!("{hour:02}:{minute:02}")
    } else {
        format!("{day:02}.{month:02} {hour:02}:{minute:02}")
    }
}

/// Python's round(): halves go to the even neighbour.
pub fn round_half_even(value: f64) -> i64 {
    let floor = value.floor();
    let rest = value - floor;
    let rounded = if rest > 0.5 { floor + 1.0 } else if rest < 0.5 { floor } else if (floor as i64) % 2 == 0 { floor } else { floor + 1.0 };
    rounded as i64
}

pub fn minutes(seconds: f64) -> String {
    let total = round_half_even(seconds / 60.0).max(0);
    if total < 1 {
        "меньше минуты".into()
    } else if total < 60 {
        format!("{total} мин")
    } else {
        format!("{} ч {:02} мин", total / 60, total % 60)
    }
}

pub fn plural<'a>(count: i64, one: &'a str, few: &'a str, many: &'a str) -> &'a str {
    let tail = count.rem_euclid(100);
    if (11..=14).contains(&tail) {
        return many;
    }
    match count.rem_euclid(10) {
        1 => one,
        2..=4 => few,
        _ => many,
    }
}

pub fn items_word(count: i64) -> String {
    format!("{count} {}", plural(count, "пункт", "пункта", "пунктов"))
}

/// Whether what a link leads to is something to open: a file on this Mac, a page, another app's own address.
pub fn is_address(target: &str) -> bool {
    if target.starts_with('/') || target.starts_with("~/") {
        return true;
    }
    let mut characters = target.chars();
    let Some(first) = characters.next() else { return false };
    if !first.is_ascii_alphabetic() {
        return false;
    }
    let Some(colon) = target.find(':') else { return false };
    let scheme = &target[..colon];
    let rest = &target[colon + 1..];
    scheme.chars().skip(1).all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
        && !rest.is_empty()
        && !rest.chars().any(char::is_whitespace)
}

/// One link found in a text: where it stands (bytes), its words and what is in its round brackets, as written.
pub struct Found {
    pub start: usize,
    pub end: usize,
    pub words: String,
    pub target: String,
}

/// The links of a text, as `common.TEXT_LINK` finds them: `[слова](адрес)`, the words without `[`, `]` or a line
/// break, the address with no line break and round brackets only in pairs, one deep.
pub fn find_links(text: &str) -> Vec<Found> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut position = 0;
    while position < bytes.len() {
        if bytes[position] != b'[' {
            position += 1;
            continue;
        }
        if let Some(link) = link_at(text, position) {
            position = link.end;
            found.push(link);
        } else {
            position += 1;
        }
    }
    found
}

fn link_at(text: &str, start: usize) -> Option<Found> {
    let rest = &text[start + 1..];
    let close = rest.find(|c| c == '[' || c == ']' || c == '\n')?;
    if close == 0 || !rest[close..].starts_with(']') {
        return None;
    }
    let words = &rest[..close];
    let after = &rest[close + 1..];
    if !after.starts_with('(') {
        return None;
    }
    let target_text = &after[1..];
    let mut characters = target_text.char_indices().peekable();
    let mut end = 0;
    let mut pieces = 0;
    while let Some(&(index, c)) = characters.peek() {
        match c {
            '\n' | ')' => break,
            '(' => {
                let inner = &target_text[index + 1..];
                let stop = inner.find(|c| c == '(' || c == ')' || c == '\n')?;
                if !inner[stop..].starts_with(')') {
                    break;
                }
                let next = index + 1 + stop + 1;
                while characters.peek().is_some_and(|&(at, _)| at < next) {
                    characters.next();
                }
                end = next;
            }
            _ => {
                characters.next();
                end = index + c.len_utf8();
            }
        }
        pieces += 1;
    }
    if pieces == 0 || !target_text[end..].starts_with(')') {
        return None;
    }
    let whole = start + 1 + close + 1 + 1 + end + 1;
    Some(Found { start, end: whole, words: words.to_string(), target: target_text[..end].to_string() })
}

/// What stands in a text as a link: the words of each and its address, trimmed, in their order.
pub fn text_links(text: &str) -> Vec<(String, String)> {
    find_links(text).into_iter().map(|link| (link.words.trim().to_string(), link.target.trim().to_string())).collect()
}

/// A text as he reads it: every link by its words.
pub fn read_text(text: &str) -> String {
    let mut out = String::new();
    let mut last = 0;
    for link in find_links(text) {
        out.push_str(&text[last..link.start]);
        let words = link.words.trim();
        if !words.is_empty() && is_address(link.target.trim()) {
            out.push_str(words);
        } else {
            out.push_str(&text[link.start..link.end]);
        }
        last = link.end;
    }
    out.push_str(&text[last..]);
    out
}

/// Characters, as Python's len() counts them.
pub fn length(text: &str) -> usize {
    text.chars().count()
}

/// Every run of white space as one space, and none at the ends: Python's " ".join(text.split()).
pub fn squeeze(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

// --------------------------------------------------------------------------
// Files

pub fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// JSON as Python's json.dumps(value, ensure_ascii=False, indent=1) writes it.
pub fn dumps(value: &Value) -> String {
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, formatter);
    serde::Serialize::serialize(value, &mut serializer).expect("JSON");
    String::from_utf8(out).expect("UTF-8")
}

/// One line of JSON as Python's json.dumps(value, ensure_ascii=False) writes it: ", " and ": " between.
pub fn dumps_line(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let inner: Vec<String> = map.iter().map(|(key, value)| format!("{}: {}", Value::String(key.clone()), dumps_line(value))).collect();
            format!("{{{}}}", inner.join(", "))
        }
        Value::Array(items) => format!("[{}]", items.iter().map(dumps_line).collect::<Vec<_>>().join(", ")),
        other => other.to_string(),
    }
}

/// Written whole or not at all: beside, then renamed over.
pub fn write_json(path: &Path, value: &Value) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let name = path.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default();
    let temporary = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::write(&temporary, dumps(value))?;
    std::fs::rename(&temporary, path)
}

pub fn text_of<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

pub fn number_of(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(Value::as_f64)
}

pub fn list_of<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

/// Python's truthiness of a value read from JSON.
pub fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|value| value != 0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
    }
}

// --------------------------------------------------------------------------
// Programs

/// What a program printed, or None when it could not be started or outlived `seconds`.
pub struct Ran {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub fn run(command: &mut Command, input: Option<&[u8]>, seconds: f64) -> Option<Ran> {
    command.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().ok()?;
    if let Some(bytes) = input {
        let mut stdin = child.stdin.take()?;
        let bytes = bytes.to_vec();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&bytes);
        });
    }
    let mut out = child.stdout.take()?;
    let mut err = child.stderr.take()?;
    let reading = std::thread::spawn(move || {
        let mut text = Vec::new();
        let _ = out.read_to_end(&mut text);
        text
    });
    let reading_errors = std::thread::spawn(move || {
        let mut text = Vec::new();
        let _ = err.read_to_end(&mut text);
        text
    });
    let deadline = Instant::now() + Duration::from_secs_f64(seconds);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let stdout = String::from_utf8_lossy(&reading.join().unwrap_or_default()).to_string();
    let stderr = String::from_utf8_lossy(&reading_errors.join().unwrap_or_default()).to_string();
    Some(Ran { code: status.code().unwrap_or(-1), stdout, stderr })
}

/// What git printed, trimmed, or "" when it failed.
pub fn git(arguments: &[&str]) -> String {
    match run(Command::new("git").args(arguments), None, 10.0) {
        Some(done) if done.code == 0 => done.stdout.trim().to_string(),
        _ => String::new(),
    }
}

fn expand(folder: &str) -> PathBuf {
    match folder.strip_prefix("~/") {
        Some(rest) => user_home().join(rest),
        None if folder == "~" => user_home(),
        None => PathBuf::from(folder),
    }
}

/// The repository a folder is a checkout of, by the git folder its worktrees share; "" for none.
pub fn repository_of(folder: &str) -> String {
    let place = expand(folder);
    if folder.is_empty() || !place.is_dir() {
        return String::new();
    }
    let shared = git(&["-C", &place.to_string_lossy(), "rev-parse", "--path-format=absolute", "--git-common-dir"]);
    if shared.is_empty() {
        return String::new();
    }
    Path::new(&shared).canonicalize().map(|path| path.display().to_string()).unwrap_or(shared)
}

/// Whether an ended check was asked from a checkout of `repository` (as `repository_of` names it). A checkout
/// removed since, a worktree in a temporary folder wiped by a restart, no longer says whose it was: then the
/// check is of the repository when it has the project's name and its commit is one of the repository's.
pub fn asked_in(entry: &Value, repository: &str, known: &mut std::collections::HashMap<String, String>) -> bool {
    let folder = text_of(entry, "project").to_string();
    let found = known.entry(folder.clone()).or_insert_with(|| repository_of(&folder)).clone();
    if !found.is_empty() || repository.is_empty() {
        return !found.is_empty() && found == repository;
    }
    let (commit, name) = (text_of(entry, "commit"), text_of(entry, "projectName"));
    if folder.is_empty() || expand(&folder).exists() || commit.is_empty() || name.is_empty()
        || name != project_name(Some(repository)) {
        return false;
    }
    !git(&["--git-dir", repository, "rev-parse", "--verify", "--quiet", &format!("{commit}^{{commit}}")]).is_empty()
}

/// The project a check comes from, by the folder of its repository, or the folder's own name; "" for the home
/// folder and for no folder.
pub fn project_name(folder: Option<&str>) -> String {
    let start = match folder {
        Some(folder) if !folder.is_empty() => expand(folder),
        _ => match std::env::current_dir() {
            Ok(here) => here,
            Err(_) => return String::new(),
        },
    };
    let place = start.canonicalize().unwrap_or(start);
    let start = if place.is_dir() { place.clone() } else { place.parent().map(Path::to_path_buf).unwrap_or(place.clone()) };
    let shared = if start.is_dir() { git(&["-C", &start.to_string_lossy(), "rev-parse", "--git-common-dir"]) } else { String::new() };
    if !shared.is_empty() {
        let store = if Path::new(&shared).is_absolute() { PathBuf::from(&shared) } else { start.join(&shared) };
        let store = store.canonicalize().unwrap_or(store);
        let named = if store.file_name().is_some_and(|name| name == ".git") { store.parent().map(Path::to_path_buf).unwrap_or(store.clone()) } else { store };
        return named.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default();
    }
    if start == user_home() || start.parent().is_none() {
        return String::new();
    }
    start.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default()
}

/// A macOS notification, through osascript.
pub fn notify(title: &str, body: &str) {
    if var("CONVEYOR_NOTIFY").as_deref() == Some("0") {
        return;
    }
    let quoted = |text: &str| format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""));
    let script = format!("display notification {} with title {}", quoted(body), quoted(title));
    let _ = run(Command::new("osascript").args(["-e", &script]), None, 10.0);
}

// --------------------------------------------------------------------------
// The chat a call belongs to

pub fn plain_id(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// What the Claude app keeps about the chat this process runs in: none outside one.
pub fn claude_sessions(environment: &Env) -> Vec<Value> {
    let session = environment.get("CLAUDE_CODE_HOST_SESSION_ID").cloned().unwrap_or_default();
    if !plain_id(&session) {
        return Vec::new();
    }
    let root = environment.get("CONVEYOR_CLAUDE_SESSIONS_DIR").map(PathBuf::from)
        .unwrap_or_else(|| user_home().join("Library/Application Support/Claude/claude-code-sessions"));
    let mut found = Vec::new();
    for first in sorted_dir(&root) {
        for second in sorted_dir(&first) {
            if let Some(kept) = read_json(&second.join(format!("{session}.json"))).filter(Value::is_object) {
                found.push(kept);
            }
        }
    }
    found
}

pub fn sorted_dir(folder: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(folder).map(|entries| entries.flatten().map(|entry| entry.path()).collect()).unwrap_or_default();
    found.sort();
    found
}

/// The link that opens the chat this process runs in, "" for a chat that has none. hitl does not know how a program
/// opens its chats: the command in the settings (`chatbutton`) does.
pub fn chat_link(environment: &Env) -> String {
    crate::chatbutton::link_of(environment)
}

pub fn codex_state(environment: &Env) -> PathBuf {
    environment.get("CONVEYOR_CODEX_STATE").map(PathBuf::from).unwrap_or_else(|| user_home().join(".codex/state_5.sqlite"))
}

/// The database of Codex or OpenCode, opened for reading only; as a file nobody writes while no program of the app
/// has it open (its `-wal` beside it is gone then, and an ordinary read-only open fails).
pub fn read_only(database: &Path, seconds: f64) -> rusqlite::Result<rusqlite::Connection> {
    let still = if Path::new(&format!("{}-wal", database.display())).exists() { "" } else { "&immutable=1" };
    let address = format!("file:{}?mode=ro{still}", database.display());
    let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI;
    let connection = rusqlite::Connection::open_with_flags(address, flags)?;
    connection.busy_timeout(Duration::from_secs_f64(seconds))?;
    Ok(connection)
}

pub fn opencode_state(environment: &Env) -> PathBuf {
    if let Some(named) = environment.get("CONVEYOR_OPENCODE_STATE") {
        return PathBuf::from(named);
    }
    let data = environment.get("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| user_home().join(".local/share"));
    data.join("opencode/opencode.db")
}

fn first_line(text: &str) -> String {
    text.trim().lines().next().unwrap_or("").to_string()
}

/// The title of the chat of OpenCode this process runs in, or "".
pub fn opencode_title(environment: &Env) -> String {
    let mut chat = environment.get("OPENCODE_SESSION_ID").cloned().unwrap_or_default();
    let database = opencode_state(environment);
    if !plain_id(&chat) || !database.exists() {
        return String::new();
    }
    let Ok(connection) = read_only(&database, 1.0) else { return String::new() };
    let mut helper = false;
    for _ in 0..8 {
        let mut row: Option<(Option<String>, Option<String>)> = None;
        for table in ["session_v2", "session"] {
            let query = format!("select title, parent_id from {table} where id = ?");
            if let Ok(found) = connection.query_row(&query, [&chat], |row| Ok((row.get::<_, Option<String>>(0).ok().flatten(), row.get::<_, Option<String>>(1).ok().flatten()))) {
                row = Some(found);
                break;
            }
        }
        let Some((title, parent)) = row else { return String::new() };
        if let Some(parent) = parent.filter(|parent| !parent.trim().is_empty()) {
            chat = parent.trim().to_string();
            helper = true;
            continue;
        }
        let title = title.map(|title| first_line(&title)).unwrap_or_default();
        return if !title.is_empty() && helper { format!("{title} · помощник") } else { title };
    }
    String::new()
}

/// The title of the agent chat this process runs in, or "".
pub fn chat_title(environment: &Env) -> String {
    if let Some(named) = environment.get("CONVEYOR_CHAT") {
        return named.trim().to_string();
    }
    for kept in claude_sessions(environment) {
        let title = text_of(&kept, "title").trim();
        if !title.is_empty() {
            return title.to_string();
        }
    }
    let thread = environment.get("CODEX_THREAD_ID").cloned().unwrap_or_default();
    if plain_id(&thread) {
        let database = codex_state(environment);
        if database.exists() {
            if let Ok(connection) = read_only(&database, 1.0) {
                let row = connection.query_row("select name, title, source from threads where id = ?", [&thread], |row| {
                    Ok((row.get::<_, Option<String>>(0).ok().flatten(), row.get::<_, Option<String>>(1).ok().flatten(),
                        row.get::<_, Option<String>>(2).ok().flatten()))
                }).ok();
                let mut parent = None;
                if let Some((name, title, source)) = &row {
                    let named = [name, title].iter().any(|value| value.as_deref().is_some_and(|text| !text.trim().is_empty()));
                    if !named {
                        let spawned = source.as_deref().and_then(|text| serde_json::from_str::<Value>(text).ok())
                            .and_then(|value| value.pointer("/subagent/thread_spawn/parent_thread_id").and_then(Value::as_str).map(str::to_string));
                        if let Some(spawned) = spawned {
                            parent = connection.query_row("select name, title from threads where id = ?", [&spawned], |row| {
                                Ok((row.get::<_, Option<String>>(0).ok().flatten(), row.get::<_, Option<String>>(1).ok().flatten()))
                            }).ok();
                        }
                    }
                }
                if let Some((name, title, _)) = &row {
                    for value in [name, title] {
                        if let Some(text) = value.as_deref().filter(|text| !text.trim().is_empty()) {
                            return first_line(text);
                        }
                    }
                }
                if let Some((name, title)) = &parent {
                    for value in [name, title] {
                        if let Some(text) = value.as_deref().filter(|text| !text.trim().is_empty()) {
                            return format!("{} · помощник", first_line(text));
                        }
                    }
                }
            }
        }
    }
    opencode_title(environment)
}

/// Which app owns the chat that issued the check.
pub fn agent(environment: &Env) -> String {
    let set = |name: &str| environment.get(name).is_some_and(|value| !value.is_empty());
    if set("T3CODE_THREAD_ID") {
        return "T3 Code".into();
    }
    if ["CLAUDE_CODE_HOST_SESSION_ID", "CLAUDECODE", "CLAUDE_CODE_ENTRYPOINT"].iter().any(|name| set(name)) {
        return "Claude".into();
    }
    if environment.keys().any(|name| name.starts_with("CODEX_")) {
        return "Codex".into();
    }
    if set("OPENCODE") || set("OPENCODE_SESSION_ID") {
        return "OpenCode".into();
    }
    String::new()
}

/// The first characters of a chat title that tell it from the others: what stands before " · ", or the first
/// words that fit.
pub fn chat_prefix(title: &str, limit: usize) -> String {
    let title = squeeze(title);
    if title.is_empty() {
        return String::new();
    }
    let head = title.split(" · ").next().unwrap_or("");
    if head != title && length(head) <= limit {
        return head.to_string();
    }
    let mut words: Vec<&str> = Vec::new();
    for word in title.split(' ') {
        let longer = if words.is_empty() { word.to_string() } else { format!("{} {word}", words.join(" ")) };
        if !words.is_empty() && length(&longer) > limit {
            break;
        }
        words.push(word);
    }
    let joined: String = words.join(" ").chars().take(limit).collect();
    joined.trim_end().to_string()
}

pub fn chat_tag() -> String {
    chat_prefix(&chat_title(&env()), 16)
}

// --------------------------------------------------------------------------
// One history for every check that ended

pub fn history_file() -> PathBuf {
    home().join("history.jsonl")
}

pub fn screen_of(entry: &Value) -> String {
    match entry.get("screen").and_then(Value::as_str).filter(|screen| !screen.is_empty()) {
        Some(screen) => screen.to_string(),
        None => if entry.get("results").is_some() { "mac".into() } else { "iphone".into() },
    }
}

/// One line per ended check, kept for good.
pub fn record(entry: &Value) {
    let path = history_file();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{}", dumps_line(entry));
    }
    emit("hitl.recorded", json!({"request": text_of(entry, "id"), "source": format!("hitl.{}", screen_of(entry)),
                                 "outcome": text_of(entry, "how"), "context": {"items": list_of(entry, "results").len()}}));
}

pub fn history() -> Vec<Value> {
    let Ok(text) = std::fs::read_to_string(history_file()) else { return Vec::new() };
    text.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()).filter(Value::is_object).collect()
}

/// A number for a check that no other check has: two sent in one second must not replace each other.
pub fn new_id(tag: &str, taken: impl Fn(&str) -> bool) -> String {
    let (_, month, day, hour, minute, second) = local(now());
    let stamp = format!("{month:02}{day:02}-{hour:02}{minute:02}{second:02}");
    let slug: String = tag.chars().filter(|c| c.is_ascii_alphanumeric() || ('А'..='я').contains(c) || *c == 'Ё' || *c == 'ё').take(12).collect();
    let slug = if slug.is_empty() { "chat".to_string() } else { slug };
    let pid = std::process::id();
    for extra in std::iter::once(String::new()).chain((2..1000).map(|number| number.to_string())) {
        let suffix = if extra.is_empty() { String::new() } else { format!("-{extra}") };
        let ticket_id = format!("{stamp}-{:03}{suffix}-{slug}", pid % 1000);
        if !taken(&ticket_id) {
            return ticket_id;
        }
    }
    format!("{stamp}-{pid}-{slug}")
}

// --------------------------------------------------------------------------
// What only hitl has, asked of hitl

/// hitl's answer to a question only it can answer (`HITL_HOOK` is its command; nothing in hitl): Realize's
/// cards, the words the reviewer has tripped on, the phone's process, the hint to merge, what Claude's protection is
/// told. None outside hitl, or when hitl did not answer.
pub fn hook(name: &str, input: &Value, wait: bool) -> Option<Value> {
    let program = var("HITL_HOOK").filter(|value| !value.is_empty())?;
    let mut command = Command::new(&program);
    command.args(["_hitl", name]);
    if !wait {
        command.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());
        let mut child = command.spawn().ok()?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(input.to_string().as_bytes());
        }
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        return None;
    }
    let done = run(&mut command, Some(input.to_string().as_bytes()), 30.0)?;
    if done.code != 0 {
        return None;
    }
    serde_json::from_str(done.stdout.trim()).ok()
}

static EVENTS: Mutex<Vec<Value>> = Mutex::new(Vec::new());

/// Where the journal's events go in the process that runs hitl as its library: hitl's own program writes each
/// one to its action journal as it happens (`write`) and lets what it holds be written before the process ends
/// (`finish`). Without it, as in a program of hitl run as hitl's core by another, they are handed to hitl
/// at the end through `HITL_HOOK _hitl journal`.
pub struct Journal {
    pub write: fn(&Value),
    pub finish: fn(),
}

static JOURNAL: OnceLock<Journal> = OnceLock::new();

/// Sets where the journal's events go, once per process; a second call changes nothing.
pub fn set_journal(write: fn(&Value), finish: fn()) {
    let _ = JOURNAL.set(Journal { write, finish });
}

/// An event of the action journal: hitl keeps them, hitl has none. The event is its kind, the moment (`at`) and
/// the fields given: request, source, outcome, context, subject, error and the like.
pub fn emit(kind: &str, fields: Value) {
    if !is_host() || var("CONVEYOR_JOURNAL").as_deref() == Some("0") {
        return;
    }
    let mut event = Map::new();
    event.insert("kind".into(), Value::String(kind.into()));
    event.insert("at".into(), json!(now()));
    if let Value::Object(more) = fields {
        event.extend(more);
    }
    if let Some(journal) = JOURNAL.get() {
        (journal.write)(&Value::Object(event));
    } else if let Ok(mut events) = EVENTS.lock() {
        events.push(Value::Object(event));
    }
}

/// Hands the events of this run to hitl's journal, once, at its end: what the journal set in this process holds
/// is written, and what was kept for the hook goes to it.
pub fn flush_events() {
    if let Some(journal) = JOURNAL.get() {
        (journal.finish)();
    }
    let events: Vec<Value> = EVENTS.lock().map(|mut events| std::mem::take(&mut *events)).unwrap_or_default();
    if !events.is_empty() {
        hook("journal", &Value::Array(events), true);
    }
}

/// Ends the program with a code, after what it owes the journal.
pub fn exit(code: i32) -> ! {
    flush_events();
    std::process::exit(code)
}

/// A C string of a path, for the calls of libc.
pub fn c_path(path: &Path) -> Option<CString> {
    CString::new(path.as_os_str().as_encoded_bytes()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_read_by_their_words() {
        assert_eq!(read_text("см. [team/320](https://x.y/a(b)c) и [нет](не адрес)"), "см. team/320 и [нет](не адрес)");
        assert_eq!(text_links("[a](b) [c]( /d )"), vec![("a".into(), "b".into()), ("c".into(), "/d".into())]);
        assert!(text_links("[a](b").is_empty());
        assert!(text_links("[a]\n(b)").is_empty());
        assert_eq!(text_links("[a](x(y)z)(w)"), vec![("a".into(), "x(y)z".into())]);
        assert!(is_address("obsidian://open") && is_address("~/a") && !is_address("не адрес") && !is_address("a:"));
    }

    #[test]
    fn words_and_numbers_say_what_python_said() {
        assert_eq!(items_word(1), "1 пункт");
        assert_eq!(items_word(12), "12 пунктов");
        assert_eq!(items_word(23), "23 пункта");
        assert_eq!(minutes(20.0), "меньше минуты");
        assert_eq!(minutes(90.0), "2 мин");
        assert_eq!(minutes(3720.0), "1 ч 02 мин");
        assert_eq!(chat_prefix("r/47 · Бары внизу", 16), "r/47");
        assert_eq!(chat_prefix("Вынести hitl для команды", 16), "Вынести hitl для");
        assert_eq!(squeeze("  a \n b "), "a b");
    }
}
