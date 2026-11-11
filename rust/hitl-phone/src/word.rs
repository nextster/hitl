//! What the phone knows the Mac by: the key it says to be heard, the port it comes to, the certificate it expects by
//! its print, the Mac's name and addresses at home, and the server's room; put on the phone over the cable, into the
//! app's own folder.

use crate::{codec, py};
use hitl_host_core::process::{self, Options};
use hitl_host_core::{now, read_json, say, var, write_json};
use serde_json::{Map, Value, json};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The app that shows the queue, and where the Mac's word for it lies in its folder on the phone.
pub fn bundle() -> String {
    var("CONVEYOR_PHONE_BUNDLE").unwrap_or_else(|| "app.realize.conveyor-demo".into())
}

pub const PAIR_PATH: &str = "Library/Application Support/hitl/mac.json";

/// What is kept for the phone: the key it is known by, the certificate, Apple's key for notifications.
pub fn folder() -> PathBuf {
    hitl_host_core::home().join("phone")
}

/// Where the window says what it shows and takes the phone's presses from.
pub fn remote_folder() -> PathBuf {
    hitl_host_core::home().join("window").join("remote")
}

/// A line of the process's log: the moment, then what happened.
pub fn log(message: &str) {
    let (year, month, day, hour, minute, second) = hitl::common::local(now());
    say(&format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} {message}"));
}

fn chmod(path: &Path, mode: u32) {
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
}

/// A port nothing listens on now.
pub fn free_port() -> u16 {
    std::net::TcpListener::bind("[::]:0").or_else(|_| std::net::TcpListener::bind("0.0.0.0:0"))
        .and_then(|probe| probe.local_addr()).map(|place| place.port()).unwrap_or(0)
}

fn openssl(arguments: &[&str]) -> Result<(), String> {
    let done = process::run("openssl", arguments, &Options::new("phone.certificate").timeout(60.0))
        .map_err(|error| format!("openssl did not start: {error}"))?;
    if done.success() {
        return Ok(());
    }
    let said = done.err();
    let said = said.trim();
    let tail: String = said.chars().rev().take(300).collect::<Vec<_>>().into_iter().rev().collect();
    Err(format!("the certificate for the phone was not made: {tail}"))
}

/// The key the phone says to be heard, the port it comes to and the certificate it expects; made once.
pub fn pairing() -> Result<Value, String> {
    let home = folder();
    std::fs::create_dir_all(&home).map_err(|error| format!("{}: {error}", home.display()))?;
    chmod(&home, 0o700);
    let kept = read_json(&home.join("pair.json")).filter(|kept| {
        kept.is_object() && hitl_host_core::truthy(kept.get("token")) && hitl_host_core::truthy(kept.get("port"))
    });
    let kept = match kept {
        Some(kept) => kept,
        None => {
            let port = var("CONVEYOR_PHONE_PORT").and_then(|port| port.trim().parse::<u16>().ok()).filter(|port| *port > 0)
                .unwrap_or_else(free_port);
            let kept = json!({"token": codec::token_urlsafe(32), "port": port});
            write_json(&home.join("pair.json"), &kept).map_err(|error| error.to_string())?;
            chmod(&home.join("pair.json"), 0o600);
            kept
        }
    };
    let (certificate, key) = (home.join("cert.pem"), home.join("key.pem"));
    if !(certificate.exists() && key.exists()) {
        let (certificate_text, key_text) = (certificate.display().to_string(), key.display().to_string());
        openssl(&["ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", &key_text])?;
        openssl(&["req", "-new", "-x509", "-key", &key_text, "-out", &certificate_text, "-days", "3650", "-subj", "/CN=hitl"])?;
        chmod(&key, 0o600);
    }
    let pin = codec::hex(&codec::sha256(&crate::tls::certificate(&certificate)?));
    let mut word = kept.as_object().cloned().unwrap_or_default();
    word.insert("pin".into(), json!(pin));
    Ok(Value::Object(word))
}

// The Mac's name changes when he renames the Mac, and every look at the queue, up to twenty a second while a phone
// listens, started `scutil` for it (the audit of 2026-10-16, O02): the name is asked once a minute.
const NAME_SECONDS: u64 = 60;
static NAMED_AT: Mutex<Option<(Instant, String)>> = Mutex::new(None);

pub fn mac_name(fresh: bool) -> String {
    mac_name_at(Instant::now(), fresh)
}

