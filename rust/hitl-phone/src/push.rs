//! Notifications: the Mac asks Apple itself, signed with his key (`AuthKey_<id>.p8`, as he downloaded it), when a check
//! waits and he is not at the Mac. «A notification is for a queue that stands.»

use crate::{codec, py, word};
use hitl_host_core::process::{self, Options};
use hitl_host_core::var;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Whose key signs a notification.
pub fn team() -> String {
    var("CONVEYOR_PHONE_TEAM").unwrap_or_else(|| "9FJ339SL23".into())
}

/// Where Apple takes a notification for a build made in Xcode.
fn push_host() -> String {
    var("CONVEYOR_PHONE_PUSH_HOST").unwrap_or_else(|| "api.sandbox.push.apple.com".into())
}

/// Apple's key for notifications, as he downloaded it: AuthKey_<its id>.p8.
pub fn push_key() -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(word::folder()).ok()?.flatten().map(|entry| entry.path()).filter(|path| {
        let name = path.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default();
        name.starts_with("AuthKey_") && name.ends_with(".p8")
    }).collect();
    found.sort();
    found.pop()
}

/// An ES256 signature as a token carries it: the two numbers, 32 bytes each, out of openssl's DER.
pub fn raw_signature(der: &[u8]) -> Result<Vec<u8>, String> {
    let refused = || "not a signature".to_string();
    let number = |at: usize| -> Result<(Vec<u8>, usize), String> {
        if der.get(at) != Some(&0x02) {
            return Err(refused());
        }
        let length = usize::from(*der.get(at + 1).ok_or_else(refused)?);
        let start = at + 2;
        let bytes = der.get(start..start + length).ok_or_else(refused)?;
        Ok((bytes.to_vec(), start + length))
    };
    if der.first() != Some(&0x30) || der.len() < 2 {
        return Err(refused());
    }
    let first = if der[1] < 0x80 { 2 } else { 2 + usize::from(der[1] & 0x7F) };
    let (r, after) = number(first)?;
    let (s, _) = number(after)?;
    let mut out = Vec::with_capacity(64);
    for part in [r, s] {
        let digits: Vec<u8> = part.into_iter().skip_while(|&byte| byte == 0).collect();
        if digits.len() > 32 {
            return Err(refused());
        }
        out.extend(std::iter::repeat_n(0u8, 32 - digits.len()));
        out.extend(digits);
    }
    Ok(out)
}

/// What says to Apple that a notification comes from his team: signed with the key, good for an hour.
pub fn push_token(key: &Path, moment: Option<f64>) -> Result<String, String> {
    let stem = key.file_stem().map(|stem| stem.to_string_lossy().to_string()).unwrap_or_default();
    let id = stem.split_once('_').map_or(stem.as_str(), |(_, id)| id);
    let head = codec::url64(json!({"alg": "ES256", "kid": id}).to_string().as_bytes());
    let at = moment.filter(|moment| *moment != 0.0).unwrap_or_else(hitl_host_core::now) as i64;
    let claims = codec::url64(json!({"iss": team(), "iat": at}).to_string().as_bytes());
    let signed = format!("{head}.{claims}");
    let key_text = key.display().to_string();
    let done = process::run("openssl", &["dgst", "-sha256", "-sign", &key_text],
                            &Options::new("phone.push_token").timeout(15.0).input(signed.as_bytes()))
        .map_err(|error| format!("openssl did not start: {error}"))?;
    if !done.success() {
        let said = done.err();
        let tail: String = said.trim().chars().rev().take(200).collect::<Vec<_>>().into_iter().rev().collect();
        return Err(format!("openssl did not sign: {tail}"));
    }
    Ok(format!("{signed}.{}", codec::url64(&raw_signature(&done.stdout)?)))
}

/// What the phone shows of a check that waits: who asks and what kind it is, the check, its first item.
pub fn notice(ticket: &Value, waiting: usize) -> Value {
    let folder = py::or(ticket.get("cwd"), py::or(ticket.get("project"), json!("")));
    let folder_name = folder.as_str().map(py::name).unwrap_or_default();
    let mut who = py::or(ticket.get("chat"), py::or(ticket.get("tag"), json!("")));
    if !hitl_host_core::truthy(Some(&who)) {
        let parts: Vec<String> = [py::or(ticket.get("agent"), json!("")), json!(folder_name)].iter()
            .filter(|part| hitl_host_core::truthy(Some(part))).map(py::str_of).collect();
        who = json!(parts.join(" · "));
    }
    let who = if hitl_host_core::truthy(Some(&who)) { py::str_of(&who) } else { "чат без имени".into() };
    let items = ticket.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut body = items.first().map(|item| hitl_host_core::read_text(&py::str_of(&py::get(item, "text", json!(""))))).unwrap_or_default();
    if items.len() > 1 {
        body += &format!(" · ещё {}", hitl_host_core::items_word(items.len() as i64 - 1));
    }
    let project = if hitl_host_core::truthy(ticket.get("projectName")) { format!("{} · ", py::str_of(&ticket["projectName"])) } else { String::new() };
    let kind = py::str_of(&py::or(ticket.get("kindTitle"), json!("проверка")));
    json!({"aps": {"alert": {"title": format!("{project}{who} · {kind}"), "subtitle": py::or(ticket.get("title"), json!("")),
                             "body": body},
                   "sound": "default", "badge": waiting, "thread-id": "hitl"},
           "ticket": ticket.get("id").cloned().unwrap_or(Value::Null)})
}

