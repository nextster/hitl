//! `hitl phone`: what is known of the phone, the Mac's word put on it by cable, and the server for the way outside
//! home put into his Cloudflare account and taken out of it.
//!
//!   hitl phone             what is known of the phone: whether it is paired, connected, has notifications
//!   hitl phone pair        put the Mac's address and key on the phone that is on the cable, now
//!   hitl phone relay       whether the server is set up, and whether it answers
//!   hitl phone relay up    put the server into his Cloudflare account, and tell the phone of it
//!   hitl phone relay down  take the server out of the account, and its word off this Mac

use crate::word::{self, folder, phones, relay_word};
use crate::{bridge, codec, push};
use hitl_host_core::process::{self, Options};
use hitl_host_core::{EXIT_REFUSED, now, read_json, say, var, write_json};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;

pub const USAGE: &str = "Usage:
  hitl phone             What is known of the phone: paired or not, connected or not, notifications.
  hitl phone pair        Put the Mac's name, port and key on the phone that is on the cable, now.
  hitl phone relay       Whether the server for the way outside home is set up and answers.
  hitl phone relay up    Put that server into his Cloudflare account (wrangler, signed in), keep its
                             address and keys on this Mac and tell the phone of them.
  hitl phone relay down  Take the server out of the account and forget it here.

The app «HITL» on the phone shows the same queue as the check window on the Mac, and an answer
pressed there is an answer pressed in the window. Nothing is to be started by hand: the process that
talks to the phone comes up with the window, and ends when nothing waits. At home the phone finds the
Mac by itself; anywhere else the two meet in a room of the server, where what they say is sealed with
a key the server does not have.
";

/// The server's own name in his account.
fn relay_name() -> String {
    var("CONVEYOR_RELAY_NAME").unwrap_or_else(|| "conveyor-relay".into())
}

/// The server's code, which wrangler puts there.
fn relay_source() -> PathBuf {
    hitl_host_core::root().join("relay")
}

/// Cloudflare's own program for putting a server into an account, as it is found on this Mac.
fn wrangler() -> Option<Vec<String>> {
    if let Some(named) = var("CONVEYOR_RELAY_TOOL").filter(|named| !named.is_empty()) {
        return Some(vec![named]);
    }
    if let Some(found) = hitl_host_core::which("wrangler") {
        return Some(vec![found.display().to_string()]);
    }
    hitl_host_core::which("npx").map(|runner| vec![runner.display().to_string(), "--yes".into(), "wrangler".into()])
}

fn last_lines(text: &str, count: usize) -> String {
    let lines: Vec<&str> = text.trim().lines().collect();
    lines[lines.len().saturating_sub(count)..].join("\n")
}

/// What a program said, its errors first, as Python's `put.stderr or put.stdout`.
fn said_by(done: &process::Output) -> String {
    if done.err().is_empty() { done.out() } else { done.err() }
}

/// The reason phrase of an HTTP status, as urllib names it.
fn reason(code: u16) -> &'static str {
    match code {
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        426 => "Upgrade Required",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "",
    }
}

/// Whether the server knows this Mac's word, and what it said if not. It is asked plainly, with no WebSocket: coming
/// into the room to look would take the place of the process that is in it.
pub fn relay_answers(known: &Value) -> (bool, String) {
    let text = |key: &str| known.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    let url = text("url");
    let url = url.trim_end_matches('/');
    let url = if let Some(rest) = url.strip_prefix("ws") { format!("http{rest}") } else { url.to_string() };
    let address = format!("{url}/room/{}/mac", text("room"));
    let authorization = format!("Authorization: Bearer {}", text("token"));
    let arguments = ["--silent", "--show-error", "--max-time", "10", "--header", &authorization, "--header", "User-Agent: hitl",
                     "--output", "-", "--write-out", "\n%{http_code}", &address];
    let done = match process::run("curl", &arguments, &Options::new("phone.relay_answers").timeout(20.0)) {
        Ok(done) => done,
        Err(error) => return (false, error.to_string()),
    };
    let out = done.out();
    let (body, code) = out.rsplit_once('\n').unwrap_or(("", out.as_str()));
    let code: u16 = code.trim().parse().unwrap_or(0);
    if code == 0 || !done.success() {
        let said = done.err().trim().to_string();
        return (false, if said.is_empty() { format!("curl ended with {:?}", done.code) } else { said });
    }
    if !(200..300).contains(&code) {
        return (false, format!("{code} {}", reason(code)).trim().to_string());
    }
    match serde_json::from_str::<Value>(body) {
        Ok(Value::Object(said)) if said.contains_key("mac") => (true, String::new()),
        Ok(_) => (false, "оно ответило не как комната".into()),
        Err(error) => (false, error.to_string()),
    }
}

