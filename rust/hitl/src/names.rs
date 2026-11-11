//! `check --presentation`: what the window says of where each check comes from. The port of
//! `src/hitl/checknames.py` with what it needs of `chatmeta.py` and `chatrefs.py`. OWNER: part D.
//!
//! The window (`window/CheckNames.swift`) hands the checks it shows as a JSON list on the input and takes one label
//! for each: the chat's current name, the link that names the chat, and whether the chat was found as the one that
//! asked. Nothing is written and no program of an app is started: the Claude app's records, the `claude` command's
//! transcripts, Codex's and OpenCode's databases are only read, and the ticket keeps what it said when a lookup fails.
//! What Python raises is kept apart as Python keeps it: what the lookups catch, and what ends its command (`Crash`),
//! so that a broken record gives the same label, or the same failure, as `hitl check --presentation` gave.

use crate::common::{self, Env};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const APPS: [&str; 3] = ["Claude", "Codex", "OpenCode"];
/// How much of the input is read (checknames.main): a longer one is refused.
const LIMIT: usize = 1024 * 1024;
const MODES: [&str; 3] = ["auto", "manual", "native"];

// --------------------------------------------------------------------------
// What Python raised

#[derive(Debug)]
#[allow(dead_code)] // the message is for a reader of a failing test
enum Fault {
    /// OSError, ValueError, TypeError, sqlite3.Error or chatmeta.Unavailable: what the lookups catch.
    Caught(String),
    /// Anything else (an AttributeError of a broken record): nothing catches it, and the command ends with code 1.
    Crash(String),
}

fn caught(message: impl Into<String>) -> Fault {
    Fault::Caught(message.into())
}

impl From<std::io::Error> for Fault {
    fn from(error: std::io::Error) -> Self {
        caught(error.to_string())
    }
}

impl From<rusqlite::Error> for Fault {
    fn from(error: rusqlite::Error) -> Self {
        caught(error.to_string())
    }
}

// --------------------------------------------------------------------------
// A value as Python holds it: what JSON gives, or the bytes of a column of a database

#[derive(Clone, Debug, PartialEq)]
enum Py {
    Json(Value),
    Bytes(Vec<u8>),
}

impl Py {
    fn text(text: &str) -> Py {
        Py::Json(Value::String(text.to_string()))
    }

    fn of(value: Option<&Value>) -> Py {
        Py::Json(value.cloned().unwrap_or(Value::Null))
    }

    fn truthy(&self) -> bool {
        match self {
            Py::Json(value) => common::truthy(Some(value)),
            Py::Bytes(bytes) => !bytes.is_empty(),
        }
    }

    /// `value or ""`.
    fn or_empty(self) -> Py {
        if self.truthy() { self } else { Py::text("") }
    }

    fn string(&self) -> Option<&str> {
        match self {
            Py::Json(Value::String(text)) => Some(text),
            _ => None,
        }
    }

    /// A column as Python's sqlite3 reads it: a text that is no UTF-8 is an error of the database.
    fn cell(cell: rusqlite::types::ValueRef) -> Result<Py, Fault> {
        use rusqlite::types::ValueRef;
        Ok(match cell {
            ValueRef::Null => Py::Json(Value::Null),
            ValueRef::Integer(number) => Py::Json(Value::from(number)),
            ValueRef::Real(number) => Py::Json(Value::from(number)),
            ValueRef::Text(bytes) => Py::Json(Value::String(
                String::from_utf8(bytes.to_vec()).map_err(|_| caught("Could not decode to UTF-8"))?,
            )),
            ValueRef::Blob(bytes) => Py::Bytes(bytes.to_vec()),
        })
    }

    fn kind(&self) -> &'static str {
        match self {
            Py::Bytes(_) => "bytes",
            Py::Json(Value::Null) => "NoneType",
            Py::Json(Value::Bool(_)) => "bool",
            Py::Json(Value::Number(number)) if number.is_f64() => "float",
            Py::Json(Value::Number(_)) => "int",
            Py::Json(Value::String(_)) => "str",
            Py::Json(Value::Array(_)) => "list",
            Py::Json(Value::Object(_)) => "dict",
        }
    }
}

// --------------------------------------------------------------------------
// Characters and paths as Python reads them

/// Unicode's format characters (Cf), as Python 3.15's unicodedata (Unicode 17) lists them.
const FORMAT: [(u32, u32); 21] = [
    (0xAD, 0xAD), (0x600, 0x605), (0x61C, 0x61C), (0x6DD, 0x6DD), (0x70F, 0x70F), (0x890, 0x891), (0x8E2, 0x8E2),
    (0x180E, 0x180E), (0x200B, 0x200F), (0x202A, 0x202E), (0x2060, 0x2064), (0x2066, 0x206F), (0xFEFF, 0xFEFF),
    (0xFFF9, 0xFFFB), (0x110BD, 0x110BD), (0x110CD, 0x110CD), (0x13430, 0x1343F), (0x1BCA0, 0x1BCA3),
    (0x1D173, 0x1D17A), (0xE0001, 0xE0001), (0xE0020, 0xE007F),
];

/// A control (Cc) or format (Cf) character; a surrogate (Cs) cannot be in a Rust string.
pub(crate) fn hostile(character: char) -> bool {
    let code = character as u32;
    code < 0x20 || (0x7F..=0x9F).contains(&code) || FORMAT.iter().any(|&(first, last)| (first..=last).contains(&code))
}

/// `str.isspace()`: Unicode's white space and the four separators of files, groups, records and units.
pub(crate) fn py_space(character: char) -> bool {
    character.is_whitespace() || ('\u{1C}'..='\u{1F}').contains(&character)
}

/// `str.strip()`.
pub(crate) fn py_strip(text: &str) -> &str {
    text.trim_matches(py_space)
}

