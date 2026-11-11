//! What the messenger needs of the chats of OpenCode: the `opencode` command, what OpenCode keeps of one chat, and a
//! message into a chat (`session.prompt` with `delivery: steer`: into the running turn, or a new turn of an idle chat).
//! The port of the parts of `src/hitl/opencodeapp.py` that `tell.py` uses. OWNER: part C.
//!
//! A request goes straight to OpenCode's background service over HTTP when its local address is known, else through
//! the command (`opencode api …`).
#![allow(dead_code)]

use crate::common::{self, Env, read_json, truthy};
use crate::tell::{self, Done, Sent, Why};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::io::{Read, Seek, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// A chat whose last word is older than this is not taken for one at work, when the service cannot be asked.
const STALE_TURN: f64 = 30.0 * 60.0;
const PROGRAMS: [&str; 2] = ["/opt/homebrew/bin/opencode", "/usr/local/bin/opencode"];
const MARK: &str = "conveyorArchivedAt";

/// The `opencode` command: the one on PATH, else the one OpenCode's desktop app keeps beside itself.
pub fn program(environment: &Env) -> String {
    if let Some(named) = environment.get("CONVEYOR_OPENCODE") {
        return if !named.is_empty() && Path::new(named).is_file() { named.clone() } else { String::new() };
    }
    let mut candidates: Vec<String> = tell::which_in("opencode", environment.get("PATH")).into_iter().collect();
    candidates.extend(PROGRAMS.iter().map(|program| program.to_string()));
    let desktop = tell::home_dir().join("Library/Application Support/ai.opencode.desktop/cli");
    let mut found: Vec<String> = crate::claude::globbed(&desktop).into_iter().map(|version| version.join("opencode-cli"))
        .filter(|path| path.exists()).map(|path| path.display().to_string()).collect();
    found.sort();
    found.reverse();
    candidates.extend(found);
    candidates.into_iter().find(|found| !found.is_empty() && Path::new(found).is_file()).unwrap_or_default()
}

/// Where OpenCode's background service writes its local address; None when a made-up command or a server named by
/// hand is used.
fn service_file(environment: &Env) -> Option<PathBuf> {
    if let Some(named) = environment.get("CONVEYOR_OPENCODE_SERVICE") {
        return (!named.is_empty()).then(|| PathBuf::from(named));
    }
    if ["CONVEYOR_OPENCODE", "CONVEYOR_FAKE_OPENCODE", "CONVEYOR_OPENCODE_SERVER"].iter().any(|key| environment.contains_key(*key)) {
        return None;
    }
    let state = environment.get("XDG_STATE_HOME").filter(|value| !value.is_empty()).map(PathBuf::from)
        .unwrap_or_else(|| tell::home_dir().join(".local/state"));
    Some(state.join("opencode").join("service.json"))
}

#[derive(Clone, Debug)]
struct Service {
    url: String,
    headers: Vec<(String, String)>,
}

fn service_headers(info: &Value) -> Vec<(String, String)> {
    match info.get("password").and_then(Value::as_str) {
        Some(password) if !password.is_empty() => {
            vec![("Authorization".into(), format!("Basic {}", crate::codex::base64(format!("opencode:{password}").as_bytes())))]
        }
        _ => Vec::new(),
    }
}

fn probe_service(info: &Value, timeout: f64) -> bool {
    let url = tell::field_or_empty(info, "url").trim_end_matches('/').to_string();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return false;
    }
    match http(&format!("{url}/api/info"), "GET", &service_headers(info), None, timeout) {
        Ok((200, body)) => {
            let body = serde_json::from_slice::<Value>(&body).ok().filter(Value::is_object);
            body.is_some_and(|body| !truthy(info.get("pid")) || body.get("pid") == info.get("pid"))
        }
        _ => false,
    }
}

/// The running local OpenCode background service, started in the background when `start` is true.
fn service_info(environment: &Env, start: bool) -> Option<Service> {
    let path = service_file(environment)?;
    let found = |timeout: f64| -> Option<Service> {
        let info = read_json(&path).filter(Value::is_object)?;
        probe_service(&info, timeout).then(|| Service {
            url: tell::field(&info, "url").trim_end_matches('/').to_string(),
            headers: service_headers(&info),
        })
    };
    if let Some(service) = found(1.5) {
        return Some(service);
    }
    if !start {
        return None;
    }
    let _ = run(environment, &["service".into(), "start".into()], 30.0);
    found(3.0)
}