/// The Mac's name as of `moment`, asked of the system again when the last answer is a minute old.
pub fn mac_name_at(moment: Instant, fresh: bool) -> String {
    if let Some(name) = var("CONVEYOR_PHONE_NAME") {
        return name;
    }
    let mut named = NAMED_AT.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((at, name)) = named.as_ref() {
        if !fresh && moment >= *at && moment.duration_since(*at) < Duration::from_secs(NAME_SECONDS) {
            return name.clone();
        }
    }
    let asked = process::run("scutil", &["--get", "LocalHostName"], &Options::new("phone.mac_name").timeout(5.0).unjournaled())
        .ok().filter(|done| !done.timed_out).map(|done| done.out().trim().to_string()).unwrap_or_default();
    let name = if asked.is_empty() { hostname().split('.').next().unwrap_or("").to_string() } else { asked };
    *named = Some((moment, name.clone()));
    name
}

/// Forgets the name asked last, as a start of the process does: for this crate's own tests.
#[doc(hidden)]
pub fn forget_mac_name() {
    *NAMED_AT.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}

fn hostname() -> String {
    let mut buffer = [0u8; 256];
    if unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) } != 0 {
        return String::new();
    }
    let end = buffer.iter().position(|&byte| byte == 0).unwrap_or(buffer.len());
    String::from_utf8_lossy(&buffer[..end]).to_string()
}

/// Where the phone may find the Mac at home: by its name on the local network, then by its number there.
pub fn addresses() -> Vec<String> {
    if let Some(named) = var("CONVEYOR_PHONE_HOSTS") {
        return named.split(',').filter(|host| !host.is_empty()).map(str::to_string).collect();
    }
    let mut found = vec![format!("{}.local", mac_name(false))];
    // No packet is sent: the system only says which of its addresses it would speak from.
    if let Ok(probe) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if probe.connect("192.0.2.1:9").is_ok() {
            if let Ok(place) = probe.local_addr() {
                let number = place.ip().to_string();
                if !number.is_empty() && !number.starts_with("127.") && number != "0.0.0.0" {
                    found.push(number);
                }
            }
        }
    }
    found
}

/// The room of the server where the Mac and the phone meet when they are not in one network, once it is set up
/// (`hitl phone relay up`): its address, the room, the word the server lets in by, and the key that seals what is
/// said there.
pub fn relay_word() -> Option<Value> {
    if var("CONVEYOR_PHONE_RELAY").as_deref() == Some("0") {
        return None;
    }
    let kept = read_json(&folder().join("relay.json"))?;
    let mut word = Map::new();
    for name in ["url", "room", "token", "key"] {
        let value = kept.get(name).and_then(Value::as_str).filter(|value| !value.is_empty())?;
        word.insert(name.into(), json!(value));
    }
    Some(Value::Object(word))
}

/// What is put on the phone by cable.
pub fn phone_word() -> Result<Value, String> {
    let known = pairing()?;
    let mut word = Map::new();
    word.insert("name".into(), json!(mac_name(true)));
    word.insert("hosts".into(), json!(addresses()));
    for name in ["port", "token", "pin"] {
        word.insert(name.into(), known.get(name).cloned().unwrap_or(Value::Null));
    }
    if let Some(far) = relay_word() {
        word.insert("relay".into(), far);
    }
    Ok(Value::Object(word))
}

/// `json.dumps(value, sort_keys=True)`: ", " and ": " between, keys in order, everything past ASCII escaped.
pub fn sorted_dump(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let inner: Vec<String> = keys.iter().map(|key| format!("{}: {}", ascii(&Value::String((*key).clone())), sorted_dump(&map[*key])))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
        Value::Array(items) => format!("[{}]", items.iter().map(sorted_dump).collect::<Vec<_>>().join(", ")),
        other => ascii(other),
    }
}