/// Where `str.splitlines()` ends a line.
fn line_break(character: char) -> bool {
    matches!(character, '\n' | '\u{B}' | '\u{C}' | '\r' | '\u{1C}' | '\u{1D}' | '\u{1E}' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

/// `float.__repr__`: the shortest digits, with the exponent Python writes («1e+16», «1e-05»).
pub(crate) fn py_float(value: f64) -> String {
    let text = format!("{value:?}");
    match text.split_once('e') {
        None => text,
        Some((mantissa, exponent)) => {
            let (sign, digits) = match exponent.strip_prefix('-') {
                Some(digits) => ('-', digits),
                None => ('+', exponent),
            };
            format!("{mantissa}e{sign}{digits:0>2}")
        }
    }
}

/// The folder `Path.home()` gives: `HOME` without its last slash ("/" when it is empty), else the user's own.
pub(crate) fn py_home() -> String {
    let home = match std::env::var_os("HOME") {
        Some(home) => home.to_string_lossy().into_owned(),
        None => {
            // SAFETY: getpwuid returns a pointer into static storage or null; it is read at once.
            let entry = unsafe { libc::getpwuid(libc::getuid()) };
            if entry.is_null() || unsafe { (*entry).pw_dir }.is_null() {
                return "~".into();
            }
            unsafe { std::ffi::CStr::from_ptr((*entry).pw_dir) }.to_string_lossy().into_owned()
        }
    };
    let home = home.trim_end_matches('/');
    if home.is_empty() { "/".into() } else { home.into() }
}

/// `str(PurePosixPath(text))`: no empty or "." parts, no slash at the end, "//" kept only as two at the start.
pub(crate) fn py_path(text: &str) -> String {
    let (root, rest) = if !text.starts_with('/') {
        ("", text)
    } else if !text[1..].starts_with('/') || text[2..].starts_with('/') {
        ("/", &text[1..])
    } else {
        ("//", &text[2..])
    };
    let parts: Vec<&str> = rest.split('/').filter(|part| !part.is_empty() && *part != ".").collect();
    let joined = format!("{root}{}", parts.join("/"));
    if joined.is_empty() { ".".into() } else { joined }
}

/// `Path(base) / name`, as its text.
pub(crate) fn py_join(base: &str, name: &str) -> String {
    if name.starts_with('/') {
        return py_path(name);
    }
    if base.is_empty() || base.ends_with('/') { py_path(&format!("{base}{name}")) } else { py_path(&format!("{base}/{name}")) }
}

/// `os.path.realpath(text)`, as Python 3.13 and later resolve it: every link that can be read is followed, a part
/// that is not there is kept as written, and ".." takes the last part off what is resolved so far.
pub(crate) fn realpath(text: &str) -> Result<String, String> {
    if text.contains('\0') {
        return Err("embedded null byte".into());
    }
    let mut path = if text.starts_with('/') {
        "/".to_string()
    } else {
        std::env::current_dir().map_err(|error| error.to_string())?.to_string_lossy().into_owned()
    };
    // What is left to resolve, last part first; None marks a link whose target is resolved once it is popped.
    let mut rest: Vec<Option<String>> = text.trim_end_matches('/').split('/').rev().map(|part| Some(part.to_string())).collect();
    let mut count = rest.len();
    let mut seen: HashMap<String, Option<String>> = HashMap::new();
    while count > 0 {
        let Some(name) = rest.pop() else { break };
        let Some(name) = name else {
            if let Some(Some(link)) = rest.pop() {
                seen.insert(link, Some(path.clone()));
            }
            continue;
        };
        count -= 1;
        if name.is_empty() || name == "." {
            continue;
        }
        if name == ".." {
            path = match path.rfind('/') {
                Some(0) | None => "/".into(),
                Some(end) => path[..end].to_string(),
            };
            continue;
        }
        let newpath = if path == "/" { format!("/{name}") } else { format!("{path}/{name}") };
        match std::fs::symlink_metadata(&newpath) {
            Ok(meta) if meta.file_type().is_symlink() => {}
            _ => {
                path = newpath;
                continue;
            }
        }
        if let Some(known) = seen.get(&newpath) {
            path = known.clone().unwrap_or_else(|| newpath.clone());
            continue;
        }
        let target = match std::fs::read_link(&newpath) {
            Ok(target) => target.to_string_lossy().into_owned(),
            Err(_) => {
                path = newpath;
                continue;
            }
        };
        if target.starts_with('/') {
            path = "/".into();
        }
        seen.insert(newpath.clone(), None);
        rest.push(Some(newpath));
        rest.push(None);
        let parts: Vec<&str> = target.split('/').collect();
        count += parts.len();
        rest.extend(parts.into_iter().rev().map(|part| Some(part.to_string())));
    }
    Ok(path)
}

/// The entries of a folder in the order the system lists them, as `pathlib.Path.glob` meets them; none for a
/// folder that cannot be read.
fn listing(folder: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(folder).map(|entries| entries.flatten().map(|entry| entry.path()).collect()).unwrap_or_default()
}

/// The folders in a folder, links to folders too: the parts of a pattern before its last.
fn folders(folder: &Path) -> Vec<PathBuf> {
    listing(folder).into_iter().filter(|path| path.is_dir()).collect()
}

/// Whether an entry is what `*SUFFIX` matches: anything of that name, a folder or a broken link too.
fn ends_with(path: &Path, suffix: &str) -> bool {
    path.file_name().is_some_and(|name| name.as_encoded_bytes().ends_with(suffix.as_bytes()))
}

/// `json.loads(path.read_text(encoding="utf-8"))`: any failure is an error the lookups catch.
fn read_strict(path: &Path) -> Result<Value, Fault> {
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8(bytes).map_err(|_| caught("'utf-8' codec can't decode"))?;
    serde_json::from_str(&text).map_err(|error| caught(error.to_string()))
}

/// The bytes `json.loads` was handed, as the text it reads: UTF-8, with or without its mark, or UTF-16 and UTF-32
/// told by their marks or by the zeros of their first characters (`json.detect_encoding`).
fn json_text(bytes: &[u8]) -> Option<String> {
    fn units<const N: usize>(bytes: &[u8], big: bool) -> Option<Vec<u32>> {
        if bytes.len() % N != 0 {
            return None;
        }
        Some(bytes.chunks(N).map(|chunk| {
            let mut value = 0u32;
            for index in 0..N {
                let byte = if big { chunk[index] } else { chunk[N - 1 - index] };
                value = (value << 8) | byte as u32;
            }
            value
        }).collect())
    }
    fn utf16(bytes: &[u8], big: bool) -> Option<String> {
        let units: Vec<u16> = units::<2>(bytes, big)?.into_iter().map(|unit| unit as u16).collect();
        String::from_utf16(&units).ok()
    }
    fn utf32(bytes: &[u8], big: bool) -> Option<String> {
        units::<4>(bytes, big)?.into_iter().map(char::from_u32).collect()
    }
    let starts = |prefix: &[u8]| bytes.starts_with(prefix);
    if starts(&[0, 0, 0xFE, 0xFF]) {
        return utf32(&bytes[4..], true);
    }
    if starts(&[0xFF, 0xFE, 0, 0]) {
        return utf32(&bytes[4..], false);
    }
    if starts(&[0xFE, 0xFF]) {
        return utf16(&bytes[2..], true);
    }
    if starts(&[0xFF, 0xFE]) {
        return utf16(&bytes[2..], false);
    }
    if starts(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8(bytes[3..].to_vec()).ok();
    }
    if bytes.len() >= 4 {
        if bytes[0] == 0 {
            return if bytes[1] != 0 { utf16(bytes, true) } else { utf32(bytes, true) };
        }
        if bytes[1] == 0 {
            return if bytes[2] != 0 || bytes[3] != 0 { utf16(bytes, false) } else { utf32(bytes, false) };
        }
    } else if bytes.len() == 2 {
        if bytes[0] == 0 {
            return utf16(bytes, true);
        }
        if bytes[1] == 0 {
            return utf16(bytes, false);
        }
    }
    String::from_utf8(bytes.to_vec()).ok()
}

// --------------------------------------------------------------------------
// The identity of a chat (chatrefs)

/// `chatrefs.component`: an id that is a text, not empty, not only dots, with no control or format character.
fn component(value: &Py) -> Result<String, Fault> {
    match value.string() {
        Some(text) if !text.is_empty() && !text.chars().all(|c| c == '.') && !text.chars().any(hostile) => Ok(text.to_string()),
        _ => Err(caught("пустой или недопустимый идентификатор чата")),
    }
}

fn app_name(value: &str) -> Result<&'static str, Fault> {
    let lower = value.to_lowercase();
    APPS.iter().copied().find(|name| name.to_lowercase() == lower)
        .ok_or_else(|| caught("приложение должно быть Claude, Codex или OpenCode"))
}

/// `urllib.parse.quote(text, safe="")`.
fn quote(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"_.-~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// `Reference.uri`: the link that names a chat, conveyor://chat/APP/ID.
pub(crate) fn uri(app: &str, id: &str) -> String {
    format!("conveyor://chat/{}/{}", quote(app), quote(id))
}

/// `chatrefs.decode`: a part of a link, every "%" followed by two hex digits, decoded to UTF-8.
fn decode(segment: &str) -> Result<String, Fault> {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let pair = bytes.get(index + 1..index + 3).filter(|pair| pair.iter().all(u8::is_ascii_hexdigit));
            let Some(pair) = pair else { return Err(caught("неверное percent-кодирование ссылки")) };
            let pair = std::str::from_utf8(pair).unwrap_or("00");
            out.push(u8::from_str_radix(pair, 16).unwrap_or(0));
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    let text = String::from_utf8(out).map_err(|_| caught("ссылка содержит неверный UTF-8"))?;
    component(&Py::text(&text))
}

/// `chatrefs.parse`: conveyor://chat/APP/ID, its two words in any case, as the app and the id it names.
fn parse(value: &Py) -> Result<(&'static str, String), Fault> {
    let refused = || caught("недопустимая ссылка на чат");
    let Some(text) = value.string() else { return Err(refused()) };
    if text.chars().any(|c| hostile(c) || c == '?' || c == '#') {
        return Err(refused());
    }
    let bytes = text.as_bytes();
    let head = bytes.len() > 16 && bytes[..8].eq_ignore_ascii_case(b"conveyor") && &bytes[8..11] == b"://"
        && bytes[11..15].eq_ignore_ascii_case(b"chat") && bytes[15] == b'/';
    let parts: Vec<&str> = if head { text[16..].split('/').collect() } else { Vec::new() };
    if parts.len() != 2 || parts.iter().any(|part| part.is_empty()) {
        return Err(caught("нужна ссылка conveyor://chat/APP/ID"));
    }
    let app = decode(parts[0])?;
    let id = decode(parts[1])?;
    Ok((app_name(&app)?, id))
}

/// A link that names a chat, conveyor://chat/APP/ID, read as the app and the id; what is wrong with it otherwise.
pub fn parse_link(value: &str) -> Result<(&'static str, String), String> {
    parse(&Py::text(value)).map_err(|fault| match fault {
        Fault::Caught(said) | Fault::Crash(said) => said,
    })
}

/// What the "Чат" button of the window and of the phone's app opens. A reference `conveyor://chat/APP/ID` named a chat
/// to the app hitl had on this Mac, which is gone (2026-11-03): it opens nothing now. Any other reference opens as
/// it stands; else the address the check carries (`chat_link`), which the setting of the button gave it when it was
/// queued (`chatbutton`). hitl does not know what any program's address looks like. "" is: nothing to open.
pub fn open_link(reference: &str, chat_link: &str) -> String {
    let usable = |link: &str| {
        let link = link.trim();
        if link.to_lowercase().starts_with("conveyor:") { String::new() } else { link.to_string() }
    };
    let own = usable(reference);
    if own.is_empty() { usable(chat_link) } else { own }
}

// --------------------------------------------------------------------------
// Where each app keeps its chats

fn setting(environment: &Env, name: &str) -> Option<String> {
    environment.get(name).filter(|value| !value.is_empty()).cloned()
}

/// hitl's own state: `CONVEYOR_HOME`, else the home of this command.
fn state(environment: &Env) -> PathBuf {
    setting(environment, "CONVEYOR_HOME").map(PathBuf::from).unwrap_or_else(common::home)
}

fn sessions_root(environment: &Env) -> PathBuf {
    setting(environment, "CONVEYOR_CLAUDE_SESSIONS_DIR").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(py_join(&py_home(), "Library/Application Support/Claude/claude-code-sessions")))
}

fn projects_root(environment: &Env) -> PathBuf {
    setting(environment, "CONVEYOR_CLAUDE_PROJECTS_DIR").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(py_join(&py_home(), ".claude/projects")))
}