/// The method, path, query and body of an operation of OpenCode's API over HTTP, for the ones that have one.
fn http_route(operation: &str, params: &[(String, String)], data: Option<&Value>) -> Option<(&'static str, String, Vec<(String, String)>, Option<Value>)> {
    let session: String = params.iter().find(|(key, _)| key == "sessionID").map(|(_, value)| quote(value)).unwrap_or_default();
    let rest: Vec<(String, String)> = params.iter().filter(|(key, _)| key != "sessionID").cloned().collect();
    let body = || Some(data.cloned().filter(|data| truthy(Some(data))).unwrap_or_else(|| json!({})));
    match operation {
        "session.active" => return Some(("GET", "/api/session/active".into(), Vec::new(), None)),
        "model.list" => return Some(("GET", "/api/model".into(), rest, None)),
        "session.create" => return Some(("POST", "/api/session".into(), Vec::new(), body())),
        _ => {}
    }
    if session.is_empty() {
        return None;
    }
    match operation {
        "session.get" => Some(("GET", format!("/api/session/{session}"), Vec::new(), None)),
        "session.update" => Some(("PATCH", format!("/api/session/{session}"), Vec::new(), body())),
        "session.remove" => Some(("DELETE", format!("/api/session/{session}"), Vec::new(), None)),
        "session.prompt" => Some(("POST", format!("/api/session/{session}/prompt"), Vec::new(), body())),
        "session.message.list" => Some(("GET", format!("/api/session/{session}/message"), rest, None)),
        _ => None,
    }
}

/// `urllib.parse.quote(text, safe="")`.
fn quote(text: &str) -> String {
    text.bytes().map(|byte| {
        if byte.is_ascii_alphanumeric() || b"_.-~".contains(&byte) { (byte as char).to_string() } else { format!("%{byte:02X}") }
    }).collect()
}

/// `urllib.parse.quote_plus(text)`, for a query.
fn quote_plus(text: &str) -> String {
    text.bytes().map(|byte| match byte {
        b' ' => "+".to_string(),
        byte if byte.is_ascii_alphanumeric() || b"_.-~".contains(&byte) => (byte as char).to_string(),
        byte => format!("%{byte:02X}"),
    }).collect()
}

/// One HTTP request, as urllib makes it, with the whole answer read: the status and the body. Only `http://`: the
/// service is on this Mac.
fn http(url: &str, method: &str, headers: &[(String, String)], body: Option<&[u8]>, timeout: f64) -> Result<(u16, Vec<u8>), String> {
    let Some(rest) = url.strip_prefix("http://") else {
        return Err(format!("<urlopen error unknown url type: {}>", url.split(':').next().unwrap_or("")));
    };
    let (host, path) = match rest.find('/') {
        Some(slash) => (&rest[..slash], &rest[slash..]),
        None => (rest, "/"),
    };
    let address = if host.contains(':') { host.to_string() } else { format!("{host}:80") };
    let timeout = Duration::from_secs_f64(timeout.max(0.001));
    let refused = |error: std::io::Error| format!("<urlopen error {}>", tell::os_error(&error));
    let addresses: Vec<_> = address.to_socket_addrs().map_err(|error| format!("<urlopen error {error}>"))?.collect();
    let mut connection = None;
    let mut last = None;
    for candidate in addresses {
        match TcpStream::connect_timeout(&candidate, timeout) {
            Ok(stream) => {
                connection = Some(stream);
                break;
            }
            Err(error) => last = Some(error),
        }
    }
    let mut connection = match connection {
        Some(connection) => connection,
        None => return Err(refused(last.unwrap_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound)))),
    };
    let _ = connection.set_read_timeout(Some(timeout));
    let _ = connection.set_write_timeout(Some(timeout));
    let mut request = format!("{method} {path} HTTP/1.1\r\nAccept-Encoding: identity\r\nHost: {host}\r\nUser-Agent: hitl\r\nConnection: close\r\n");
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    if let Some(body) = body {
        request.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    request.push_str("\r\n");
    let mut data = request.into_bytes();
    data.extend_from_slice(body.unwrap_or(&[]));
    connection.write_all(&data).map_err(|error| tell::os_error(&error))?;
    let mut answer = Vec::new();
    connection.read_to_end(&mut answer).map_err(|error| tell::os_error(&error))?;
    let split = crate::claude::find_bytes(&answer, b"\r\n\r\n").ok_or_else(|| "Remote end closed connection without response".to_string())?;
    let head = String::from_utf8_lossy(&answer[..split]).to_string();
    let mut body = answer[split + 4..].to_vec();
    let status = head.split_whitespace().nth(1).and_then(|code| code.parse::<u16>().ok()).ok_or_else(|| format!("bad status line: {head}"))?;
    let chunked = head.lines().any(|line| {
        let lower = line.to_ascii_lowercase();
        lower.starts_with("transfer-encoding:") && lower.contains("chunked")
    });
    if chunked {
        let mut whole = Vec::new();
        let mut at = 0;
        while let Some(end) = crate::claude::find_bytes(&body[at..], b"\r\n").map(|end| at + end) {
            let size = usize::from_str_radix(String::from_utf8_lossy(&body[at..end]).split(';').next().unwrap_or("").trim(), 16).unwrap_or(0);
            if size == 0 || end + 2 + size > body.len() {
                break;
            }
            whole.extend_from_slice(&body[end + 2..end + 2 + size]);
            at = end + 2 + size + 2;
        }
        body = whole;
    }
    Ok((status, body))
}

