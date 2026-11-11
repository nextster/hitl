//! What the messenger needs of the chats of Codex: a chat's own record, which says whether a message reached it, and
//! the shared server `hitl limits` and `hitl models --app Codex` ask.
//!
//! The shared server is Codex's app-server daemon, reached through its own command `codex app-server proxy`, which
//! carries bytes to the daemon's socket; the daemon speaks JSON-RPC over WebSocket there.

use crate::common::{self, Env, truthy};
use crate::tell::{self, python_str};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::io::{Read, Seek, Write};
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

/// A turn whose record has said nothing for this long is not taken for one at work.
const WANTED: [&str; 7] = ["message", "function_call", "custom_tool_call", "web_search_call", "local_shell_call", "agent_message", "turn_aborted"];

// --------------------------------------------------------------------------
// The chats

pub fn database(environment: &Env) -> PathBuf {
    common::codex_state(environment)
}

/// What Codex keeps of one chat: its record, folder and whether it is archived.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub thread: String,
    pub rollout: String,
    pub cwd: String,
    pub archived: bool,
}

/// None for a chat Codex does not know; Err with what sqlite said.
pub fn row_of(chat_id: &str, environment: &Env) -> Result<Option<Row>, String> {
    if !tell::id_like(chat_id, 1) || !database(environment).exists() {
        return Ok(None);
    }
    let connection = common::read_only(&database(environment), tell::PATIENCE).map_err(|error| tell::sqlite_said(&error))?;
    let row = connection.query_row("select rollout_path, cwd, archived from threads where id = ?", [chat_id], |row| {
        let text = |index: usize| row.get::<_, Option<String>>(index).ok().flatten().unwrap_or_default();
        let archived = match row.get_ref(2)? {
            rusqlite::types::ValueRef::Null => false,
            rusqlite::types::ValueRef::Integer(number) => number != 0,
            rusqlite::types::ValueRef::Real(number) => number != 0.0,
            rusqlite::types::ValueRef::Text(text) | rusqlite::types::ValueRef::Blob(text) => !text.is_empty(),
        };
        Ok(Row { thread: chat_id.to_string(), rollout: text(0), cwd: text(1), archived })
    });
    match row {
        Ok(row) => Ok(Some(row)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(tell::sqlite_said(&error)),
    }
}

/// The records of the chats written in last, the newest first: where `hitl limits` looks when the daemon is down.
pub fn recent_rollouts(environment: &Env, limit: usize) -> Vec<String> {
    if !database(environment).exists() {
        return Vec::new();
    }
    let Ok(connection) = common::read_only(&database(environment), tell::PATIENCE) else { return Vec::new() };
    let Ok(mut statement) = connection.prepare("select rollout_path from threads order by updated_at desc limit ?") else { return Vec::new() };
    let Ok(rows) = statement.query_map([limit as i64], |row| row.get::<_, Option<String>>(0)) else { return Vec::new() };
    rows.flatten().flatten().collect()
}

/// How long the chat's record is now: where a message sent after this would be written.
pub fn record_size(chat_id: &str, environment: &Env) -> u64 {
    match row_of(chat_id, environment) {
        Ok(Some(row)) if !row.rollout.is_empty() => std::fs::metadata(&row.rollout).map(|metadata| metadata.len()).unwrap_or(0),
        _ => 0,
    }
}

// --------------------------------------------------------------------------
// What he wrote in a chat, as its record keeps it

fn whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

fn skip_space(data: &[u8], mut index: usize) -> usize {
    while index < data.len() && whitespace(data[index]) {
        index += 1;
    }
    index
}

/// `"type":\s*"(response_item|compacted|event_msg)"`, searched: which of the three.
fn line_kind(head: &[u8]) -> Option<&'static str> {
    let key = b"\"type\":";
    let mut from = 0;
    while let Some(found) = claude_find(&head[from..], key) {
        let at = skip_space(head, from + found + key.len());
        from += found + 1;
        if head.get(at) != Some(&b'"') {
            continue;
        }
        for word in ["response_item", "compacted", "event_msg"] {
            let end = at + 1 + word.len();
            if head.len() > end && &head[at + 1..end] == word.as_bytes() && head[end] == b'"' {
                return Some(word);
            }
        }
    }
    None
}