fn codex_database(environment: &Env) -> PathBuf {
    setting(environment, "CONVEYOR_CODEX_STATE").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(py_join(&py_home(), ".codex/state_5.sqlite")))
}

fn opencode_database(environment: &Env) -> PathBuf {
    if let Some(named) = setting(environment, "CONVEYOR_OPENCODE_STATE") {
        return PathBuf::from(named);
    }
    let data = setting(environment, "XDG_DATA_HOME").unwrap_or_else(|| py_join(&py_home(), ".local/share"));
    PathBuf::from(py_join(&data, "opencode/opencode.db"))
}

/// What hitl wrote down of the chats it started (`spawned/*.json`); an unreadable one fails the lookup.
fn marks(environment: &Env) -> Result<Vec<Map<String, Value>>, Fault> {
    let mut found = Vec::new();
    for path in listing(&state(environment).join("spawned")) {
        if ends_with(&path, ".json") {
            if let Value::Object(mark) = read_strict(&path)? {
                found.push(mark);
            }
        }
    }
    Ok(found)
}

/// One chat as its app keeps it.
struct Row {
    id: Py,
    native: Py,
    display: Py,
    project: Py,
    archived: bool,
}

impl Row {
    /// One of the two rows an id that names two chats is answered with.
    fn either(id: &str) -> Row {
        Row { id: Py::text(id), native: Py::text(""), display: Py::text(""), project: Py::text(""), archived: false }
    }
}

fn columns(connection: &rusqlite::Connection, table: &str) -> Result<Vec<String>, Fault> {
    let mut statement = connection.prepare(&format!("pragma table_info({table})"))?;
    let names = statement.query_map([], |row| row.get::<_, String>(1))?.collect::<Result<Vec<_>, _>>()?;
    Ok(names)
}

/// The first row a query of one id gives, its columns as Python reads them.
fn select(connection: &rusqlite::Connection, table: &str, fields: &[&str], id: &str) -> Result<Option<Vec<Py>>, Fault> {
    let have = columns(connection, table)?;
    let fields: Vec<String> = fields.iter().map(|name| {
        if have.iter().any(|column| column == name) { name.to_string() } else { format!("null as {name}") }
    }).collect();
    let query = format!("select {} from {table} where id = ?", fields.join(","));
    let mut statement = connection.prepare(&query)?;
    let mut rows = statement.query([id])?;
    let Some(row) = rows.next()? else { return Ok(None) };
    (0..fields.len()).map(|index| Py::cell(row.get_ref(index)?)).collect::<Result<Vec<_>, _>>().map(Some)
}

/// `tell.parent_of`: the chat that started a helper of Codex, from what Codex keeps of where the helper came from.
fn parent_of(source: &Py) -> String {
    let parent = loads(source).and_then(|value| value.get("subagent")?.get("thread_spawn")?.get("parent_thread_id").cloned());
    match parent {
        Some(Value::String(parent)) if !parent.is_empty() && parent.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') => parent,
        _ => String::new(),
    }
}