/// One call to the service over HTTP: its answer, or why not (Python's `Failure`).
fn http_call(service: &Service, method: &str, path: &str, query: &[(String, String)], data: Option<&Value>, wait: f64) -> Result<Value, String> {
    let mut url = format!("{}{path}", service.url);
    if !query.is_empty() {
        url.push('?');
        url.push_str(&query.iter().map(|(key, value)| format!("{}={}", quote_plus(key), quote_plus(value))).collect::<Vec<_>>().join("&"));
    }
    let mut headers = service.headers.clone();
    let body = data.map(|data| common::dumps_line(data).into_bytes());
    if body.is_some() {
        headers.push(("Content-Type".into(), "application/json".into()));
    }
    match http(&url, method, &headers, body.as_deref(), wait) {
        Ok((status, raw)) if status < 400 => {
            let raw = String::from_utf8_lossy(&raw).to_string();
            Ok(if raw.trim().is_empty() { json!({}) } else { parsed(&raw).unwrap_or(Value::Null) })
        }
        Ok((status, raw)) => {
            let raw = String::from_utf8_lossy(&raw).to_string();
            let said = parsed(&raw).filter(Value::is_object).and_then(|answer| answer.get("message").filter(|said| truthy(Some(said))).map(tell::python_str));
            let said = said.unwrap_or_else(|| if raw.is_empty() { format!("HTTP {status}") } else { raw });
            Err(common::squeeze(&said).chars().take(400).collect())
        }
        Err(error) => Err(format!("opencode не ответил: {error}")),
    }
}

/// Python's way of writing a list of words: `['a', 'b']`.
fn words_repr(words: &[String]) -> String {
    format!("[{}]", words.iter().map(|word| tell::repr_text(word)).collect::<Vec<_>>().join(", "))
}

/// A number of seconds as Python writes the one it was given: `60`, `0.5`.
fn number(seconds: f64) -> String {
    if seconds.fract() == 0.0 { format!("{}", seconds as i64) } else { format!("{seconds}") }
}

/// The command, run to its end with its output in files: the command leaves before a pipe has taken all it wrote, and
/// a long answer came cut at 256 or 384 KB (opencode 2.0.25).
fn run(environment: &Env, arguments: &[String], wait: f64) -> Result<Done, String> {
    let command = program(environment);
    if command.is_empty() {
        return Err("на этом Маке нет opencode".into());
    }
    let mut words: Vec<String> = std::iter::once(command.clone()).chain(arguments.iter().cloned()).collect();
    if let Some(server) = environment.get("CONVEYOR_OPENCODE_SERVER").filter(|server| !server.is_empty()) {
        words.extend(["--server".to_string(), server.clone()]);
    }
    let made = |kind: &str| -> std::io::Result<std::fs::File> {
        let path = std::env::temp_dir().join(format!("hitl-opencode-{}-{kind}-{}", std::process::id(), tell::uuid4()));
        let file = std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(&path)?;
        let _ = std::fs::remove_file(&path);
        Ok(file)
    };
    let (mut out, mut err) = match (made("out"), made("err")) {
        (Ok(out), Ok(err)) => (out, err),
        (Err(error), _) | (_, Err(error)) => return Err(format!("opencode не ответил: {}", tell::os_error(&error))),
    };
    let mut child = {
        let (Ok(stdout), Ok(stderr)) = (out.try_clone(), err.try_clone()) else { return Err("opencode не ответил".into()) };
        let mut process = Command::new(&command);
        process.args(&words[1..]).stdin(Stdio::null()).stdout(Stdio::from(stdout)).stderr(Stdio::from(stderr))
            .env_clear().envs(environment).current_dir(tell::home_dir());
        match process.spawn() {
            Ok(child) => child,
            Err(error) => return Err(format!("opencode не ответил: {}", tell::file_error(&error, &command))),
        }
    };
    let deadline = Instant::now() + Duration::from_secs_f64(wait.max(0.0));
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("opencode не ответил: Command '{}' timed out after {} seconds", words_repr(&words), number(wait)));
            }
        }
    };
    let read = |file: &mut std::fs::File| -> String {
        let mut data = Vec::new();
        let _ = file.seek(std::io::SeekFrom::Start(0)).and_then(|_| file.read_to_end(&mut data));
        String::from_utf8_lossy(&data).to_string()
    };
    Ok(Done { code: status.code().unwrap_or(-1), stdout: read(&mut out), stderr: read(&mut err) })
}