/// `"payload":\s*\{\s*"type":\s*"([a-z_]+)"`, searched.
fn payload_kind(head: &[u8]) -> Option<String> {
    let key = b"\"payload\":";
    let mut from = 0;
    while let Some(found) = claude_find(&head[from..], key) {
        let mut at = skip_space(head, from + found + key.len());
        from += found + 1;
        if head.get(at) != Some(&b'{') {
            continue;
        }
        at = skip_space(head, at + 1);
        if !head[at..].starts_with(b"\"type\":") {
            continue;
        }
        at = skip_space(head, at + 7);
        if head.get(at) != Some(&b'"') {
            continue;
        }
        let word: Vec<u8> = head[at + 1..].iter().take_while(|byte| byte.is_ascii_lowercase() || **byte == b'_').copied().collect();
        if !word.is_empty() && head.get(at + 1 + word.len()) == Some(&b'"') {
            return Some(String::from_utf8_lossy(&word).to_string());
        }
    }
    None
}

fn claude_find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    crate::claude::find_bytes(haystack, needle)
}

fn word_char(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

/// `\s*<([A-Za-z_][\w-]*)[^>]*>.*?</\1>` matched at the start of a text: where the block ends.
fn block_end(text: &str) -> Option<usize> {
    let start = text.len() - text.trim_start().len();
    let rest = text[start..].strip_prefix('<')?;
    let name_start = start + 1;
    let first = rest.chars().next()?;
    if !(first.is_ascii_alphabetic() || first == '_') {
        return None;
    }
    let name: String = std::iter::once(first).chain(rest[first.len_utf8()..].chars().take_while(|c| word_char(*c) || *c == '-')).collect();
    let closing = text[name_start..].find('>')? + name_start;
    if closing < name_start + name.len() {
        return None;
    }
    let body = closing + 1;
    let characters: Vec<char> = name.chars().collect();
    for count in (1..=characters.len()).rev() {
        let shorter: String = characters[..count].iter().collect();
        let tag = format!("</{shorter}>");
        if let Some(found) = text[body..].find(&tag) {
            return Some(body + found + tag.len());
        }
    }
    None
}

/// `\s*<[A-Za-z_][\w-]*[\s>]` matched at the start of a text.
fn opens_block(text: &str) -> bool {
    let Some(rest) = text.trim_start().strip_prefix('<') else { return false };
    let mut characters = rest.chars();
    if !characters.next().is_some_and(|first| first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    characters.find(|c| !(word_char(*c) || *c == '-')).is_some_and(|after| after.is_whitespace() || after == '>')
}

/// What of one part of a user message he wrote: "" for a part that is all context.
pub fn his_words(text: &str) -> String {
    if text.trim_start().starts_with("# AGENTS.md instructions") {
        return String::new();
    }
    let mut text = text;
    while let Some(end) = block_end(text) {
        text = &text[end..];
    }
    if opens_block(text) {
        return String::new();
    }
    let trimmed = text.trim_start();
    let text = match trimmed.strip_prefix("## My request for Codex:") {
        Some(after) => after,
        None => text,
    };
    text.trim().to_string()
}

fn texts(content: Option<&Value>, kinds: &[&str]) -> Vec<String> {
    let mut found = Vec::new();
    for part in content.and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]) {
        if !part.is_object() {
            continue;
        }
        let kind = part.get("type").and_then(Value::as_str).unwrap_or("");
        if kinds.contains(&kind) && part.get("text").is_some_and(Value::is_string) {
            found.push(part["text"].as_str().unwrap_or("").to_string());
        } else if kind == "input_image" || kind == "image" {
            found.push("[картинка]".into());
        }
    }
    found
}