/// `json.loads(value)` of a text or of bytes; None for what it raises.
fn loads(value: &Py) -> Option<Value> {
    match value {
        Py::Json(Value::String(text)) => serde_json::from_str(text).ok(),
        Py::Bytes(bytes) => serde_json::from_str(&json_text(bytes)?).ok(),
        _ => None,
    }
}

/// What `str.strip()` or `bytes.strip()` leaves; a number has no strip, and Python's command ends there.
enum Words {
    Text(String),
    Bytes(Vec<u8>),
}

impl Words {
    fn empty(&self) -> bool {
        match self {
            Words::Text(text) => text.is_empty(),
            Words::Bytes(bytes) => bytes.is_empty(),
        }
    }
}

fn strip(value: &Py) -> Result<Words, Fault> {
    match value {
        Py::Json(Value::String(text)) => Ok(Words::Text(py_strip(text).to_string())),
        Py::Bytes(bytes) => {
            let space = |byte: &u8| b" \t\n\r\x0b\x0c".contains(byte);
            let start = bytes.iter().position(|byte| !space(byte)).unwrap_or(bytes.len());
            let end = bytes.iter().rposition(|byte| !space(byte)).map_or(start, |end| end + 1);
            Ok(Words::Bytes(bytes[start..end.max(start)].to_vec()))
        }
        other => Err(Fault::Crash(format!("AttributeError: '{}' object has no attribute 'strip'", other.kind()))),
    }
}

/// `codexapp.heading`: what a chat of Codex is called, its name, else the title Codex gave it, and a helper with
/// neither by its task and nickname; the first line of it, its spaces squeezed. Each value is `value or ""`.
fn heading(name: &Py, title: &Py, source: &Py, nickname: &Py) -> Result<String, Fault> {
    let mut words = strip(name)?;
    if words.empty() {
        words = strip(title)?;
    }
    if words.empty() && !parent_of(source).is_empty() {
        let task = loads(source).and_then(|value| value.get("subagent")?.get("thread_spawn").cloned());
        if let Some(Value::Object(spawn)) = task {
            // A path that is no text has no rsplit: Python passes over it.
            if let Py::Json(Value::String(task)) = Py::of(spawn.get("agent_path")).or_empty() {
                let last = task.rsplit('/').next().unwrap_or("");
                words = Words::Text(last.replace('_', " "));
            }
        }
        let mut parts: Vec<String> = Vec::new();
        for part in [match words { Words::Text(text) => Py::text(&text), Words::Bytes(bytes) => Py::Bytes(bytes) }, nickname.clone()] {
            if !part.truthy() {
                continue;
            }
            match part.string() {
                Some(text) => parts.push(text.to_string()),
                None => return Err(caught(format!("sequence item: expected str instance, {} found", part.kind()))),
            }
        }
        words = Words::Text(parts.join(" · "));
    }
    match words {
        Words::Bytes(bytes) if !bytes.is_empty() => Err(caught("sequence item 0: expected str instance, bytes found")),
        Words::Text(text) if !text.is_empty() => {
            let line: String = text.chars().take_while(|c| !line_break(*c)).collect();
            Ok(line.split(py_space).filter(|word| !word.is_empty()).collect::<Vec<_>>().join(" "))
        }
        _ => Ok(String::new()),
    }
}

/// `chatmeta.codex`: a chat of Codex by its id, from Codex's database.
fn codex(id: &str, environment: &Env) -> Result<Vec<Row>, Fault> {
    let database = codex_database(environment);
    if !database.is_file() {
        return Err(caught("хранилище Codex недоступно"));
    }
    let connection = common::read_only(&database, 8.0)?;
    let fields = ["id", "title", "name", "cwd", "source", "archived", "agent_nickname"];
    let Some(cells) = select(&connection, "threads", &fields, id)? else { return Ok(Vec::new()) };
    let [ident, title, name, folder, source, archived, nickname]: [Py; 7] = cells.try_into().map_err(|_| caught("row"))?;
    let native = if name.truthy() { name.clone() } else { title.clone().or_empty() };
    let display = heading(&name.or_empty(), &title.or_empty(), &source.or_empty(), &nickname.or_empty())?;
    Ok(vec![Row { id: ident, native, display: Py::text(&display), project: folder.or_empty(), archived: archived.truthy() }])
}

/// `opencodeapp.archived_mark`: whether hitl marked a chat of OpenCode archived in its metadata.
fn archived_mark(metadata: &Py) -> bool {
    match metadata.string().map(serde_json::from_str::<Value>) {
        Some(Ok(Value::Object(map))) => common::truthy(map.get("conveyorArchivedAt")),
        _ => false,
    }
}

/// `chatmeta.opencode`: a chat of OpenCode by its id, from the first table of its database that is there.
fn opencode(id: &str, environment: &Env) -> Result<Vec<Row>, Fault> {
    let database = opencode_database(environment);
    if !database.is_file() {
        return Err(caught("хранилище OpenCode недоступно"));
    }
    let connection = common::read_only(&database, 2.0)?;
    for table in ["session_v2", "session"] {
        if columns(&connection, table)?.is_empty() {
            continue;
        }
        let fields = ["id", "title", "directory", "parent_id", "time_archived", "metadata"];
        let Some(cells) = select(&connection, table, &fields, id)? else { return Ok(Vec::new()) };
        let [ident, title, folder, _parent, archived, metadata]: [Py; 6] = cells.try_into().map_err(|_| caught("row"))?;
        let title = title.or_empty();
        let archived = archived != Py::Json(Value::Null) || archived_mark(&metadata);
        return Ok(vec![Row { id: ident, native: title.clone(), display: title, project: folder.or_empty(), archived }]);
    }
    Err(caught("неизвестная схема хранилища OpenCode"))
}

/// `ref.id in (value or [])`, as Python tests it: a text holds the id within it, a list as one of its items, a
/// dictionary as one of its keys; a number has no such test and the lookup fails.
fn holds(value: Option<&Value>, id: &str) -> Result<bool, Fault> {
    let value = value.unwrap_or(&Value::Null);
    if !common::truthy(Some(value)) {
        return Ok(false);
    }
    match value {
        Value::String(text) => Ok(text.contains(id)),
        Value::Array(items) => Ok(items.iter().any(|item| item.as_str() == Some(id))),
        Value::Object(map) => Ok(map.contains_key(id)),
        _ => Err(caught("argument of type is not iterable")),
    }
}

/// The lines of a file as Python's text mode gives them: ended by "\n", "\r\n" or "\r".
fn lines(bytes: &[u8]) -> Vec<&[u8]> {
    let mut found = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\n' => {
                found.push(&bytes[start..index]);
                index += 1;
                start = index;
            }
            b'\r' => {
                found.push(&bytes[start..index]);
                index += if bytes.get(index + 1) == Some(&b'\n') { 2 } else { 1 };
                start = index;
            }
            _ => index += 1,
        }
    }
    if start < bytes.len() {
        found.push(&bytes[start..]);
    }
    found
}