/// One notification to one phone, through Apple; the answer's number and what Apple said.
pub fn send_push(device: &str, payload: &Value, kind: &str) -> Result<(u16, String), String> {
    let Some(key) = push_key() else { return Ok((0, "no key".into())) };
    let body = hitl_host_core::dumps_line(payload).into_bytes();
    if let Some(stand_in) = var("CONVEYOR_PHONE_PUSH").filter(|named| !named.is_empty()) {
        // this project's own checks: a program that takes the place of Apple
        let done = process::run(stand_in.as_str(), &[device, kind], &Options::new("phone.push").timeout(30.0).input(&body))
            .map_err(|error| error.to_string())?;
        return Ok((if done.success() { 200 } else { 500 }, done.out()));
    }
    let token = push_token(&key, None)?;
    let (authorization, topic) = (format!("authorization: bearer {token}"), format!("apns-topic: {}", word::bundle()));
    let (push_type, priority) = (format!("apns-push-type: {kind}"), format!("apns-priority: {}", if kind == "alert" { "10" } else { "5" }));
    let address = format!("https://{}/3/device/{device}", push_host());
    let arguments = ["--silent", "--http2", "--max-time", "20", "--output", "-", "--write-out", "\n%{http_code}",
                     "--header", &authorization, "--header", &topic, "--header", &push_type, "--header", &priority,
                     "--data-binary", "@-", &address];
    let done = process::run("curl", &arguments, &Options::new("phone.push").timeout(40.0).input(&body)).map_err(|error| error.to_string())?;
    if done.timed_out {
        return Err("curl timed out after 40 seconds".into());
    }
    let text = done.out();
    let (said, code) = text.rsplit_once('\n').unwrap_or(("", text.as_str()));
    let said = if said.trim().is_empty() { done.err().trim().to_string() } else { said.trim().to_string() };
    Ok((code.trim().parse().unwrap_or(0), said))
}

/// How long ago the Mac's keyboard and mouse were last touched.
pub fn seconds_idle() -> f64 {
    if let Some(named) = var("CONVEYOR_PHONE_IDLE") {
        return named.trim().parse().unwrap_or(0.0);
    }
    let Ok(done) = process::run("ioreg", &["-c", "IOHIDSystem", "-d", "4"], &Options::new("phone.idle").timeout(5.0).unjournaled()) else {
        return 0.0;
    };
    let said = done.out();
    let Some(at) = said.find("\"HIDIdleTime\"") else { return 0.0 };
    let rest = said[at + "\"HIDIdleTime\"".len()..].trim_start();
    let Some(rest) = rest.strip_prefix('=') else { return 0.0 };
    let digits: String = rest.trim_start().chars().take_while(char::is_ascii_digit).collect();
    digits.parse::<f64>().map(|nanoseconds| nanoseconds / 1e9).unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signature_is_taken_out_of_der() {
        // r with a leading zero (its high bit set), s short.
        let mut der = vec![0x30, 0x26, 0x02, 0x21, 0x00];
        der.extend([0x80u8; 32]);
        der.extend([0x02, 0x01, 0x07]);
        let raw = raw_signature(&der).unwrap();
        assert_eq!((raw.len(), raw[0], raw[31], raw[63], raw[62]), (64, 0x80, 0x80, 0x07, 0x00));
        assert!(raw_signature(&[0x31, 0x00]).is_err() && raw_signature(&[0x30, 0x02, 0x02]).is_err());
    }

    #[test]
    fn a_notice_says_who_asks_and_the_first_item_as_he_reads_it() {
        let ticket = json!({"id": "t1", "chat": "", "tag": "", "agent": "Codex", "cwd": "/x/realize/", "kindTitle": "",
                            "title": "Вход", "projectName": "realize",
                            "items": [{"text": "Форма после [team/320](https://realize.day/t/team/320) закрывается"}, {"text": "2"}]});
        assert_eq!(notice(&ticket, 3), json!({"aps": {"alert": {"title": "realize · Codex · realize · проверка", "subtitle": "Вход",
                                                                 "body": "Форма после team/320 закрывается · ещё 1 пункт"},
                                                        "sound": "default", "badge": 3, "thread-id": "hitl"},
                                               "ticket": "t1"}));
        assert_eq!(notice(&json!({"id": "t2", "items": []}), 1)["aps"]["alert"]["title"], json!("чат без имени · проверка"));
    }
}