fn relay_up() -> i32 {
    let Some(tool) = wrangler() else {
        say("hitl phone relay: на Маке нет программы wrangler. Поставь: npm install -g wrangler, потом wrangler login.");
        return EXIT_REFUSED;
    };
    let source = relay_source();
    if !source.join("worker.js").is_file() {
        say(&format!("hitl phone relay: нет кода сервера в {}.", source.display()));
        return EXIT_REFUSED;
    }
    let kept = read_json(&folder().join("relay.json")).filter(Value::is_object).unwrap_or_else(|| json!({}));
    let keep = |key: &str, made: String| kept.get(key).and_then(Value::as_str).filter(|value| !value.is_empty()).map(str::to_string).unwrap_or(made);
    // The room, the word the server lets in by and the key that seals what is said are made once and kept: a phone
    // that knows them goes on knowing the server after it is put there again.
    let (room, token, key) = (keep("room", codec::token_hex(16)), keep("token", codec::token_urlsafe(32)), keep("key", codec::token_hex(32)));
    let name = relay_name();
    say(&format!("==> Ставлю сервер «{name}» в аккаунт Cloudflare…"));
    let run = |words: &[&str], input: Option<&str>, seconds: f64| {
        let mut arguments: Vec<String> = tool[1..].to_vec();
        arguments.extend(words.iter().map(|word| word.to_string()));
        let mut options = Options::new("phone.relay").cwd(&source).timeout(seconds);
        if let Some(input) = input {
            options = options.input(input.as_bytes());
        }
        process::run(tool[0].as_str(), &arguments.iter().map(String::as_str).collect::<Vec<_>>(), &options)
    };
    let put = match run(&["deploy", "--name", &name], None, 300.0) {
        Ok(put) => put,
        Err(error) => {
            say(&format!("hitl phone relay: wrangler не запустился: {error}"));
            return 1;
        }
    };
    let both = put.out() + &put.err();
    let address = both.split("https://").skip(1).find_map(|rest| {
        let host: String = rest.chars().take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '.' || *c == '-').collect();
        host.ends_with(".workers.dev").then_some(host)
    });
    let Some(address) = address.filter(|_| put.success()) else {
        say(&format!("hitl phone relay: сервер не встал. Последнее, что сказал wrangler:\n{}", last_lines(&said_by(&put), 8)));
        return 1;
    };
    let worded = match run(&["secret", "put", "KEY", "--name", &name], Some(&token), 180.0) {
        Ok(worded) => worded,
        Err(error) => {
            say(&format!("hitl phone relay: wrangler не запустился: {error}"));
            return 1;
        }
    };
    if !worded.success() {
        say(&format!("hitl phone relay: сервер встал, но не взял слово, по которому пускает:\n{}", last_lines(&said_by(&worded), 8)));
        return 1;
    }
    let url = var("CONVEYOR_RELAY_URL").filter(|url| !url.is_empty()).unwrap_or_else(|| format!("wss://{address}"));
    let known = json!({"room": room, "token": token, "key": key, "url": url});
    let mut written = known.clone();
    written["at"] = json!(now());
    let file = folder().join("relay.json");
    if let Err(error) = write_json(&file, &written) {
        say(&format!("hitl phone relay: {}: {error}", file.display()));
        return 1;
    }
    let _ = std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600));
    say(&format!("==> Сервер стоит: {url}"));
    let (mut answers, mut said) = (false, String::new());
    for attempt in 0..6 {
        // a server put there a second ago may not answer by its address yet
        (answers, said) = relay_answers(&known);
        if answers {
            break;
        }
        if attempt < 5 {
            std::thread::sleep(Duration::from_secs(5));
        }
    }
    if answers {
        say("==> Сервер пускает этот Мак в комнату.");
    } else {
        say(&format!("==> Сервер пока не пускает этот Мак ({said}): посмотри `hitl phone relay` через минуту."));
    }
    if !phones().is_empty() && !word::pair(false).unwrap_or(false) {
        say("==> На телефон адрес сервера попадёт сам: по кабелю или когда «HITL» откроется дома.");
    }
    if answers { 0 } else { 1 }
}