/// `chatmeta.claude`: a chat of Claude by the Claude app's canonical id or the current id of its command, else by
/// the transcript of a chat of the `claude` command alone.
fn claude(id: &str, environment: &Env) -> Result<Vec<Row>, Fault> {
    let sessions = sessions_root(environment);
    let mut records: Vec<Map<String, Value>> = Vec::new();
    for first in folders(&sessions) {
        for second in folders(&first) {
            for path in listing(&second) {
                if !ends_with(&path, ".json") {
                    continue;
                }
                if let Value::Object(row) = read_strict(&path)? {
                    if row.get("sessionId").is_some_and(Value::is_string) {
                        records.push(row);
                    }
                }
            }
        }
    }
    let wanted = Value::String(id.to_string());
    let mut found = Vec::new();
    for row in &records {
        if row.get("sessionId") != Some(&wanted) && row.get("cliSessionId") != Some(&wanted) {
            continue;
        }
        found.push(Row {
            id: Py::of(row.get("sessionId")),
            native: Py::of(row.get("title")).or_empty(),
            display: Py::of(row.get("title")).or_empty(),
            project: Py::of(row.get("cwd")).or_empty(),
            archived: common::truthy(row.get("isArchived")),
        });
    }
    if !found.is_empty() {
        return Ok(found);
    }
    for row in &records {
        if holds(row.get("priorCliSessionIds"), id)? {
            return Ok(Vec::new()); // a past id names no chat to deliver to
        }
    }
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Ok(Vec::new());
    }
    let projects = projects_root(environment);
    let paths: Vec<PathBuf> = folders(&projects).into_iter().map(|folder| folder.join(format!("{id}.jsonl")))
        .filter(|path| std::fs::symlink_metadata(path).is_ok()).collect();
    let saved: Vec<Map<String, Value>> = marks(environment)?.into_iter().filter(|mark| {
        mark.get("app") == Some(&Value::String("Claude".into())) && mark.get("id") == Some(&wanted) && common::truthy(mark.get("hosted"))
    }).collect();
    if paths.is_empty() {
        if !sessions.exists() && !projects.exists() {
            return Err(caught("хранилище Claude недоступно"));
        }
        return Ok(Vec::new());
    }
    if paths.len() > 1 || saved.len() > 1 {
        return Ok(vec![Row::either(id), Row::either(id)]);
    }
    let (mut native, mut folder, mut identity) = (String::new(), String::new(), false);
    let bytes = std::fs::read(&paths[0])?;
    for line in lines(&bytes) {
        let line = std::str::from_utf8(line).map_err(|_| caught("'utf-8' codec can't decode"))?;
        let Ok(row) = serde_json::from_str::<Value>(line) else { continue };
        let Value::Object(row) = row else {
            return Err(Fault::Crash(format!("AttributeError: '{}' object has no attribute 'get'", Py::Json(row).kind())));
        };
        if row.get("sessionId") == Some(&wanted) {
            identity = true;
        }
        if row.get("type") == Some(&Value::String("custom-title".into())) {
            if let Some(Value::String(title)) = row.get("customTitle") {
                native = title.clone();
            }
        }
        if let Some(Value::String(cwd)) = row.get("cwd") {
            folder = cwd.clone();
        }
    }
    if !identity {
        return Err(caught("Claude не подтвердил идентификатор в нативной записи чата"));
    }
    let mark = saved.into_iter().next().unwrap_or_default();
    let title = if common::truthy(mark.get("title")) { Py::of(mark.get("title")) } else { Py::text(&native) };
    let project = if common::truthy(mark.get("folder")) { Py::of(mark.get("folder")) } else { Py::text(&folder) };
    let hidden_file = state(environment).join("chats").join("hidden.json");
    let hidden = if hidden_file.exists() { read_strict(&hidden_file)? } else { Value::Array(Vec::new()) };
    let archived = hidden.as_array().is_some_and(|hidden| hidden.contains(&wanted));
    Ok(vec![Row { id: Py::text(id), native: Py::text(&native), display: title, project, archived }])
}

// --------------------------------------------------------------------------
// One chat's metadata, and its name kept by hitl (chatmeta.native_info, info, project)

/// What `chatmeta.info` says of a chat, as much of it as a label needs.
struct Info {
    status: String,
    id: String,
    display: Py,
    project: Py,
    link: String,
}

fn native_info(app: &'static str, id: &str, environment: &Env) -> Result<(Info, Py), String> {
    let mut info = Info { status: "not_found".into(), id: id.to_string(), display: Py::text(""), project: Py::text(""), link: uri(app, id) };
    let mut native = Py::text("");
    let found = (|| -> Result<(), Fault> {
        let rows = match app {
            "Codex" => codex(id, environment)?,
            "OpenCode" => opencode(id, environment)?,
            _ => claude(id, environment)?,
        };
        if rows.len() > 1 {
            info.status = "ambiguous".into();
            return Ok(());
        }
        let Some(row) = rows.into_iter().next() else { return Ok(()) };
        (info.display, info.project, native) = (row.display, row.project, row.native);
        let canonical = component(&row.id)?;
        info.link = uri(app, &canonical);
        info.status = if row.archived { "archived".into() } else { "active".into() };
        info.id = canonical;
        let saved = marks(environment)?.into_iter().filter(|mark| {
            mark.get("app") == Some(&Value::String(app.into())) && mark.get("id") == Some(&Value::String(info.id.clone()))
        }).count();
        if saved > 1 {
            return Err(caught("сохранённые метаданные чата неоднозначны"));
        }
        Ok(())
    })();
    match found {
        Ok(()) => Ok((info, native)),
        Err(Fault::Caught(_)) => {
            info.status = "backend_unavailable".into();
            Ok((info, native))
        }
        Err(Fault::Crash(message)) => Err(message),
    }
}