fn parsed(body: &str) -> Option<Value> {
    if body.trim().is_empty() {
        return None;
    }
    serde_json::from_str(body).ok()
}

/// One request to OpenCode's service: directly over HTTP when the local service is discovered, else by `opencode api`.
/// With `alone`, a read request falls back to `--standalone` when the service does not answer.
fn api(environment: &Env, operation: &str, params: &[(String, String)], data: Option<&Value>, wait: f64, alone: bool) -> Result<Value, String> {
    if let Some((method, path, query, payload)) = http_route(operation, params, data) {
        if let Some(service) = service_info(environment, !alone) {
            return http_call(&service, method, &path, &query, payload.as_ref(), wait);
        }
    }
    let mut arguments = vec!["api".to_string(), operation.to_string()];
    for (key, value) in params {
        arguments.extend(["--param".to_string(), format!("{key}={value}")]);
    }
    if let Some(data) = data {
        arguments.extend(["--data".to_string(), common::dumps_line(data)]);
    }
    let mut done = run(environment, &arguments, wait)?;
    let mut answer = parsed(&done.stdout);
    if done.code != 0 && answer.is_none() && alone && !environment.get("CONVEYOR_OPENCODE_SERVER").is_some_and(|server| !server.is_empty()) {
        arguments.push("--standalone".into());
        done = run(environment, &arguments, wait + 30.0)?;
        answer = parsed(&done.stdout);
    }
    if done.code != 0 {
        let said = answer.as_ref().filter(|answer| answer.is_object()).and_then(|answer| answer.get("message")).filter(|said| truthy(Some(said)))
            .map(tell::python_str);
        let said = said.or_else(|| Some(done.stdout.clone()).filter(|text| !text.is_empty()))
            .or_else(|| Some(done.stderr.clone()).filter(|text| !text.is_empty())).unwrap_or_else(|| format!("exit {}", done.code));
        return Err(common::squeeze(&said).chars().take(400).collect());
    }
    Ok(answer.unwrap_or(Value::Null))
}

/// Whether a chat's metadata, as its database keeps it, carries «Конвейер»'s mark of the archive.
fn archived_mark(metadata: &Value) -> bool {
    let metadata = match metadata {
        Value::String(text) => match serde_json::from_str::<Value>(text) {
            Ok(value) => value,
            Err(_) => return false,
        },
        other => other.clone(),
    };
    metadata.is_object() && truthy(metadata.get(MARK))
}

/// What OpenCode keeps of one chat in its database.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: String,
    pub folder: String,
    pub archived: bool,
    pub working: bool,
    pub parent: String,
}

fn sql_value(value: rusqlite::types::Value) -> Value {
    match value {
        rusqlite::types::Value::Null => Value::Null,
        rusqlite::types::Value::Integer(number) => json!(number),
        rusqlite::types::Value::Real(number) => json!(number),
        rusqlite::types::Value::Text(text) => json!(text),
        rusqlite::types::Value::Blob(bytes) => json!(String::from_utf8_lossy(&bytes)),
    }
}