fn relay_down() -> i32 {
    let Some(tool) = wrangler() else {
        say("hitl phone relay: на Маке нет программы wrangler.");
        return EXIT_REFUSED;
    };
    let name = relay_name();
    let mut arguments: Vec<String> = tool[1..].to_vec();
    arguments.extend(["delete".into(), "--name".into(), name, "--force".into()]);
    let options = Options::new("phone.relay").cwd(&relay_source()).timeout(180.0).input(b"y\n");
    let gone = match process::run(tool[0].as_str(), &arguments.iter().map(String::as_str).collect::<Vec<_>>(), &options) {
        Ok(gone) => gone,
        Err(error) => {
            say(&format!("hitl phone relay: wrangler не запустился: {error}"));
            return 1;
        }
    };
    if !gone.success() {
        say(&format!("hitl phone relay: сервер не убран:\n{}", last_lines(&said_by(&gone), 8)));
        return 1;
    }
    let _ = std::fs::remove_file(folder().join("relay.json"));
    say("==> Сервер убран из аккаунта, его адрес и ключи стёрты с этого Мака.");
    if !phones().is_empty() {
        let _ = word::pair(true);
    }
    0
}

fn relay_lines() -> Vec<String> {
    let Some(known) = relay_word() else {
        return vec!["  Сервер для связи вне дома: не поставлен (`hitl phone relay up`)".into()];
    };
    let (answers, said) = relay_answers(&known);
    let how = if answers { "отвечает".to_string() } else { format!("не отвечает ({said})") };
    vec![format!("  Сервер для связи вне дома: {}, {how}", known["url"].as_str().unwrap_or(""))]
}

fn status_lines() -> Vec<String> {
    let mut lines = vec!["Приложение «HITL» на телефоне".to_string()];
    match read_json(&folder().join("paired.json")).filter(Value::is_object) {
        Some(paired) => {
            let hosts: Vec<String> = paired.get("hosts").and_then(Value::as_array).into_iter().flatten().map(crate::py::str_of).collect();
            let port = crate::py::str_of(paired.get("port").unwrap_or(&Value::Null));
            lines.push(format!("  Адрес Мака записан на телефон {}: {}, порт {port}",
                               hitl_host_core::clock(paired.get("at").and_then(Value::as_f64)), hosts.join(", ")));
        }
        None => lines.push("  Адрес Мака на телефон ещё не записан: подключи телефон кабелем, `hitl phone pair`".into()),
    }
    let link = if bridge::running() { "работает, пока в очереди есть проверки" } else { "спит, поднимется с первой проверкой" };
    lines.push(format!("  Связь с телефоном: {link}"));
    let device = read_json(&folder().join("device.json"));
    if push::push_key().is_none() {
        lines.push(format!("  Уведомления: нет ключа Apple в {}", folder().display()));
    } else if !device.as_ref().is_some_and(|device| hitl_host_core::truthy(device.get("push"))) {
        lines.push("  Уведомления: ключ есть, телефон ещё не сказал, куда их слать (открой приложение)".into());
    } else {
        lines.push("  Уведомления: приходят, когда проверка ждёт, а ты не за Маком".into());
    }
    lines.extend(relay_lines());
    lines
}

/// `hitl phone ARGUMENTS`.
pub fn main(arguments: &[String]) -> i32 {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["-h" | "--help" | "help", ..] => {
            print!("{USAGE}");
            0
        }
        ["_serve"] => bridge::serve(),
        ["pair"] => {
            if phones().is_empty() {
                say("hitl phone: ни один телефон ещё не был в очереди проверок на этом Маке.");
                return EXIT_REFUSED;
            }
            match word::pair(false) {
                Ok(told) => i32::from(!told),
                Err(error) => {
                    say(&format!("hitl phone: {error}"));
                    1
                }
            }
        }
        ["relay"] => {
            println!("{}", relay_lines().join("\n"));
            0
        }
        ["relay", "up"] => relay_up(),
        ["relay", "down"] => relay_down(),
        [] => {
            println!("{}", status_lines().join("\n"));
            0
        }
        _ => {
            say(&format!("hitl phone: {USAGE}"));
            EXIT_REFUSED
        }
    }
}