/// The file hitl keeps a chat's name in: `chats/names/<sha256 of ["APP", "ID"]>.json`.
fn names_file(app: &str, id: &str, environment: &Env) -> PathBuf {
    let key = format!("[{}, {}]", Value::String(app.into()), Value::String(id.into()));
    let digest: String = Sha256::digest(key.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect();
    state(environment).join("chats").join("names").join(format!("{digest}.json"))
}

/// `chatmeta.read`: how a chat is named, its mode and the name hitl gave it; a file that does not hold what
/// hitl writes there fails the lookup.
fn kept_name(app: &str, id: &str, environment: &Env) -> Result<(String, String), Fault> {
    let file = names_file(app, id, environment);
    if !file.exists() {
        return Ok(("auto".into(), String::new()));
    }
    let broken = || caught("неизвестное или повреждённое состояние имени Conveyor");
    let Value::Object(value) = read_strict(&file)? else { return Err(broken()) };
    let one = match value.get("version") {
        Some(Value::Number(number)) => number.as_f64() == Some(1.0),
        Some(Value::Bool(flag)) => *flag,
        _ => false,
    };
    let whole = |key: &str| match value.get(key) {
        Some(Value::Number(number)) => number.is_u64() || number.as_i64().is_some_and(|number| number >= 0),
        _ => false,
    };
    let dictionary = |key: &str| value.get(key).is_none_or(Value::is_object);
    let mode = value.get("mode").and_then(Value::as_str).filter(|mode| MODES.contains(mode));
    let valid = one && value.get("app") == Some(&Value::String(app.into())) && value.get("id") == Some(&Value::String(id.into()))
        && mode.is_some() && whole("revision") && whole("generation") && value.get("title").is_some_and(Value::is_string)
        && value.get("decisions").is_some_and(Value::is_object) && dictionary("events") && dictionary("task") && dictionary("latestTask");
    if !valid {
        return Err(broken());
    }
    Ok((mode.unwrap_or("auto").to_string(), value.get("title").and_then(Value::as_str).unwrap_or("").to_string()))
}

/// `chatmeta.info`: the chat as its app keeps it, with the name hitl keeps for it; the error of a Python
/// exception nothing catches.
fn info(app: &'static str, id: &str, environment: &Env) -> Result<Info, String> {
    let (mut info, native) = native_info(app, id, environment)?;
    if info.status != "active" && info.status != "archived" {
        return Ok(info);
    }
    match kept_name(app, &info.id, environment) {
        Ok((mode, title)) => {
            if mode != "native" && !title.is_empty() {
                info.display = Py::text(&title);
            } else if mode == "native" && native.truthy() {
                info.display = native;
            }
        }
        Err(Fault::Caught(_)) => info.status = "backend_unavailable".into(),
        Err(Fault::Crash(message)) => return Err(message),
    }
    Ok(info)
}

// --------------------------------------------------------------------------
// The label (checknames)

/// The folder a value names, as `Path(value)` takes it: a text, else Python's TypeError.
fn folder_text(value: &Py) -> Result<&str, Fault> {
    value.string().ok_or_else(|| caught(format!("expected str, bytes or os.PathLike object, not {}", value.kind())))
}

/// `checknames.same_checkout`: the same folder, or two folders of one git checkout; a worktree of the same
/// repository is another checkout.
fn same_checkout(left: &Py, right: &Py) -> Result<bool, Fault> {
    if !left.truthy() || !right.truthy() {
        return Ok(false);
    }
    let left = folder_text(left)?;
    if !left.starts_with('/') {
        return Ok(false);
    }
    let right = folder_text(right)?;
    if !right.starts_with('/') {
        return Ok(false);
    }
    let a = realpath(&py_path(left)).map_err(caught)?;
    let b = realpath(&py_path(right)).map_err(caught)?;
    if a == b {
        return Ok(true);
    }
    if !Path::new(&a).is_dir() || !Path::new(&b).is_dir() {
        return Ok(false);
    }
    let root_a = common::git(&["-C", &a, "rev-parse", "--show-toplevel"]);
    let root_b = common::git(&["-C", &b, "rev-parse", "--show-toplevel"]);
    if root_a.is_empty() || root_b.is_empty() {
        return Ok(false);
    }
    Ok(realpath(&py_path(&root_a)).map_err(caught)? == realpath(&py_path(&root_b)).map_err(caught)?)
}

/// What the window shows of one check.
struct Label {
    chat: Py,
    reference: String,
    status: String,
}

enum Found {
    Status(String),
    Active(Py, String),
}

fn lookup(ticket: &Map<String, Value>, app: &'static str, environment: &Env) -> Result<Found, Fault> {
    let status = |text: &str| Ok(Found::Status(text.into()));
    let pinned = Py::of(ticket.get("chatRef"));
    let pinned = pinned.truthy().then_some(pinned);
    let ids: Vec<String> = match &pinned {
        Some(reference) => {
            let (named, id) = parse(reference)?;
            if named != app {
                return status("foreign");
            }
            vec![id]
        }
        None => {
            let Some(Value::Array(list)) = ticket.get("chatIds") else { return status("unidentified") };
            if list.is_empty() {
                return status("unidentified");
            }
            if list.iter().any(|id| id.is_array() || id.is_object()) {
                return Err(caught("unhashable type"));
            }
            let mut ids: Vec<&Value> = Vec::new();
            for id in list {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            ids.into_iter().map(|id| component(&Py::Json(id.clone()))).collect::<Result<_, _>>()?
        }
    };
    let mut found = Vec::new();
    for id in &ids {
        found.push(info(app, id, environment).map_err(Fault::Crash)?);
    }
    if let Some(row) = found.iter().find(|row| row.status != "active") {
        return status(&row.status.clone());
    }
    if found.iter().any(|row| row.id != found[0].id) {
        return status("ambiguous");
    }
    let row = found.swap_remove(0);
    if pinned.is_some() && row.id != ids[0] {
        return status("stale");
    }
    let folder = Py::of(ticket.get("cwd"));
    let folder = if folder.truthy() { folder } else { Py::of(ticket.get("project")) };
    if !same_checkout(&folder, &row.project)? {
        return status("foreign");
    }
    Ok(Found::Active(row.display, row.link))
}

/// `checknames.presentation`: the label of one check; Err is a Python exception nothing catches.
fn label(ticket: &Map<String, Value>, environment: &Env) -> Result<Label, String> {
    let chat = Py::of(ticket.get("chat")).or_empty();
    let fallback = |status: &str| Label { chat: chat.clone(), reference: String::new(), status: status.into() };
    let app = match ticket.get("agent") {
        Some(Value::String(agent)) => APPS.iter().copied().find(|name| name == agent),
        _ => None,
    };
    let Some(app) = app else { return Ok(fallback("unidentified")) };
    match lookup(ticket, app, environment) {
        Ok(Found::Status(status)) => Ok(fallback(&status)),
        Ok(Found::Active(display, link)) => {
            Ok(Label { chat: if display.truthy() { display } else { chat.clone() }, reference: link, status: "active".into() })
        }
        Err(Fault::Caught(_)) => Ok(fallback("unavailable")),
        Err(Fault::Crash(message)) => Err(message),
    }
}

/// JSON as `json.dumps(value, ensure_ascii=False)` writes it, floats as Python writes them; None for bytes, which
/// Python cannot write.
fn dumps(value: &Py) -> Option<String> {
    fn json(value: &Value) -> String {
        match value {
            Value::Object(map) => {
                let inner: Vec<String> = map.iter().map(|(key, value)| format!("{}: {}", Value::String(key.clone()), json(value))).collect();
                format!("{{{}}}", inner.join(", "))
            }
            Value::Array(items) => format!("[{}]", items.iter().map(json).collect::<Vec<_>>().join(", ")),
            Value::Number(number) if number.is_f64() => py_float(number.as_f64().unwrap_or(0.0)),
            other => other.to_string(),
        }
    }
    match value {
        Py::Json(value) => Some(json(value)),
        Py::Bytes(_) => None,
    }
}

/// The process's environment, as `dict(os.environ)` gives it (a variable that is no UTF-8 is left out).
fn environment() -> Env {
    std::env::vars_os().filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?))).collect()
}

/// The label of one check, for what queues a check: `{"chat", "reference", "status"}`. Err is a Python exception
/// nothing catches (a broken record of an app, or a name that is bytes and cannot be written as JSON).
#[allow(dead_code)] // for what queues a check (check.submit)
pub fn presentation(ticket: &Value, environment: &Env) -> Result<Value, String> {
    let Value::Object(ticket) = ticket else { return Err("AttributeError: object has no attribute 'get'".into()) };
    let label = label(ticket, environment)?;
    let Py::Json(chat) = label.chat else { return Err("TypeError: Object of type bytes is not JSON serializable".into()) };
    let mut out = Map::new();
    out.insert("chat".into(), chat);
    out.insert("reference".into(), Value::String(label.reference));
    out.insert("status".into(), Value::String(label.status));
    Ok(Value::Object(out))
}