/// His messages in a record from a byte offset on: the user's words of each line that has them.
fn his_messages(path: &str, start: u64) -> std::io::Result<Vec<String>> {
    let mut file = std::fs::File::open(path)?;
    let size = file.metadata()?.len();
    let start = if start > size { 0 } else { start }; // a record shorter than the cursor is another one
    file.seek(std::io::SeekFrom::Start(start))?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;
    let mut found = Vec::new();
    for raw in data.split_inclusive(|byte| *byte == b'\n') {
        if !raw.ends_with(b"\n") {
            break; // a line Codex is still writing
        }
        let head = &raw[..raw.len().min(300)];
        let Some(kind) = line_kind(head) else { continue };
        if kind != "compacted" && !payload_kind(head).is_some_and(|payload| WANTED.contains(&payload.as_str())) {
            continue;
        }
        let Ok(item) = serde_json::from_slice::<Value>(raw) else { continue };
        if item.get("type").and_then(Value::as_str) != Some("response_item") {
            continue;
        }
        let payload = item.get("payload").filter(|payload| payload.is_object());
        let Some(payload) = payload else { continue };
        if payload.get("type").and_then(Value::as_str) != Some("message") || payload.get("role").and_then(Value::as_str) != Some("user") {
            continue;
        }
        let words: Vec<String> = texts(payload.get("content"), &["input_text", "text"]).iter().map(|text| his_words(text)).filter(|word| !word.is_empty()).collect();
        let text = words.join("\n\n");
        if !text.is_empty() {
            found.push(text);
        }
    }
    Ok(found)
}

/// Whether a message the chat was sent has come into its own record after `start`, within `wait` seconds: the receipt
/// a send that ended without an answer is checked by.
pub fn received(chat_id: &str, needle: &str, start: u64, environment: &Env, wait: f64) -> bool {
    let Ok(Some(row)) = row_of(chat_id, environment) else { return false };
    if row.rollout.is_empty() || needle.is_empty() {
        return false;
    }
    let deadline = Instant::now() + Duration::from_secs_f64(wait.max(0.0));
    loop {
        if let Ok(messages) = his_messages(&row.rollout, start) {
            if messages.iter().any(|message| message.contains(needle)) {
                return true;
            }
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

// --------------------------------------------------------------------------
// The shared daemon of Codex

/// What went wrong with the daemon: its own refusal or a connection that broke (Python's DaemonError), no answer in
/// time (Late), or a request that went out whole and had no answer, which may have been done (Unanswered).
#[derive(Clone, Debug, PartialEq)]
pub enum DaemonError {
    Refused(String),
    Late(String),
    Unanswered(String),
}

impl DaemonError {
    pub fn text(&self) -> &str {
        match self {
            DaemonError::Refused(text) | DaemonError::Late(text) | DaemonError::Unanswered(text) => text,
        }
    }
}

impl std::fmt::Display for DaemonError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.text())
    }
}