/// None when the chat is not there; Err with what sqlite said.
pub fn row_of(chat_id: &str, environment: &Env) -> Result<Option<Row>, String> {
    let database = common::opencode_state(environment);
    let session = chat_id.len() > 3 && chat_id.starts_with("ses") && tell::id_like(&chat_id[3..], 1);
    if !session || !database.exists() {
        return Ok(None);
    }
    let connection = common::read_only(&database, 2.0).map_err(|error| tell::sqlite_said(&error))?;
    for table in ["session_v2", "session"] {
        let mut have: HashSet<String> = HashSet::new();
        let mut statement = connection.prepare(&format!("pragma table_info({table})")).map_err(|error| tell::sqlite_said(&error))?;
        let names = statement.query_map([], |row| row.get::<_, String>(1)).map_err(|error| tell::sqlite_said(&error))?;
        for name in names {
            have.insert(name.map_err(|error| tell::sqlite_said(&error))?);
        }
        if have.is_empty() {
            continue;
        }
        let column = |name: &str| if have.contains(name) { name.to_string() } else { "null".to_string() };
        let query = format!("select directory, time_archived, {}, {}, time_updated, {}, parent_id from {table} where id = ?",
                            column("metadata"), column("time_suspended"), column("time_idle"));
        let row = connection.query_row(&query, [chat_id], |row| {
            (0..7).map(|index| row.get::<_, rusqlite::types::Value>(index).map(sql_value)).collect::<Result<Vec<Value>, _>>()
        });
        let row = match row {
            Ok(row) => row,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(error) => return Err(tell::sqlite_said(&error)),
        };
        let (folder, archived, meta, suspended, updated, idle, parent) = (&row[0], &row[1], &row[2], &row[3], &row[4], &row[5], &row[6]);
        let updated_at = updated.as_f64().unwrap_or(0.0);
        let at_work = if have.contains("time_suspended") {
            !suspended.is_null()
        } else {
            truthy(Some(updated)) && (idle.is_null() || idle.as_f64().is_some_and(|idle| updated_at > idle))
        };
        let busy = at_work && truthy(Some(updated)) && common::now() - updated_at / 1000.0 < STALE_TURN;
        let text = |value: &Value| if truthy(Some(value)) { tell::python_str(value) } else { String::new() };
        return Ok(Some(Row { id: chat_id.to_string(), folder: text(folder), archived: !archived.is_null() || archived_mark(meta),
                             working: busy, parent: text(parent) }));
    }
    Ok(None)
}

/// The chats at work now, as the service says; None when it cannot be asked.
fn running(environment: &Env) -> Option<HashSet<String>> {
    let answer = api(environment, "session.active", &[], None, 20.0, false).ok()?;
    let data = answer.get("data")?.as_object()?;
    Some(data.iter().filter(|(_, value)| value.get("type").and_then(Value::as_str) == Some("running")).map(|(key, _)| key.clone()).collect())
}

/// Hands his message to a chat as the OpenCode app does: Ok when the service took it, or why not.
pub fn send(chat_id: &str, text: &str, environment: &Env) -> Sent {
    let text = text.trim();
    if text.is_empty() {
        return Err(Why::new("пустое сообщение"));
    }
    // Python lets an error of OpenCode's database end the messenger here; it is said as why the message did not go.
    let row = row_of(chat_id, environment).map_err(Why::new)?;
    if row.as_ref().is_some_and(|row| row.archived) {
        return Err(Why::new("чат в архиве OpenCode"));
    }
    if row.is_none() || environment.get("CONVEYOR_FAKE_OPENCODE").is_some_and(|value| !value.is_empty()) {
        let _ = running(environment); // what the turn is is asked as Python asks it; the message is the same either way
    }
    let payload = json!({"text": text, "delivery": "steer"});
    match api(environment, "session.prompt", &[("sessionID".into(), chat_id.into())], Some(&payload), 60.0, false) {
        Ok(_) => Ok(()),
        Err(error) => Err(Why::new(format!("OpenCode не принял сообщение: {error}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mark_of_the_archive_is_read_from_text_or_an_object() {
        assert!(archived_mark(&json!("{\"conveyorArchivedAt\": 5}")));
        assert!(archived_mark(&json!({"conveyorArchivedAt": true})));
        assert!(!archived_mark(&json!("{\"own\": 1}")) && !archived_mark(&json!("not json")) && !archived_mark(&Value::Null));
    }

    #[test]
    fn a_session_is_put_into_a_path_quoted() {
        let (method, path, _, body) = http_route("session.prompt", &[("sessionID".into(), "ses_a b".into())], Some(&json!({"text": "x"}))).unwrap();
        assert_eq!((method, path.as_str(), body), ("POST", "/api/session/ses_a%20b/prompt", Some(json!({"text": "x"}))));
        assert!(http_route("session.prompt", &[], None).is_none());
        assert_eq!(http_route("session.active", &[], None).unwrap().1, "/api/session/active");
    }
}