/// `check --presentation` (checknames.main): the checks on the input, one label each on the output; 64 for input
/// that is no bounded list of checks.
pub fn main() -> i32 {
    let mut data = Vec::new();
    if std::io::stdin().lock().take(LIMIT as u64 + 1).read_to_end(&mut data).is_err() || data.len() > LIMIT {
        return common::EXIT_REFUSED;
    }
    let Some(Value::Array(tickets)) = json_text(&data).and_then(|text| serde_json::from_str::<Value>(&text).ok()) else {
        return common::EXIT_REFUSED;
    };
    if tickets.len() > 256 || tickets.iter().any(|ticket| !ticket.is_object()) {
        return common::EXIT_REFUSED;
    }
    let environment = environment();
    let mut written = Vec::new();
    for ticket in tickets.iter().filter_map(Value::as_object) {
        let label = match label(ticket, &environment) {
            Ok(label) => label,
            Err(message) => {
                common::say(&format!("{} check --presentation: {message}", common::tool()));
                return 1;
            }
        };
        let Some(chat) = dumps(&label.chat) else { return common::EXIT_REFUSED };
        written.push(format!("{{\"chat\": {chat}, \"reference\": {}, \"status\": {}}}", Value::String(label.reference), Value::String(label.status)));
    }
    let mut out = std::io::stdout().lock();
    if writeln!(out, "[{}]", written.join(", ")).and_then(|_| out.flush()).is_err() {
        return common::EXIT_REFUSED;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Fixture {
        root: PathBuf,
        folder: PathBuf,
        environment: Env,
    }

    impl Fixture {
        fn new(name: &str) -> Fixture {
            let root = std::env::temp_dir().join(format!("hitl-names-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            let root = PathBuf::from(realpath(&root.to_string_lossy()).unwrap());
            let folder = root.join("project");
            std::fs::create_dir_all(&folder).unwrap();
            let mut environment = Env::new();
            for (key, value) in [("CONVEYOR_HOME", "home"), ("CONVEYOR_CLAUDE_SESSIONS_DIR", "desktop"),
                                 ("CONVEYOR_CLAUDE_PROJECTS_DIR", "projects"), ("CONVEYOR_CODEX_STATE", "codex.sqlite"),
                                 ("CONVEYOR_OPENCODE_STATE", "oc.sqlite")] {
                environment.insert(key.into(), root.join(value).display().to_string());
            }
            let fixture = Fixture { root, folder, environment };
            fixture.record("owner", &json!({"sessionId": "owner-chat", "cliSessionId": "current-alias", "title": "native",
                                            "cwd": fixture.folder.display().to_string()}));
            fixture.name("Claude", "owner-chat", json!({"mode": "manual", "title": "roadmap", "revision": 1}));
            fixture
        }

        fn record(&self, file: &str, value: &Value) {
            let path = self.root.join("desktop/a/b").join(format!("{file}.json"));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, value.to_string()).unwrap();
        }

        fn name(&self, app: &str, id: &str, change: Value) {
            let mut value = json!({"version": 1, "app": app, "id": id, "mode": "auto", "revision": 0, "generation": 0,
                                   "title": "", "decisions": {}});
            for (key, field) in change.as_object().unwrap() {
                value[key] = field.clone();
            }
            let file = names_file(app, id, &self.environment);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, value.to_string()).unwrap();
        }

        fn ticket(&self, change: Value) -> Value {
            let mut ticket = json!({"id": "fixture-check", "agent": "Claude", "chat": "old label", "chatIds": ["owner-chat"],
                                    "cwd": self.folder.display().to_string(), "project": self.folder.display().to_string()});
            for (key, field) in change.as_object().unwrap() {
                ticket[key] = field.clone();
            }
            ticket
        }

        fn label(&self, change: Value) -> Value {
            presentation(&self.ticket(change), &self.environment).unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn links_are_read_and_written_as_chatrefs_does() {
        for app in APPS {
            for id in ["local_1/2", "a%3F", "a?b#c", "x y", "Имя", "a';$(touch foo)", "MiXeD"] {
                let link = uri(app, id);
                assert_eq!(parse(&Py::text(&link)).unwrap(), (app, id.to_string()));
            }
        }
        assert_eq!(parse(&Py::text("CONVEYOR://CHAT/cOdEx/a%252F")).unwrap(), ("Codex", "a%2F".to_string()));
        assert_eq!(uri("Claude", "local_1/2"), "conveyor://chat/Claude/local_1%2F2");
        for hostile in ["", " conveyor://chat/Codex/a", "conveyor://chat/Codex/a?", "conveyor://chat/Codex/a#",
                        "conveyor://x@chat/Codex/a", "conveyor://chat:3/Codex/a", "conveyor://chat/Codex/a/b",
                        "conveyor://chat/Codex/", "conveyor://chat/ChatGPT/a", "conveyor://chat/Codex/%",
                        "conveyor://chat/Codex/%00", "conveyor://chat/Codex/%FF", "conveyor://chat/Codex/%2e%2e",
                        "conveyor://chat/Codex/a\nb", "conveyor://chat/Codex/%E2%80%AE", "conveyor://chat//a"] {
            assert!(parse(&Py::text(hostile)).is_err(), "{hostile}");
        }
    }

    #[test]
    fn the_chat_button_opens_what_the_check_carries_and_names_no_program() {
        let carried = "my-chat://Some/id-1";
        assert_eq!(open_link("conveyor://chat/Codex/019a8f2e-7c41", carried), carried); // the dead scheme is never opened
        assert_eq!(open_link("conveyor://chat/Claude/owner-chat", ""), "");
        assert_eq!(open_link("conveyor://chat/Claude/owner-chat", "conveyor://chat/Claude/x"), "");
        assert_eq!(open_link("", carried), carried);
        assert_eq!(open_link("", ""), "");
        assert_eq!(open_link("obsidian://open?x=1", carried), "obsidian://open?x=1");
    }

    #[test]
    fn a_helper_of_codex_is_named_by_its_task() {
        let source = Py::text(r#"{"subagent": {"thread_spawn": {"parent_thread_id": "abc", "agent_path": "/x/fix_the_bar"}}}"#);
        assert_eq!(heading(&Py::text(""), &Py::text(""), &source, &Py::text("Bolt")).unwrap(), "fix the bar · Bolt");
        assert_eq!(heading(&Py::text("  Name\nsecond"), &Py::text("t"), &source, &Py::text("")).unwrap(), "Name");
        assert_eq!(heading(&Py::text(""), &Py::text(" a \u{1c} b "), &Py::text(""), &Py::text("")).unwrap(), "a");
        assert_eq!(heading(&Py::text(""), &Py::text(" a \u{1f} b\u{3000}c "), &Py::text(""), &Py::text("")).unwrap(), "a b c");
        assert!(matches!(heading(&Py::Json(json!(5)), &Py::text(""), &Py::text(""), &Py::text("")), Err(Fault::Crash(_))));
        assert!(matches!(heading(&Py::text(""), &Py::text(""), &source, &Py::Json(json!(5))), Err(Fault::Caught(_))));
    }

    #[test]
    fn the_shared_name_and_the_exact_link() {
        let fixture = Fixture::new("shared");
        assert_eq!(fixture.label(json!({})), json!({"chat": "roadmap", "reference": "conveyor://chat/Claude/owner-chat", "status": "active"}));
        assert_eq!(fixture.label(json!({"chatIds": ["owner-chat", "current-alias"]}))["chat"], "roadmap");
    }

    #[test]
    fn missing_stale_foreign_and_ambiguous_are_not_guessed() {
        let fixture = Fixture::new("guess");
        let foreign = fixture.root.join("foreign").display().to_string();
        let cases = [(json!({"chatIds": []}), "unidentified"), (json!({"chatIds": ["missing"]}), "not_found"),
                     (json!({"chatIds": ["owner-chat", "missing"]}), "not_found"), (json!({"cwd": foreign}), "foreign"),
                     (json!({"cwd": "."}), "foreign"), (json!({"agent": "Codex"}), "backend_unavailable"),
                     (json!({"chatRef": "conveyor://chat/Claude/current-alias"}), "stale"), (json!({"chatIds": [5]}), "unavailable"),
                     (json!({"agent": "ChatGPT"}), "unidentified"), (json!({"chatRef": "conveyor://chat/Codex/x"}), "foreign")];
        for (change, status) in cases {
            let label = fixture.label(change.clone());
            assert_eq!((label["status"].as_str().unwrap(), label["chat"].as_str().unwrap(), label["reference"].as_str().unwrap()),
                       (status, "old label", ""), "{change}");
        }
        fixture.record("owner", &json!({"sessionId": "owner-chat", "cliSessionId": "current-alias", "title": "native",
                                         "cwd": fixture.folder.display().to_string(), "priorCliSessionIds": ["old-alias"]}));
        assert_eq!(fixture.label(json!({"chatIds": ["old-alias"]}))["status"], "not_found");
        fixture.record("other", &json!({"sessionId": "other-chat", "cliSessionId": "current-alias"}));
        assert_eq!(fixture.label(json!({"chatIds": ["current-alias"]}))["status"], "ambiguous");
        assert_eq!(fixture.label(json!({"chatIds": ["owner-chat", "other-chat"]}))["status"], "ambiguous");
        fixture.record("owner", &json!({"sessionId": "owner-chat", "title": "native", "isArchived": true,
                                         "cwd": fixture.folder.display().to_string()}));
        assert_eq!(fixture.label(json!({"chatRef": "conveyor://chat/Claude/owner-chat"}))["status"], "archived");
    }

    #[test]
    fn codex_and_opencode_keep_their_own_names() {
        let fixture = Fixture::new("apps");
        let folder = fixture.folder.display().to_string();
        let codex = rusqlite::Connection::open(fixture.root.join("codex.sqlite")).unwrap();
        codex.execute_batch("create table threads (id text,title text,name text,cwd text,source text,archived integer)").unwrap();
        codex.execute("insert into threads values (?,?,?,?,?,?)", ("same-chat", "Native", "", &folder, "cli", 0)).unwrap();
        let oc = rusqlite::Connection::open(fixture.root.join("oc.sqlite")).unwrap();
        oc.execute_batch("create table session_v2 (id text,title text,directory text,parent_id text,time_archived integer,metadata text)").unwrap();
        oc.execute("insert into session_v2 values (?,?,?,?,?,?)", ("same-chat", "Native OC", &folder, "", None::<i64>, "{}")).unwrap();
        drop((codex, oc));
        assert_eq!(fixture.label(json!({"agent": "Codex", "chatIds": ["same-chat"]}))["chat"], "Native");
        assert_eq!(fixture.label(json!({"agent": "OpenCode", "chatIds": ["same-chat"]}))["chat"], "Native OC");
        for app in ["Codex", "OpenCode"] {
            fixture.name(app, "same-chat", json!({"mode": "manual", "title": format!("{app} label")}));
            let label = fixture.label(json!({"agent": app, "chatIds": ["same-chat"]}));
            assert_eq!((label["chat"].as_str().unwrap(), label["reference"].as_str().unwrap()),
                       (format!("{app} label").as_str(), uri(app, "same-chat").as_str()));
        }
    }

    #[test]
    fn a_chat_of_the_command_alone_is_read_from_its_transcript() {
        let fixture = Fixture::new("cli");
        std::fs::remove_file(fixture.root.join("desktop/a/b/owner.json")).unwrap();
        let transcript = fixture.root.join("projects/private/owner-chat.jsonl");
        std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
        let folder = fixture.folder.display().to_string();
        std::fs::write(&transcript, format!("{}\r\n{}\n", json!({"sessionId": "owner-chat", "cwd": folder}),
                                            json!({"type": "custom-title", "customTitle": "timeline"}))).unwrap();
        assert_eq!(fixture.label(json!({}))["chat"], "roadmap");
        fixture.name("Claude", "owner-chat", json!({"mode": "native"}));
        assert_eq!(fixture.label(json!({}))["chat"], "timeline");
        std::fs::write(&transcript, "[1]\n").unwrap();
        assert!(presentation(&fixture.ticket(json!({})), &fixture.environment).is_err());
    }

    #[test]
    fn corrupt_metadata_keeps_what_the_ticket_said() {
        let fixture = Fixture::new("corrupt");
        std::fs::write(names_file("Claude", "owner-chat", &fixture.environment), "bad json").unwrap();
        let label = fixture.label(json!({}));
        assert_eq!((label["status"].as_str().unwrap(), label["chat"].as_str().unwrap()), ("backend_unavailable", "old label"));
        std::fs::write(fixture.root.join("desktop/a/b/owner.json"), "bad json").unwrap();
        assert_eq!(fixture.label(json!({}))["reference"], "");
    }

    #[test]
    fn a_subfolder_counts_only_inside_the_same_git_checkout() {
        let fixture = Fixture::new("git");
        let folder = fixture.folder.display().to_string();
        let sub = fixture.folder.join("apps/web");
        std::fs::create_dir_all(&sub).unwrap();
        fixture.record("owner", &json!({"sessionId": "owner-chat", "title": "native", "cwd": sub.display().to_string()}));
        assert_eq!(fixture.label(json!({}))["status"], "foreign");
        let git = |arguments: &[&str]| {
            assert!(std::process::Command::new("git").args(arguments).output().unwrap().status.success(), "{arguments:?}");
        };
        git(&["init", "-q", &folder]);
        git(&["-C", &folder, "-c", "user.name=Fixture", "-c", "user.email=fixture@invalid", "commit", "-q", "--allow-empty", "-m", "fixture"]);
        assert_eq!(fixture.label(json!({}))["chat"], "roadmap");
        let other = fixture.root.join("other-checkout").display().to_string();
        git(&["-C", &folder, "worktree", "add", "-q", "--detach", &other]);
        fixture.record("owner", &json!({"sessionId": "owner-chat", "title": "native", "cwd": other}));
        assert_eq!(fixture.label(json!({}))["status"], "foreign");
    }
}