fn ascii(value: &Value) -> String {
    let text = value.to_string();
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        if character.is_ascii() {
            out.push(character);
        } else {
            let mut units = [0u16; 2];
            for unit in character.encode_utf16(&mut units) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    out
}

/// What is kept of a word put on the phone, to know later whether the phone still has it right: where the Mac is
/// found, and a print of the server's room, not its keys.
pub fn told_of(word: &Value) -> Value {
    let far = word.get("relay").filter(|far| hitl_host_core::truthy(Some(far)));
    let room = far.map(|far| codec::hex(&codec::sha256(sorted_dump(far).as_bytes()))[..16].to_string()).unwrap_or_default();
    json!({"hosts": word.get("hosts").cloned().unwrap_or(Value::Null), "port": word.get("port").cloned().unwrap_or(Value::Null),
           "pin": word.get("pin").cloned().unwrap_or(Value::Null), "relay": room})
}

/// The phones that have been through the queue of hand checks: the ones to tell.
pub fn phones() -> Vec<String> {
    let root = hitl_host_core::home().join("iphone");
    let mut found: Vec<String> = std::fs::read_dir(&root).map(|entries| {
        entries.flatten().filter(|entry| entry.path().is_dir()).map(|entry| entry.file_name().to_string_lossy().to_string()).collect()
    }).unwrap_or_default();
    found.sort();
    found
}

/// Puts the Mac's word into the app's folder on a phone; whether it was taken, and what was said if not.
pub fn put_on_phone(udid: &str, word: &Value) -> (bool, String) {
    let scratch = std::env::temp_dir().join(format!("hitl-phone.{}.{}", std::process::id(), codec::token_hex(4)));
    if let Err(error) = std::fs::create_dir_all(&scratch) {
        return (false, error.to_string());
    }
    let source = scratch.join("mac.json");
    let result = std::fs::write(&source, hitl_host_core::dumps_line(word)).map_err(|error| error.to_string()).and_then(|()| {
        let source = source.display().to_string();
        let bundle = bundle();
        let arguments = ["devicectl", "device", "copy", "to", "--device", udid, "--domain-type", "appDataContainer",
                         "--domain-identifier", &bundle, "--source", &source, "--destination", PAIR_PATH, "--timeout", "15"];
        let done = process::run("xcrun", &arguments, &Options::new("phone.pair").timeout(45.0)).map_err(|error| error.to_string())?;
        if done.timed_out {
            return Err("xcrun devicectl timed out after 45 seconds".into());
        }
        let said = if done.err().trim().is_empty() { done.out() } else { done.err() };
        Ok((done.success(), said.trim().lines().last().unwrap_or("").to_string()))
    });
    let _ = std::fs::remove_dir_all(&scratch);
    result.unwrap_or_else(|error| (false, error))
}

/// Tells every known phone how to find this Mac. True when one took it.
pub fn pair(quiet: bool) -> Result<bool, String> {
    let word = phone_word()?;
    let mut told = false;
    for udid in phones() {
        let (taken, said) = put_on_phone(&udid, &word);
        told = told || taken;
        if !quiet {
            let how = if taken { "адрес и ключ Мака записаны в «HITL».".to_string() } else { format!("не записал ({said}).") };
            say(&format!("==> Телефон {udid}: {how}"));
        }
    }
    if told {
        let mut kept = Map::new();
        kept.insert("at".into(), json!(now()));
        if let Value::Object(more) = told_of(&word) {
            kept.extend(more);
        }
        let _ = write_json(&folder().join("paired.json"), &Value::Object(kept));
    }
    Ok(told)
}

/// Whether what the phone was last told is still how this Mac is found.
pub fn is_paired() -> bool {
    let Some(Value::Object(told)) = read_json(&folder().join("paired.json")) else { return false };
    let Ok(word) = phone_word() else { return false };
    let Value::Object(known) = told_of(&word) else { return false };
    known.iter().all(|(key, value)| py::get(&Value::Object(told.clone()), key, json!("")) == *value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_room_is_printed_as_python_printed_it() {
        // json.dumps({"url": …, "room": …, "token": …, "key": …}, sort_keys=True), and its print, as Python made them.
        let far = json!({"url": "wss://r.example.workers.dev", "room": "5f5f", "token": "слово", "key": "a7a7"});
        // The word in the token escaped as Python's ensure_ascii does: a backslash, u, four hex digits a letter.
        let escaped: String = ["0441", "043b", "043e", "0432", "043e"].iter().map(|code| format!("{}u{code}", '\\')).collect();
        assert_eq!(sorted_dump(&far),
                   format!(r#"{{"key": "a7a7", "room": "5f5f", "token": "{escaped}", "url": "wss://r.example.workers.dev"}}"#));
        let told = told_of(&json!({"hosts": ["mac.local"], "port": 4, "pin": "p", "relay": far, "token": "secret"}));
        assert_eq!(told, json!({"hosts": ["mac.local"], "port": 4, "pin": "p", "relay": "f2534128d7ea2674"}));
        assert_eq!(told_of(&json!({"hosts": [], "port": 4, "pin": "p", "relay": null}))["relay"], json!(""));
    }
}