/// `" ".join(str(text).split())`, cut to `limit` with «…».
fn one_line(text: &str, limit: usize) -> String {
    let text = common::squeeze(text);
    if text.chars().count() <= limit { text } else { format!("{}…", text.chars().take(limit - 1).collect::<String>()) }
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub(crate) fn base64(data: &[u8]) -> String {
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let bytes = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let number = (bytes[0] as u32) << 16 | (bytes[1] as u32) << 8 | bytes[2] as u32;
        for index in 0..4 {
            if index <= chunk.len() {
                out.push(BASE64[(number >> (18 - 6 * index) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// One connection to the shared app-server daemon of Codex, through its own command `codex app-server proxy`.
pub struct Daemon {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
    waiting: VecDeque<Value>,
    number: i64,
    said: Option<String>,
    closed: bool,
}

impl Daemon {
    pub fn new(environment: &Env, wait: f64) -> Result<Daemon, DaemonError> {
        let program = tell::codex_program(environment);
        if program.is_empty() {
            return Err(DaemonError::Refused("на этом Маке нет codex".into()));
        }
        let mut command = Command::new(&program);
        command.args(["app-server", "proxy"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
            .env_clear().envs(environment).current_dir(tell::home_dir());
        let mut child = command.spawn().map_err(|error| DaemonError::Refused(format!("codex не запустился: {}", tell::file_error(&error, &program))))?;
        let (stdin, stdout, stderr) = (child.stdin.take(), child.stdout.take(), child.stderr.take());
        let mut daemon = Daemon { child, stdin, stdout, stderr, waiting: VecDeque::new(), number: 0, said: None, closed: false };
        let begun = daemon.handshake(wait).and_then(|_| {
            daemon.call("initialize", json!({"clientInfo": {"name": "hitl", "title": "Конвейер", "version": "1"},
                                             "capabilities": {"experimentalApi": true}}), wait)
        }).and_then(|_| daemon.send(&json!({"method": "initialized"}), 1, &[]));
        match begun {
            Ok(()) => Ok(daemon),
            Err(error) => {
                daemon.close();
                Err(error)
            }
        }
    }

    fn handshake(&mut self, wait: f64) -> Result<(), DaemonError> {
        let key = base64(&tell::random_bytes(16));
        self.write(format!("GET / HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\n\
                            Sec-WebSocket-Version: 13\r\n\r\n").as_bytes())?;
        let deadline = Instant::now() + Duration::from_secs_f64(wait);
        let mut head: Vec<u8> = Vec::new();
        while claude_find(&head, b"\r\n\r\n").is_none() {
            head.extend(self.exactly(1, deadline)?);
        }
        let status = head.split(|byte| *byte == b'\r').next().unwrap_or(&[]).to_vec();
        if claude_find(&status, b" 101 ").is_none() {
            return Err(DaemonError::Refused(format!("демон Codex не принял соединение: {}", String::from_utf8_lossy(&status))));
        }
        Ok(())
    }

    fn write(&mut self, data: &[u8]) -> Result<(), DaemonError> {
        let written = match self.stdin.as_mut() {
            Some(stdin) => stdin.write_all(data).and_then(|_| stdin.flush()),
            None => Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe)),
        };
        written.map_err(|error| {
            let reason = self.reason();
            DaemonError::Refused(format!("демон Codex закрыл соединение: {}", if reason.is_empty() { tell::os_error(&error) } else { reason }))
        })
    }

    /// What the proxy said on its way out, if it is out.
    fn reason(&mut self) -> String {
        if self.said.is_none() && matches!(self.child.try_wait(), Ok(Some(_))) {
            if let Some(mut stderr) = self.stderr.take() {
                let mut text = Vec::new();
                let _ = stderr.read_to_end(&mut text);
                self.said = Some(one_line(&String::from_utf8_lossy(&text), 300));
            }
        }
        self.said.clone().unwrap_or_default()
    }

    fn exactly(&mut self, size: usize, deadline: Instant) -> Result<Vec<u8>, DaemonError> {
        let mut data = Vec::with_capacity(size);
        while data.len() < size {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(DaemonError::Late("демон Codex не ответил вовремя".into()));
            }
            let Some(stdout) = self.stdout.as_mut() else { return Err(DaemonError::Refused("демон Codex закрыл соединение".into())) };
            let mut poll = libc::pollfd { fd: stdout.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            let ready = unsafe { libc::poll(&mut poll, 1, remaining.as_millis().clamp(1, i32::MAX as u128) as i32) };
            if ready <= 0 {
                continue;
            }
            let mut chunk = vec![0u8; size - data.len()];
            let count = stdout.read(&mut chunk).unwrap_or(0);
            if count == 0 {
                let deadline = Instant::now() + Duration::from_secs(2);
                while Instant::now() < deadline && matches!(self.child.try_wait(), Ok(None)) {
                    std::thread::sleep(Duration::from_millis(10));
                }
                let reason = self.reason();
                return Err(DaemonError::Refused(format!("демон Codex закрыл соединение{}", if reason.is_empty() { String::new() } else { format!(": {reason}") })));
            }
            data.extend_from_slice(&chunk[..count]);
        }
        Ok(data)
    }

    fn send(&mut self, message: &Value, opcode: u8, data: &[u8]) -> Result<(), DaemonError> {
        let body = if opcode == 1 { common::dumps_line(message).into_bytes() } else { data.to_vec() };
        let mask = tell::random_bytes(4);
        let size = body.len();
        let mut frame = vec![0x80 | opcode];
        if size < 126 {
            frame.push(0x80 | size as u8);
        } else if size < 1 << 16 {
            frame.push(0x80 | 126);
            frame.extend((size as u16).to_be_bytes());
        } else {
            frame.push(0x80 | 127);
            frame.extend((size as u64).to_be_bytes());
        }
        frame.extend(&mask);
        frame.extend(body.iter().enumerate().map(|(index, byte)| byte ^ mask[index % 4]));
        self.write(&frame)
    }

    /// The next message from the daemon; pings are answered on the way.
    fn receive(&mut self, deadline: Instant) -> Result<Value, DaemonError> {
        let mut parts: Vec<u8> = Vec::new();
        loop {
            let head = self.exactly(2, deadline)?;
            let (first, second) = (head[0], head[1]);
            let opcode = first & 0x0f;
            let mut size = (second & 0x7f) as u64;
            if size == 126 {
                let bytes = self.exactly(2, deadline)?;
                size = u16::from_be_bytes([bytes[0], bytes[1]]) as u64;
            } else if size == 127 {
                let bytes = self.exactly(8, deadline)?;
                size = u64::from_be_bytes(bytes.try_into().unwrap_or([0; 8]));
            }
            let mask = if second & 0x80 != 0 { self.exactly(4, deadline)? } else { Vec::new() };
            let mut data = self.exactly(size as usize, deadline)?;
            if !mask.is_empty() {
                for (index, byte) in data.iter_mut().enumerate() {
                    *byte ^= mask[index % 4];
                }
            }
            match opcode {
                9 => {
                    self.send(&Value::Null, 10, &data)?;
                    continue;
                }
                10 => continue,
                8 => return Err(DaemonError::Refused("демон Codex закрыл соединение".into())),
                _ => {}
            }
            parts.extend(data);
            if first & 0x80 != 0 {
                match serde_json::from_slice::<Value>(&parts) {
                    Ok(message) if message.is_object() => return Ok(message),
                    _ => parts.clear(),
                }
            }
        }
    }

    /// One request and its result; what arrives meanwhile is kept for `next`. The daemon's own error says it did not
    /// do it (Refused); a connection that ends or stays silent after the request went out does not (Unanswered).
    pub fn call(&mut self, method: &str, params: Value, wait: f64) -> Result<Value, DaemonError> {
        self.number += 1;
        let number = self.number;
        self.send(&json!({"id": number, "method": method, "params": params}), 1, &[])?;
        let deadline = Instant::now() + Duration::from_secs_f64(wait);
        let message = loop {
            let answered = self.receive(deadline).and_then(|message| {
                let ours = message.get("id").and_then(Value::as_f64) == Some(number as f64)
                    && (message.get("result").is_some() || message.get("error").is_some());
                if ours { Ok(Some(message)) } else { self.answer(message).map(|_| None) }
            });
            match answered {
                Ok(Some(message)) => break message,
                Ok(None) => continue,
                Err(error) => return Err(DaemonError::Unanswered(format!("{error} (запрос {method} ушёл, ответа нет)"))),
            }
        };
        if let Some(error) = message.get("error") {
            let error = if error.is_object() { error.clone() } else { json!({"message": error}) };
            let said = match error.get("message") {
                Some(text) if truthy(Some(text)) => python_str(text),
                _ => python_str(&error),
            };
            return Err(DaemonError::Refused(one_line(&said, 400)));
        }
        Ok(message.get("result").cloned().unwrap_or(Value::Null))
    }

    /// A request of the daemon's own (an approval) is refused: nobody is at this connection to approve it. A
    /// notification is kept for whoever waits for it.
    fn answer(&mut self, message: Value) -> Result<(), DaemonError> {
        if message.get("id").is_some() && message.get("method").is_some() {
            self.send(&json!({"id": message["id"], "error": {"code": -32601, "message": "hitl does not answer this"}}), 1, &[])
        } else {
            if message.get("method").is_some() {
                self.waiting.push_back(message);
            }
            Ok(())
        }
    }

    /// The next notification, or None when none came in time.
    pub fn next(&mut self, wait: f64) -> Result<Option<Value>, DaemonError> {
        if let Some(message) = self.waiting.pop_front() {
            return Ok(Some(message));
        }
        let deadline = Instant::now() + Duration::from_secs_f64(wait);
        loop {
            let message = match self.receive(deadline) {
                Ok(message) => message,
                Err(DaemonError::Late(_)) => return Ok(None),
                Err(error) => return Err(error),
            };
            self.answer(message)?;
            if let Some(message) = self.waiting.pop_front() {
                return Ok(Some(message));
            }
        }
    }

    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        self.stdin.take();
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline && matches!(self.child.try_wait(), Ok(None)) {
            std::thread::sleep(Duration::from_millis(10));
        }
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        self.stdout.take();
        self.stderr.take();
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        self.close();
    }
}

/// A connection to the daemon; with `start`, the daemon is started by Codex's own command when it is not up.
pub fn daemon(environment: &Env, start: bool) -> Result<Daemon, DaemonError> {
    match Daemon::new(environment, 20.0) {
        Ok(daemon) => return Ok(daemon),
        Err(error) if !start => return Err(error),
        Err(_) => {}
    }
    let program = tell::codex_program(environment);
    // The server is shared by unrelated chats: a caller's journal correlation is not handed to it.
    let settings: Env = environment.iter().filter(|(key, _)| {
        !["CONVEYOR_TRACE_ID", "CONVEYOR_PARENT_ACTION", "CONVEYOR_ACTOR", "CONVEYOR_SOURCE"].contains(&key.as_str())
    }).map(|(key, value)| (key.clone(), value.clone())).collect();
    if !program.is_empty() {
        let mut command = Command::new(&program);
        command.args(["app-server", "daemon", "start"]).env_clear().envs(&settings);
        let _ = tell::execute(&mut command, &program, 60.0);
    }
    Daemon::new(environment, 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn his_words_leave_the_context_codex_puts_around_them() {
        assert_eq!(his_words("<environment_context>\n<cwd>/x</cwd>\n</environment_context>\nпривет"), "привет");
        assert_eq!(his_words("# AGENTS.md instructions for /x\n..."), "");
        assert_eq!(his_words("  <open attr=1>не закрыт"), "");
        assert_eq!(his_words("<a>x</a><b>y</b>  ## My request for Codex:\n  сделай"), "сделай");
        assert_eq!(his_words("<abc>x</ab> хвост"), "хвост");
        assert_eq!(his_words("просто текст <b>"), "просто текст <b>");
    }

    #[test]
    fn the_kinds_of_a_line_are_read_from_its_head() {
        assert_eq!(line_kind(br#"{"timestamp":"x","type": "response_item","payload":{"type":"message"}}"#), Some("response_item"));
        assert_eq!(payload_kind(br#"{"type":"response_item","payload": { "type": "message","role":"user"}}"#).as_deref(), Some("message"));
        assert_eq!(payload_kind(br#"{"payload":{"type":"message2"}}"#), None);
        assert_eq!(line_kind(br#"{"type":"session_meta"}"#), None);
    }

    #[test]
    fn base64_is_the_standard_one() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(b"opencode:secret"), "b3BlbmNvZGU6c2VjcmV0");
    }
}
