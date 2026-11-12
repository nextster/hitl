//! `hitl phone`: the process that carries the window's queue to the app on the phone and its presses to the window.
//!
//! No phone is touched and nothing is heard from the network: the process listens on this Mac's own loopback, with a
//! made-up phone talking to it, a made-up Apple, and a made-up server for the way outside home, which keeps a room as
//! `relay/worker.js` does. The process is the program the file `hitl` builds, as chats run it; checks are queued
//! by `hitl check`. The window's own Swift is driven by a script with presses of the phone laid out for it, never
//! shown and never holding the keyboard. What was `tests/phone-tests.py` before the move.

use hitl_phone::relay::{self, Message, Seal, Wire};
use hitl_phone::{codec, push, shown, tls, word};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::io::{BufRead, ErrorKind, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

const ABOUT: &str = "Плагин Obsidian для Realize: делаю ему вход без пароля.";
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

fn png() -> Vec<u8> {
    codec::unbase64(PNG).expect("a picture")
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("the project")
}

fn read(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))).expect("JSON")
}

fn write(path: &Path, value: &Value) {
    std::fs::create_dir_all(path.parent().expect("a folder")).expect("a folder");
    std::fs::write(path, value.to_string()).expect("written");
}

/// Tests here change this process's environment, which the programs they start take: one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

/// The program built once before the first test, so that no test waits for the build.
fn built() {
    static BUILT: OnceLock<()> = OnceLock::new();
    BUILT.get_or_init(|| {
        let done = Command::new(root().join("hitl")).args(["phone", "--help"]).stdout(Stdio::null()).stderr(Stdio::null())
            .status().expect("hitl runs");
        assert!(done.success(), "hitl phone --help");
    });
}

/// A made-up home, no window, a chat of its own, and nothing of the phone's that is real; with the process of
/// `hitl phone` in it when a test starts one, a made-up Apple, and the window's word said by the test itself.
struct Case {
    home: PathBuf,
    saved: Vec<(String, Option<String>)>,
    process: Option<Child>,
    pushes: PathBuf,
    word: Value,
    _serial: MutexGuard<'static, ()>,
}

impl Case {
    fn new() -> Case {
        let serial = SERIAL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        built();
        let home = std::env::temp_dir().join(format!("hitl-phone-tests.{}.{}", std::process::id(), codec::token_hex(6)));
        std::fs::create_dir_all(&home).expect("a home");
        let home = home.canonicalize().expect("a home");
        let mut case = Case { pushes: home.join("pushes"), home, saved: Vec::new(), process: None, word: Value::Null, _serial: serial };
        let cards = case.home.join("vault/cards").display().to_string();
        for (name, value) in [("HITL_HOME", case.home.display().to_string().as_str()),
                              ("CONVEYOR_HOME", case.home.display().to_string().as_str()), ("CONVEYOR_WINDOW", ""), ("CONVEYOR_NOTIFY", "0"),
                              ("CONVEYOR_CHAT", "r/66 · API для плагина"), ("CONVEYOR_CARDS", &cards), ("CONVEYOR_CODEX", ""),
                              ("CONVEYOR_PHONE_NAME", "mac"), ("CONVEYOR_PHONE_HOSTS", "localhost"), ("CONVEYOR_PHONE_PAIR", "0")] {
            case.set(name, Some(value));
        }
        for name in ["CONVEYOR_PHONE_PORT", "CONVEYOR_PHONE_PUSH", "CONVEYOR_PHONE_IDLE", "CONVEYOR_PHONE_BIND", "CONVEYOR_PHONE_RELAY",
                     "CONVEYOR_RELAY_TOOL", "CONVEYOR_RELAY_URL", "CONVEYOR_PHONE_IDLE_EXIT", "CONVEYOR_WINDOW_SCRIPT"] {
            case.set(name, None);
        }
        case
    }

    /// A home the process of `hitl phone` can be started in: a port of its own on the loopback, a made-up Apple.
    fn bridge() -> Case {
        let mut case = Case::new();
        // A made-up Apple: every notification it is asked to show is a file, its first line the phone and the kind.
        let apple = case.home.join("apple.sh");
        let pushes = case.pushes.display().to_string();
        std::fs::write(&apple, format!("#!/bin/sh\nmkdir -p \"{pushes}\"\n{{ echo \"$1 $2\"; cat; }} > \"{pushes}/$$.tmp\"\n\
                                        mv \"{pushes}/$$.tmp\" \"{pushes}/$$.txt\"\n")).expect("Apple");
        chmod(&apple, 0o755);
        let port = word::free_port().to_string();
        let apple = apple.display().to_string();
        for (name, value) in [("CONVEYOR_PHONE_BIND", "::1"), ("CONVEYOR_PHONE_PORT", port.as_str()), ("CONVEYOR_PHONE_PUSH", &apple),
                              ("CONVEYOR_PHONE_IDLE", "0")] {
            case.set(name, Some(value));
        }
        case.word = word::phone_word().expect("the Mac's word");
        case
    }

    fn set(&mut self, name: &str, value: Option<&str>) {
        if !self.saved.iter().any(|(saved, _)| saved == name) {
            self.saved.push((name.to_string(), std::env::var(name).ok()));
        }
        // SAFETY: SERIAL is held: no other test of this binary reads or changes the environment meanwhile.
        unsafe {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
    }

    fn port(&self) -> u16 {
        self.word["port"].as_u64().expect("a port") as u16
    }

    /// A check queued as a chat queues it.
    fn queue(&self, arguments: &[&str]) -> String {
        let done = Command::new(root().join("hitl")).args(["check", "--about", ABOUT]).args(arguments).arg("--no-wait")
            .current_dir(root()).output().expect("hitl check runs");
        let id = String::from_utf8_lossy(&done.stdout).trim().to_string();
        assert!(!id.is_empty() && matches!(done.status.code(), Some(0 | 75)), "hitl check: {}", String::from_utf8_lossy(&done.stderr));
        id
    }

    fn ticket(&self, id: &str) -> PathBuf {
        self.home.join("queue").join(format!("{id}.json"))
    }

    /// A check's ticket, changed as a test wants it.
    fn change(&self, id: &str, change: impl FnOnce(&mut Value)) {
        let mut ticket = read(&self.ticket(id));
        change(&mut ticket);
        write(&self.ticket(id), &ticket);
    }

    fn serve(&mut self) {
        let log = std::fs::OpenOptions::new().create(true).append(true).open(self.home.join("bridge.log")).expect("a log");
        let child = Command::new(root().join("hitl")).args(["phone", "_serve"]).stdin(Stdio::null())
            .stdout(log.try_clone().expect("a log")).stderr(log).spawn().expect("the process starts");
        self.process = Some(child);
        let until = Instant::now() + Duration::from_secs(30);
        while Instant::now() < until {
            if TcpStream::connect_timeout(&format!("[::1]:{}", self.port()).parse().expect("an address"), Duration::from_secs(1)).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("the process did not start listening: {}", std::fs::read_to_string(self.home.join("bridge.log")).unwrap_or_default());
    }

    /// What the process wrote to its log, and where its threads stand when it still runs: said when a test of it fails.
    fn log(&self) -> String {
        let mut said = std::fs::read_to_string(self.home.join("bridge.log")).unwrap_or_default();
        if let Some(child) = &self.process {
            let sampled = Command::new("/usr/bin/sample").args([&child.id().to_string(), "1"]).output();
            said += &sampled.map(|done| String::from_utf8_lossy(&done.stdout).to_string()).unwrap_or_default();
        }
        said
    }

    fn kill(&mut self) {
        if let Some(mut child) = self.process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Waits for the process to end by itself: its exit code.
    fn ended(&mut self, seconds: u64) -> Option<i32> {
        let child = self.process.as_mut().expect("a process");
        let until = Instant::now() + Duration::from_secs(seconds);
        while Instant::now() < until {
            if let Ok(Some(status)) = child.try_wait() {
                self.process = None;
                return status.code();
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        None
    }

    fn call(&self) -> Caller {
        Caller::new(&self.word, None, json!({}))
    }

    /// What the window would say of itself; this test stands in for it, and is alive.
    fn window_says(&self, state: Value) {
        let mut said = json!({"seq": 1, "pid": std::process::id(), "active": null, "index": 0, "marks": [], "order": [], "kept": [],
                              "canPostpone": false, "canFinish": false, "editing": false});
        for (key, value) in state.as_object().expect("an object") {
            said[key] = value.clone();
        }
        write(&self.home.join("window/remote/state.json"), &said);
    }

    fn presses(&self) -> Vec<Value> {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(self.home.join("window/remote/in")).map(|entries| {
            entries.flatten().map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|extension| extension == "json")).collect()
        }).unwrap_or_default();
        paths.sort();
        paths.iter().map(|path| read(path)).collect()
    }

    fn wait_presses(&self, count: usize, seconds: u64) -> Vec<Value> {
        let until = Instant::now() + Duration::from_secs(seconds);
        while self.presses().len() < count && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(50));
        }
        self.presses()
    }

    /// Apple's key for notifications, made up.
    fn key(&self) {
        let key = self.home.join("phone/AuthKey_TESTKEY123.p8");
        std::fs::create_dir_all(key.parent().expect("a folder")).expect("a folder");
        let done = Command::new("openssl").args(["ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out"]).arg(&key).output()
            .expect("openssl");
        assert!(done.status.success());
    }

    /// The notifications the made-up Apple was asked to show: which phone and kind, and what.
    fn told(&self) -> Vec<(String, Value)> {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(&self.pushes).map(|entries| {
            entries.flatten().map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|extension| extension == "txt")).collect()
        }).unwrap_or_default();
        paths.sort();
        paths.iter().map(|path| {
            let text = std::fs::read_to_string(path).expect("a notification");
            let (head, body) = text.split_once('\n').expect("a head");
            (head.to_string(), serde_json::from_str(body).expect("JSON"))
        }).collect()
    }
}

impl Drop for Case {
    fn drop(&mut self) {
        self.kill();
        for (name, value) in self.saved.drain(..).rev() {
            // SAFETY: SERIAL is still held.
            unsafe {
                match value {
                    Some(value) => std::env::set_var(&name, value),
                    None => std::env::remove_var(&name),
                }
            }
        }
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

fn chmod(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("chmod");
}

/// A made-up phone at home: it knows the Mac by what `hitl phone pair` would have put on it.
struct Caller {
    link: rustls::ClientConnection,
    socket: TcpStream,
    heard: Vec<u8>,
    closed: bool,
}

impl Caller {
    fn new(word: &Value, token: Option<&str>, hello: Value) -> Caller {
        let port = word["port"].as_u64().expect("a port") as u16;
        let mut socket = TcpStream::connect_timeout(&format!("[::1]:{port}").parse().expect("an address"), Duration::from_secs(5))
            .expect("the Mac answers");
        socket.set_read_timeout(Some(Duration::from_secs(5))).expect("a timeout");
        let mut link = tls::trusting_client();
        while link.is_handshaking() {
            link.complete_io(&mut socket).expect("TLS");
        }
        // The phone trusts one certificate, by its print.
        let certificate = link.peer_certificates().expect("a certificate")[0].to_vec();
        assert_eq!(json!(codec::hex(&codec::sha256(&certificate))), word["pin"]);
        let mut caller = Caller { link, socket, heard: Vec::new(), closed: false };
        let mut said = json!({"token": token.map_or_else(|| word["token"].clone(), |token| json!(token))});
        for (key, value) in hello.as_object().expect("an object") {
            said[key] = value.clone();
        }
        caller.say(&json!({"hello": said}));
        caller
    }

    fn say(&mut self, word: &Value) {
        let mut line = word.to_string().into_bytes();
        line.push(b'\n');
        let _ = self.link.writer().write_all(&line);
        while self.link.wants_write() {
            if self.link.write_tls(&mut self.socket).is_err() {
                break;
            }
        }
    }

    /// Takes in what came within `wait`; false once the Mac has hung up.
    fn fill(&mut self, wait: Duration) -> bool {
        if self.closed {
            return false;
        }
        let mut buffer = vec![0u8; 1 << 16];
        loop {
            match self.link.reader().read(&mut buffer) {
                Ok(0) => {
                    self.closed = true;
                    return false;
                }
                Ok(count) => {
                    self.heard.extend_from_slice(&buffer[..count]);
                    return true;
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => {}
                Err(_) => {
                    self.closed = true;
                    return false;
                }
            }
            let _ = self.socket.set_read_timeout(Some(wait.max(Duration::from_millis(1))));
            match self.link.read_tls(&mut self.socket) {
                Ok(0) => {
                    self.closed = true;
                    return false;
                }
                Ok(_) => {
                    if self.link.process_new_packets().is_err() {
                        self.closed = true;
                        return false;
                    }
                }
                Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => return true,
                Err(_) => {
                    self.closed = true;
                    return false;
                }
            }
        }
    }

    /// The first word that is the wanted one, or None when the Mac hung up or said nothing of the kind.
    fn hear(&mut self, wanted: impl Fn(&Value) -> bool, seconds: f64) -> Option<Value> {
        let until = Instant::now() + Duration::from_secs_f64(seconds);
        while Instant::now() < until {
            while let Some(end) = self.heard.iter().position(|&byte| byte == b'\n') {
                let line: Vec<u8> = self.heard.drain(..=end).collect();
                let word: Value = serde_json::from_slice(&line[..line.len() - 1]).expect("a line of JSON");
                if wanted(&word) {
                    return Some(word);
                }
            }
            if !self.fill(Duration::from_millis(100)) {
                return None;
            }
        }
        None
    }

    fn queue(&mut self, when: impl Fn(&Value) -> bool) -> Option<Value> {
        self.hear(|word| word.get("queue").is_some_and(&when), 8.0).map(|word| word["queue"].clone())
    }

    /// Whether the Mac closes this connection within that time.
    fn is_hung_up_on(&mut self, seconds: f64) -> bool {
        let until = Instant::now() + Duration::from_secs_f64(seconds);
        while Instant::now() < until {
            self.heard.clear();
            if !self.fill(Duration::from_millis(100)) {
                return true;
            }
        }
        false
    }
}

/// Waits for `done` to come true, up to `seconds`: whether it did.
fn until(seconds: u64, done: impl Fn() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(seconds);
    while !done() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
    }
    done()
}

fn any(_: &Value) -> bool {
    true
}

fn text(value: &str) -> Value {
    json!(value)
}

// ----------------------------------------------------------------------------------------------------------------------
// The process at home

#[test]
fn the_phone_is_shown_the_queue_with_the_windows_word() {
    let mut case = Case::bridge();
    let picture = case.home.join("screen.png");
    std::fs::write(&picture, png()).unwrap();
    let note = case.home.join("plan.md");
    std::fs::write(&note, "# План\n").unwrap();
    let (picture_text, note_text) = (picture.display().to_string(), note.display().to_string());
    let first = case.queue(&["--title", "Вход", "--kind", "result", "--check", "1. Плагин входит", "--details", "Проверено руками.",
                             "--fact", "14 **тестов**", "--image", &picture_text, "--link", "https://github.com/o/r/pull/12",
                             "--link", &note_text, "--check", "2. Влить?"]);
    let second = case.queue(&["--title", "Репозиторий", "--kind", "question", "--check", "1. Где?", "--option", "Свой", "--option",
                              "В Realize", "--many", "--check", "2. Запусти команду", "--fact", "Она спросит вход", "--copy",
                              "hitl phone relay up"]);
    case.window_says(json!({"active": first, "index": 1, "order": [second, first], "canPostpone": true,
                            "marks": [{"label": "1", "state": "ok", "note": ""}, {"label": "2", "note": ""}],
                            "kept": [{"id": second, "marked": 1}]}));
    case.serve();
    let seen = case.call().queue(any).expect("the queue");
    assert_eq!((&seen["mac"], &seen["window"], &seen["active"], &seen["index"], &seen["canPostpone"]),
               (&text("mac"), &json!(true), &json!(first), &json!(1), &json!(true)));
    let checks: Vec<(Value, Value, Value)> = seen["checks"].as_array().unwrap().iter()
        .map(|one| (one["id"].clone(), one["marked"].clone(), one["kindTitle"].clone())).collect();
    // in the window's own order
    assert_eq!(checks, vec![(json!(second), json!(1), text("вопрос")), (json!(first), json!(1), text("результат"))]);
    let result = &seen["checks"][1];
    assert_eq!((&result["chat"], &result["tag"], &result["about"], &result["answers"]),
               (&text("r/66 · API для плагина"), &text("r/66"), &text(ABOUT), &json!(["ок", "не ок", "пропустить"])));
    // Which of his projects the check is about goes with it: the app says it first, as the window does.
    let project = hitl::common::project_name(Some(&root().display().to_string()));
    assert!(!project.is_empty());
    assert_eq!(result["projectName"], json!(project));
    let item = &result["items"][0];
    assert_eq!((&item["label"], &item["text"], &item["details"], &item["facts"]),
               (&text("1"), &text("Плагин входит"), &text("Проверено руками."), &json!(["14 **тестов**"])));
    assert_eq!(item["pictures"], json!([picture.canonicalize().unwrap().display().to_string()]));
    assert_eq!(item["opens"], json!([{"title": "PR #12", "link": "https://github.com/o/r/pull/12", "kind": "web"},
                                     {"title": "plan.md", "link": note_text, "kind": "file"}]));
    let question = &seen["checks"][0]["items"][0];
    assert_eq!((&question["options"], &question["many"]), (&json!(["Свой", "В Realize"]), &json!(true)));
    // A line he copies with a click in the window goes to the app as such, and among the item's facts for a build that
    // knows none.
    let command = &seen["checks"][0]["items"][1];
    assert_eq!((&command["copies"], &command["facts"], &item["copies"]),
               (&json!(["hitl phone relay up"]), &json!(["Она спросит вход", "hitl phone relay up"]), &json!([])));
}

#[test]
fn a_link_in_a_text_goes_to_the_phone_and_a_file_it_names_can_be_asked_for() {
    // A link a chat wrote into a text of a check goes to the app as written, for the app to make a link of; one to a
    // file of this Mac by the file's full path, and that file is one the phone may ask for.
    let mut case = Case::bridge();
    let note = case.home.join("план работ.md");
    std::fs::write(&note, "# План\n").unwrap();
    let user = PathBuf::from(std::env::var("HOME").unwrap()).canonicalize().unwrap();
    let short = match note.strip_prefix(&user) {
        Ok(rest) => format!("~/{}", rest.display()), // as a chat writes a path, from his home folder
        Err(_) => note.display().to_string(),
    };
    let full = hitl_phone::py::expanded(&short);
    let note_text = note.display().to_string();
    let first = case.queue(&["--title", "Форма", "--kind", "question", "--about", &format!("Правлю форму по [плану]({short})."),
                             "--check", "1. Форма после [team/320](https://realize.day/t/team/320) закрывается рывком",
                             "--details", &format!("Рывок описан в [плане работ]({short})."),
                             "--fact", &format!("См. [план]({note_text}) и [PR](https://github.com/o/r/pull/1)")]);
    case.window_says(json!({"active": first, "order": [first], "marks": [{"label": "1", "note": ""}]}));
    case.serve();
    let mut caller = case.call();
    let seen = caller.queue(any).expect("the queue")["checks"][0].clone();
    let item = &seen["items"][0];
    assert_eq!(item["text"], text("Форма после [team/320](https://realize.day/t/team/320) закрывается рывком"));
    assert_eq!((&seen["about"], &item["details"]),
               (&json!(format!("Правлю форму по [плану]({full}).")), &json!(format!("Рывок описан в [плане работ]({full})."))));
    assert_eq!(item["facts"], json!([format!("См. [план]({note_text}) и [PR](https://github.com/o/r/pull/1)")]));
    caller.say(&json!({"get": full, "id": 1}));
    let got = caller.hear(|word| word["file"] == json!(1), 8.0).expect("the file");
    assert_eq!((&got["name"], codec::unbase64(got["data"].as_str().unwrap()).unwrap()), (&text("план работ.md"), "# План\n".as_bytes().to_vec()));
    // A notification says the item as he reads it.
    let ticket = read(&case.ticket(&first));
    assert_eq!(push::notice(&ticket, 1)["aps"]["alert"]["body"], text("Форма после team/320 закрывается рывком"));
}

#[test]
fn an_item_of_a_kind_of_its_own_comes_with_its_answers() {
    let mut case = Case::bridge();
    let first = case.queue(&["--title", "Вход", "--kind", "question", "--check", "1. Шлёт сам hitl merge", "--as", "explain",
                             "--check", "2. Убрать его?", "--check", "3. Войди в Namecheap", "--as", "action"]);
    case.window_says(json!({"active": first, "order": [first],
                            "marks": [{"label": "1", "note": ""}, {"label": "2", "note": ""}, {"label": "3", "note": ""}]}));
    case.serve();
    let seen = case.call().queue(any).expect("the queue");
    let one = &seen["checks"][0];
    let kinds: Vec<Value> = one["items"].as_array().unwrap().iter()
        .map(|item| json!([item["kind"], item["kindTitle"], item["answers"]])).collect();
    assert_eq!(kinds, vec![json!(["explain", "объяснение", ["понятно", "не понятно"]]), json!([null, null, null]),
                           json!(["action", "действие", ["сделал", "не сделаю", "позже"]])]);
    assert_eq!((&one["kind"], &one["answers"]), (&text("question"), &json!(["да", "нет", "пропустить"])));
}

#[test]
fn a_check_queued_later_and_an_answer_in_the_window_come_by_themselves() {
    let mut case = Case::bridge();
    let first = case.queue(&["--title", "Вход", "--check", "1. Раз"]);
    case.window_says(json!({"active": first, "order": [first], "marks": [{"label": "1", "note": ""}]}));
    case.serve();
    let mut caller = case.call();
    assert_eq!(caller.queue(any).expect("the queue")["checks"].as_array().unwrap().len(), 1);
    let second = case.queue(&["--title", "Вторая", "--check", "1. Раз"]);
    let seen = caller.queue(|seen| seen["checks"].as_array().is_some_and(|checks| checks.len() == 2)).expect("two checks");
    let ids: Vec<Value> = seen["checks"].as_array().unwrap().iter().map(|one| one["id"].clone()).collect();
    assert_eq!(ids, vec![json!(first), json!(second)]);
    case.window_says(json!({"seq": 2, "active": first, "order": [first, second], "marks": [{"label": "1", "state": "ok", "note": ""}],
                            "canFinish": true}));
    let seen = caller.queue(|seen| seen["canFinish"] == json!(true)).expect("the window's answer");
    assert_eq!((&seen["marks"][0]["state"], &seen["checks"][0]["marked"]), (&text("ok"), &json!(1)));
}

#[test]
fn a_press_is_handed_to_the_window_as_it_takes_it() {
    let mut case = Case::bridge();
    let first = case.queue(&["--title", "Вход", "--check", "1. Раз"]);
    case.window_says(json!({"active": first, "order": [first], "marks": [{"label": "1", "note": ""}]}));
    case.serve();
    let mut caller = case.call();
    caller.queue(any).expect("the queue");
    caller.say(&json!({"do": "answer", "ticket": first, "label": "1", "number": 1, "anything": "else"}));
    caller.say(&json!({"do": "note", "ticket": first, "label": "1", "text": "не открывается"}));
    caller.say(&json!({"do": "rm -rf", "ticket": first}));
    caller.say(&json!({"shot": {"ticket": first, "label": "1", "data": PNG}}));
    caller.say(&json!({"shot": {"ticket": "../../x", "label": "1", "data": PNG}}));
    caller.say(&json!({"do": "finish", "ticket": first, "how": "rework"}));
    let presses = case.wait_presses(4, 8);
    assert_eq!(presses[0], json!({"do": "answer", "ticket": first, "label": "1", "number": 1}));
    assert_eq!(presses[1], json!({"do": "note", "ticket": first, "label": "1", "text": "не открывается"}));
    assert_eq!((&presses[2]["do"], &presses[2]["label"]), (&text("shot"), &text("1")));
    let shot = PathBuf::from(presses[2]["path"].as_str().unwrap());
    assert_eq!((std::fs::read(&shot).unwrap(), shot.parent().unwrap().to_path_buf()), (png(), case.home.join("shots")));
    assert_eq!(presses[3], json!({"do": "finish", "ticket": first, "how": "rework"}));
    assert_eq!(presses.len(), 4);
}

#[test]
fn a_phone_that_comes_again_takes_the_place_of_its_earlier_connection() {
    let mut case = Case::bridge();
    let first = case.queue(&["--title", "Вход", "--check", "1. Раз"]);
    case.window_says(json!({"active": first, "order": [first], "marks": [{"label": "1", "note": ""}]}));
    case.serve();
    let mut earlier = case.call();
    earlier.queue(any).expect("the queue");
    let mut later = case.call(); // the app was ended without a word and started again
    later.queue(any).expect("the queue");
    assert!(earlier.is_hung_up_on(5.0));
    case.window_says(json!({"seq": 2, "active": first, "order": [first], "marks": [{"label": "1", "state": "ok", "note": ""}],
                            "canFinish": true}));
    assert!(later.queue(|seen| seen["canFinish"] == json!(true)).is_some());
}

#[test]
fn only_a_phone_that_says_the_key_is_told_anything() {
    let mut case = Case::bridge();
    let first = case.queue(&["--title", "Вход", "--check", "1. Раз"]);
    case.window_says(json!({"active": first, "order": [first], "marks": [{"label": "1", "note": ""}]}));
    case.serve();
    let mut stranger = Caller::new(&case.word, Some("не тот ключ"), json!({}));
    assert_eq!(stranger.hear(any, 3.0), None);
    stranger.say(&json!({"do": "finish", "ticket": first, "how": "rework"}));
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(case.presses(), Vec::<Value>::new());
    assert!(case.call().queue(any).is_some());
}

#[test]
fn the_app_a_check_came_from_goes_to_the_phone_as_its_icon() {
    // «слева от имени проекта только символом если можно. если сложно то текстом коротенечко», and «нет» to the phone
    // going without it. The phone has no way to the Mac's apps: the window keeps each app's icon as a file.
    let mut case = Case::bridge();
    let mut tickets = Vec::new();
    for agent in ["OpenCode", "T3 Code", "Codex", "../../phone/pair"] {
        let title = format!("От {}", agent.chars().take(8).collect::<String>());
        let id = case.queue(&["--title", &title, "--check", "1. Раз"]);
        case.change(&id, |ticket| ticket["agent"] = json!(agent));
        tickets.push(id);
    }
    let icons = case.home.join("window/remote/icons");
    std::fs::create_dir_all(&icons).unwrap();
    std::fs::write(icons.join("OpenCode.png"), png()).unwrap();
    std::fs::write(icons.join("T3 Code.png"), png()).unwrap();
    std::fs::write(case.home.join("phone/pair.png"), png()).unwrap(); // not an icon, whatever a check calls its agent
    case.window_says(json!({"active": tickets[0], "order": tickets, "marks": [{"label": "1", "note": ""}]}));
    case.serve();
    let mut caller = case.call();
    let seen: HashMap<String, String> = caller.queue(any).expect("the queue")["checks"].as_array().unwrap().iter()
        .map(|one| (one["agent"].as_str().unwrap().to_string(), one["appIcon"].as_str().unwrap().to_string())).collect();
    assert_eq!(PathBuf::from(&seen["OpenCode"]).canonicalize().unwrap(), icons.join("OpenCode.png").canonicalize().unwrap());
    assert_eq!(PathBuf::from(&seen["T3 Code"]).canonicalize().unwrap(), icons.join("T3 Code.png").canonicalize().unwrap());
    // An app this Mac has no icon of is said by its name, and so is whatever a check calls its agent.
    assert_eq!((seen["Codex"].as_str(), seen["../../phone/pair"].as_str()), ("", ""));
    caller.say(&json!({"get": seen["T3 Code"], "id": 1}));
    let got = caller.hear(|word| word["file"] == json!(1), 8.0).expect("the icon");
    assert_eq!(codec::unbase64(got["data"].as_str().unwrap()).unwrap(), png());
    caller.say(&json!({"get": icons.join("Codex.png").display().to_string(), "id": 2}));
    assert_eq!(caller.hear(|word| word["file"] == json!(2), 8.0), Some(json!({"file": 2, "error": "этого файла нет в очереди"})));
}

#[test]
fn a_check_that_waits_is_told_to_the_phone_when_he_is_away() {
    let mut case = Case::bridge();
    case.key();
    case.set("CONVEYOR_PHONE_IDLE", Some("0")); // he is at the Mac
    let first = case.queue(&["--title", "Репозиторий плагина", "--kind", "question", "--check", "1. Где лежит репозиторий?",
                             "--check", "2. А это?"]);
    case.change(&first, |ticket| ticket["createdAt"] = json!(ticket["createdAt"].as_f64().unwrap() - 10.0));
    case.window_says(json!({"active": first, "order": [first], "marks": [{"label": "1", "note": ""}, {"label": "2", "note": ""}]}));
    case.serve();
    let mut caller = Caller::new(&case.word, None, json!({"push": "ab".repeat(32), "device": "iPhone", "front": true}));
    caller.queue(any).expect("the queue");
    // The check is on the phone's screen: the process keeps that the phone has seen it.
    let pushed = case.home.join("phone/pushed.json");
    assert!(until(8, || std::fs::read_to_string(&pushed).is_ok_and(|kept| kept.contains(&first))), "{}", case.log());
    drop(caller); // the app is put away
    assert!(until(8, || std::fs::read_to_string(case.home.join("bridge.log")).is_ok_and(|log| log.contains(" left"))), "{}", case.log());
    assert!(!case.pushes.exists()); // it was on the phone's screen: nothing to tell

    let second = case.queue(&["--title", "Вход", "--kind", "result", "--check", "1. Плагин входит"]);
    case.change(&second, |ticket| ticket["createdAt"] = json!(ticket["createdAt"].as_f64().unwrap() - 10.0));
    std::thread::sleep(Duration::from_millis(1500));
    assert!(!case.pushes.exists()); // he is at the Mac, and the window is in front of him

    case.kill();
    case.set("CONVEYOR_PHONE_IDLE", Some("600")); // he has been away for ten minutes
    case.serve();
    assert!(until(8, || !case.told().is_empty()), "{}", case.log());
    std::thread::sleep(Duration::from_millis(500)); // and no other comes after it
    // Only the one he has not seen: the first was on the phone's screen, and that is kept across a start.
    let told = case.told();
    assert_eq!(told.len(), 1, "{told:?}");
    let (head, said) = &told[0];
    assert_eq!(head, &format!("{} alert", "ab".repeat(32)));
    // The project first, as in the window and in the app: he holds several at once.
    let project = hitl::common::project_name(Some(&root().display().to_string()));
    assert_eq!(said["aps"]["alert"], json!({"title": format!("{project} · r/66 · API для плагина · результат"), "subtitle": "Вход",
                                            "body": "Плагин входит"}));
    assert_eq!((&said["ticket"], &said["aps"]["badge"]), (&json!(second), &json!(2)));
    // Started again in the middle of the same queue, the process does not tell of it a second time.
    case.kill();
    case.serve();
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(case.told().len(), 1);
}

#[test]
fn a_token_for_apple_is_signed_with_his_key() {
    let case = Case::new();
    case.key();
    let key = push::push_key().expect("the key");
    let token = push::push_token(&key, Some(1_800_000_000.0)).unwrap();
    let parts: Vec<&str> = token.split('.').collect();
    let unpad = |part: &str| {
        let standard: String = part.chars().map(|c| match c { '-' => '+', '_' => '/', other => other }).collect();
        let padding = "=".repeat((4 - standard.len() % 4) % 4);
        codec::unbase64(&(standard + &padding)).expect("base64")
    };
    assert_eq!(serde_json::from_slice::<Value>(&unpad(parts[0])).unwrap(), json!({"alg": "ES256", "kid": "TESTKEY123"}));
    assert_eq!(serde_json::from_slice::<Value>(&unpad(parts[1])).unwrap(), json!({"iss": push::team(), "iat": 1_800_000_000}));
    let raw = unpad(parts[2]);
    assert_eq!(raw.len(), 64);
    // openssl takes the signature back in the form it gave it in.
    let number = |part: &[u8]| {
        let mut digits: Vec<u8> = part.iter().copied().skip_while(|&byte| byte == 0).collect();
        if digits.is_empty() {
            digits.push(0);
        }
        if digits[0] & 0x80 != 0 {
            digits.insert(0, 0);
        }
        let mut out = vec![0x02, digits.len() as u8];
        out.extend(digits);
        out
    };
    let body = [number(&raw[..32]), number(&raw[32..])].concat();
    let mut der = vec![0x30, body.len() as u8];
    der.extend(body);
    std::fs::write(case.home.join("signature.der"), der).unwrap();
    std::fs::write(case.home.join("signed.txt"), format!("{}.{}", parts[0], parts[1])).unwrap();
    let public = Command::new("openssl").args(["ec", "-in"]).arg(&key).arg("-pubout").arg("-out").arg(case.home.join("public.pem"))
        .output().unwrap();
    assert!(public.status.success());
    let checked = Command::new("openssl").args(["dgst", "-sha256", "-verify"]).arg(case.home.join("public.pem")).arg("-signature")
        .arg(case.home.join("signature.der")).arg(case.home.join("signed.txt")).output().unwrap();
    assert!(String::from_utf8_lossy(&checked.stdout).contains("Verified OK"));
}

#[test]
fn the_checks_of_this_project_start_no_such_process() {
    let case = Case::new();
    // CONVEYOR_WINDOW is set here, as in every check of this project.
    let done = Command::new(root().join("hitl")).args(["_hitl", "panel"]).stdin(Stdio::null()).output().unwrap();
    assert!(done.status.success());
    std::thread::sleep(Duration::from_millis(300));
    assert!(!case.home.join("phone").exists());
}

// ----------------------------------------------------------------------------------------------------------------------
// The way outside home

/// A made-up server on this Mac's loopback: one room, kept the way `relay/worker.js` keeps it. It lets in who says its
/// word, hands what one side says to the other as it was said, and tells each whether the other is in. What the phone
/// says while the Mac is away it holds and hands to the Mac when it comes, saying to the phone of each frame that it held
/// it. What it carried is kept, to see that it could read none of it.
struct Server {
    port: u16,
    room: Arc<Room>,
}

type Side = Arc<Mutex<TcpStream>>;

struct Room {
    word: Mutex<Box<dyn Fn() -> String + Send>>,
    sides: Mutex<HashMap<String, Side>>,
    resting: AtomicBool,
    carried: Mutex<Vec<Vec<u8>>>,
    held: Mutex<Vec<Vec<u8>>>,
    closed: AtomicBool,
}

fn frame(link: &Side, opcode: u8, payload: &[u8]) {
    let size = payload.len();
    let mut head = vec![0x80 | opcode];
    if size < 126 {
        head.push(size as u8);
    } else if size < 65536 {
        head.push(126);
        head.extend_from_slice(&(size as u16).to_be_bytes());
    } else {
        head.push(127);
        head.extend_from_slice(&(size as u64).to_be_bytes());
    }
    head.extend_from_slice(payload);
    let _ = link.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).write_all(&head);
}

fn tell(link: &Side, word: &Value) {
    frame(link, 1, hitl_host_core::dumps_line(word).as_bytes());
}

fn take(link: &mut TcpStream, heard: &mut Vec<u8>, size: usize) -> std::io::Result<Vec<u8>> {
    let mut buffer = vec![0u8; 1 << 16];
    while heard.len() < size {
        let count = link.read(&mut buffer)?;
        if count == 0 {
            return Err(std::io::Error::new(ErrorKind::UnexpectedEof, "gone"));
        }
        heard.extend_from_slice(&buffer[..count]);
    }
    Ok(heard.drain(..size).collect())
}

impl Server {
    fn new(word: &str) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let word = word.to_string();
        let room = Arc::new(Room { word: Mutex::new(Box::new(move || word.clone())), sides: Mutex::new(HashMap::new()),
                                   resting: AtomicBool::new(false), carried: Mutex::new(Vec::new()), held: Mutex::new(Vec::new()),
                                   closed: AtomicBool::new(false) });
        let serving = Arc::clone(&room);
        std::thread::spawn(move || {
            for link in listener.incoming() {
                if serving.closed.load(Ordering::SeqCst) {
                    return;
                }
                let Ok(link) = link else { return };
                let room = Arc::clone(&serving);
                std::thread::spawn(move || room.serve(link));
            }
        });
        Server { port, room }
    }

    fn address(&self) -> String {
        format!("ws://127.0.0.1:{}", self.port)
    }

    fn carried(&self) -> Vec<Vec<u8>> {
        self.room.carried.lock().unwrap().clone()
    }

    fn held(&self) -> Vec<Vec<u8>> {
        self.room.held.lock().unwrap().clone()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.room.closed.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port)); // the accepting thread wakes and ends
        for side in self.room.sides.lock().unwrap().values() {
            let _ = side.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).shutdown(Shutdown::Both);
        }
    }
}

impl Room {
    fn serve(self: Arc<Room>, mut link: TcpStream) {
        let mut head = Vec::new();
        let mut buffer = [0u8; 4096];
        let split = loop {
            if let Some(at) = head.windows(4).position(|window| window == b"\r\n\r\n") {
                break at;
            }
            match link.read(&mut buffer) {
                Ok(count) if count > 0 => head.extend_from_slice(&buffer[..count]),
                _ => return,
            }
        };
        let mut heard = head[split + 4..].to_vec();
        let text: String = head[..split].iter().map(|&byte| byte as char).collect();
        let lines: Vec<&str> = text.split("\r\n").collect();
        let said: HashMap<String, String> = lines[1..].iter().map(|line| {
            let (name, value) = line.split_once(':').unwrap_or((line, ""));
            (name.trim().to_lowercase(), value.trim().to_string())
        }).collect();
        let path = lines[0].split_whitespace().nth(1).unwrap_or("");
        let role = path.split('?').next().unwrap_or("").split('/').nth(3).unwrap_or("").to_string();
        let word = (self.word.lock().unwrap())();
        if said.get("authorization") != Some(&format!("Bearer {word}")) {
            let _ = link.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            return;
        }
        if said.get("upgrade").map(|value| value.to_lowercase()) != Some("websocket".into()) {
            // A look from outside: who is in the room. Nobody's place is taken by the asking.
            let here = {
                let sides = self.sides.lock().unwrap();
                let state = |side: &str| if sides.contains_key(side) { "here" } else if side == "mac" && self.resting.load(Ordering::SeqCst) {
                    "resting" } else { "gone" };
                json!({"mac": state("mac"), "phone": state("phone")})
            };
            let body = hitl_host_core::dumps_line(&here);
            let _ = link.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                                           body.len()).as_bytes());
            return;
        }
        let key = said.get("sec-websocket-key").cloned().unwrap_or_default();
        let accept = codec::base64(&codec::sha1(format!("{key}{}", relay::GUID).as_bytes()));
        let _ = link.write_all(format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
                                        Sec-WebSocket-Accept: {accept}\r\n\r\n").as_bytes());
        let other = if role == "mac" { "phone" } else { "mac" }.to_string();
        let me: Side = Arc::new(Mutex::new(link.try_clone().unwrap()));
        let (earlier, peer, state) = {
            let mut sides = self.sides.lock().unwrap();
            let earlier = sides.insert(role.clone(), Arc::clone(&me));
            if role == "mac" {
                self.resting.store(false, Ordering::SeqCst);
            }
            let peer = sides.get(&other).cloned();
            let state = if peer.is_some() { "here" } else if other == "mac" && self.resting.load(Ordering::SeqCst) { "resting" } else { "gone" };
            (earlier, peer, state)
        };
        if let Some(earlier) = earlier {
            frame(&earlier, 8, &4000u16.to_be_bytes());
        }
        let mut first = Map::new();
        first.insert(other.clone(), json!(state));
        if role == "phone" {
            first.insert("holds".into(), json!(true));
        }
        tell(&me, &Value::Object(first));
        if role == "mac" {
            let held = std::mem::take(&mut *self.held.lock().unwrap());
            for payload in held {
                frame(&me, 2, &payload);
            }
        }
        if let Some(peer) = &peer {
            tell(peer, &json!({role.clone(): "here"}));
        }
        let _ = (|| -> std::io::Result<()> {
            loop {
                let start = take(&mut link, &mut heard, 2)?;
                let mut size = usize::from(start[1] & 0x7F);
                if size == 126 {
                    let bytes = take(&mut link, &mut heard, 2)?;
                    size = usize::from(u16::from_be_bytes([bytes[0], bytes[1]]));
                } else if size == 127 {
                    let bytes = take(&mut link, &mut heard, 8)?;
                    size = u64::from_be_bytes(bytes.try_into().unwrap()) as usize;
                }
                let mask = take(&mut link, &mut heard, 4)?;
                let payload = relay::mixed(&take(&mut link, &mut heard, size)?, &mask.repeat(size / 4 + 1));
                match start[0] & 0x0F {
                    8 => return Ok(()),
                    1 => {
                        if payload == b"ping" {
                            frame(&me, 1, b"pong");
                        } else if role == "mac" && payload == b"resting" {
                            self.resting.store(true, Ordering::SeqCst);
                        }
                    }
                    _ => {
                        let peer = {
                            let sides = self.sides.lock().unwrap();
                            self.carried.lock().unwrap().push(payload.clone());
                            let peer = sides.get(&other).cloned();
                            if peer.is_none() && role == "phone" {
                                self.held.lock().unwrap().push(payload.clone());
                            }
                            peer
                        };
                        match peer {
                            Some(peer) => frame(&peer, 2, &payload),
                            None if role == "phone" => tell(&me, &json!({"held": 1})),
                            None => {}
                        }
                    }
                }
            }
        })();
        let (mine, peer, state) = {
            let mut sides = self.sides.lock().unwrap();
            let mine = sides.get(&role).is_some_and(|side| Arc::ptr_eq(side, &me));
            if mine {
                sides.remove(&role);
            }
            let state = if role == "mac" && self.resting.load(Ordering::SeqCst) { "resting" } else { "gone" };
            (mine, sides.get(&other).cloned(), state)
        };
        if let (true, Some(peer)) = (mine, peer) {
            tell(&peer, &json!({role: state}));
        }
        let _ = link.shutdown(Shutdown::Both);
    }
}

/// A made-up phone that is not at home: it comes to the server's room, and seals what it says there.
struct FarCaller {
    wire: Wire,
    seal: Seal,
    words: Vec<Value>,
    rooms: Vec<Value>,
}

impl FarCaller {
    fn new(far: &Value, key: Option<&str>) -> FarCaller {
        let address = format!("{}/room/{}/phone", far["url"].as_str().unwrap(), far["room"].as_str().unwrap());
        let authorization = format!("Bearer {}", far["token"].as_str().unwrap());
        let wire = Wire::open(&address, &[("Authorization", &authorization)]).expect("the room");
        let key = codec::unhex(key.unwrap_or_else(|| far["key"].as_str().unwrap())).unwrap();
        FarCaller { wire, seal: Seal::new(&key, relay::PHONE, relay::MAC, None).unwrap(), words: Vec::new(), rooms: Vec::new() }
    }

    fn say(&mut self, word: &Value) {
        self.say_at(word, hitl_host_core::now());
    }

    fn say_at(&mut self, word: &Value, moment: f64) {
        for frame in self.seal.frames_at(word.to_string().as_bytes(), moment) {
            self.wire.send(&frame).expect("sent");
        }
    }

    fn listen(&mut self, seconds: f64) {
        match self.wire.receive(seconds) {
            Ok(Some(Message::Text(text))) if text != b"pong" => self.rooms.push(serde_json::from_slice(&text).unwrap()),
            Ok(Some(Message::Binary(payload))) => {
                if let Some(line) = self.seal.line(&payload) {
                    self.words.push(serde_json::from_slice(&line).unwrap());
                }
            }
            _ => {}
        }
    }

    fn hear(&mut self, wanted: impl Fn(&Value) -> bool, seconds: f64) -> Option<Value> {
        let until = Instant::now() + Duration::from_secs_f64(seconds);
        while Instant::now() < until {
            if let Some(at) = self.words.iter().position(&wanted) {
                let found = self.words.remove(at);
                self.words.drain(..at);
                return Some(found);
            }
            self.words.clear();
            self.listen(0.1);
        }
        None
    }

    /// The first word of the server itself that is the wanted one: who is in the room.
    fn room(&mut self, wanted: impl Fn(&Value) -> bool, seconds: f64) -> Option<Value> {
        let until = Instant::now() + Duration::from_secs_f64(seconds);
        while Instant::now() < until {
            if let Some(at) = self.rooms.iter().position(&wanted) {
                let found = self.rooms.remove(at);
                self.rooms.drain(..at);
                return Some(found);
            }
            self.rooms.clear();
            self.listen(0.1);
        }
        None
    }

    fn queue(&mut self, when: impl Fn(&Value) -> bool) -> Option<Value> {
        self.hear(|word| word.get("queue").is_some_and(&when), 8.0).map(|word| word["queue"].clone())
    }
}

/// The way outside home: the Mac's process and a phone meet in a server's room.
struct Far {
    case: Case,
    server: Server,
    far: Value,
}

impl Far {
    fn new() -> Far {
        let case = Case::bridge();
        let server = Server::new("the-servers-word");
        let far = json!({"url": server.address(), "room": "5f".repeat(16), "token": "the-servers-word", "key": "a7".repeat(32)});
        write(&case.home.join("phone/relay.json"), &far);
        Far { case, server, far }
    }

    fn come(&self) -> FarCaller {
        FarCaller::new(&self.far, None)
    }

    fn hello(&self, more: Value) -> Value {
        let mut hello = json!({"token": self.case.word["token"]});
        for (key, value) in more.as_object().expect("an object") {
            hello[key] = value.clone();
        }
        json!({"hello": hello})
    }
}

fn mac_here(word: &Value) -> bool {
    word.get("mac") == Some(&json!("here"))
}

fn of_mac(word: &Value) -> bool {
    word.get("mac").is_some()
}

#[test]
fn a_phone_that_is_not_at_home_is_shown_the_queue_and_its_press_is_the_windows() {
    let mut far = Far::new();
    let first = far.case.queue(&["--title", "Вход", "--check", "1. Плагин входит"]);
    far.case.window_says(json!({"active": first, "order": [first], "marks": [{"label": "1", "note": ""}]}));
    far.case.serve();
    let mut caller = far.come();
    assert!(caller.room(mac_here, 8.0).is_some()); // with "holds" if it came first
    caller.say(&far.hello(json!({"device": "iPhone"})));
    assert_eq!(caller.hear(|word| word.get("welcome").is_some(), 8.0).expect("welcome")["welcome"]["mac"], text("mac"));
    let seen = caller.queue(any).expect("the queue");
    assert_eq!((&seen["active"], &seen["checks"][0]["items"][0]["text"]), (&json!(first), &text("Плагин входит")));
    caller.say(&json!({"do": "answer", "ticket": first, "label": "1", "number": 0}));
    assert_eq!(far.case.wait_presses(1, 8), vec![json!({"do": "answer", "ticket": first, "label": "1", "number": 0})]);
    // What the window answers to that comes to the phone by itself, the same way.
    far.case.window_says(json!({"seq": 2, "active": first, "order": [first], "marks": [{"label": "1", "state": "ok", "note": ""}],
                                "canFinish": true}));
    assert!(caller.queue(|seen| seen["canFinish"] == json!(true)).is_some());
    // And the server carried it all without a word of it in the open.
    let carried = far.server.carried().concat();
    for word in ["Плагин входит", "Вход", &first, far.case.word["token"].as_str().unwrap(), "answer"] {
        assert!(!carried.windows(word.len()).any(|window| window == word.as_bytes()), "{word}");
    }
}

#[test]
fn only_who_has_the_key_is_heard_in_the_room_and_a_word_is_heard_once() {
    let mut far = Far::new();
    let first = far.case.queue(&["--title", "Вход", "--check", "1. Раз"]);
    far.case.window_says(json!({"active": first, "order": [first], "marks": [{"label": "1", "note": ""}]}));
    far.case.serve();
    // Somebody who knows the room and the server's word, and not the key that seals.
    let zeros = "00".repeat(32);
    let mut stranger = FarCaller::new(&far.far, Some(&zeros));
    stranger.room(mac_here, 8.0);
    stranger.say(&far.hello(json!({})));
    stranger.say(&json!({"do": "finish", "ticket": first, "how": "rework"}));
    assert_eq!(stranger.hear(any, 1.5), None);
    assert_eq!(far.case.presses(), Vec::<Value>::new());
    stranger.wire.close();
    // The phone itself; and what it said once, said again by whoever kept it, is not a second press.
    let mut caller = far.come();
    caller.room(mac_here, 8.0);
    caller.say(&far.hello(json!({})));
    assert!(caller.queue(any).is_some());
    let frames = caller.seal.frames(json!({"do": "answer", "ticket": first, "label": "1", "number": 0}).to_string().as_bytes());
    for frame in frames.iter().chain(frames.iter()) {
        caller.wire.send(frame).unwrap();
    }
    caller.say(&json!({"do": "note", "ticket": first, "label": "1", "text": "раз"}));
    far.case.wait_presses(2, 8);
    std::thread::sleep(Duration::from_millis(300));
    let done: Vec<Value> = far.case.presses().iter().map(|press| press["do"].clone()).collect();
    assert_eq!(done, vec![text("answer"), text("note")]);
}

#[test]
fn a_file_too_large_for_one_frame_comes_whole() {
    let mut far = Far::new();
    let picture = far.case.home.join("screen.png");
    std::fs::write(&picture, [png(), codec::random(400_000)].concat()).unwrap();
    let first = far.case.queue(&["--title", "Вход", "--check", "1. Раз", "--image", &picture.display().to_string()]);
    far.case.window_says(json!({"active": first, "order": [first], "marks": [{"label": "1", "note": ""}]}));
    far.case.serve();
    let mut caller = far.come();
    caller.room(mac_here, 8.0);
    caller.say(&far.hello(json!({})));
    caller.queue(any).expect("the queue");
    caller.say(&json!({"get": picture.canonicalize().unwrap().display().to_string(), "id": 7}));
    let got = caller.hear(|word| word["file"] == json!(7), 15.0).expect("the picture");
    assert_eq!((&got["name"], codec::unbase64(got["data"].as_str().unwrap()).unwrap()), (&text("screen.png"), std::fs::read(&picture).unwrap()));
    assert!(far.server.carried().len() > 5); // said in parts
    caller.say(&json!({"get": far.case.home.join("phone/relay.json").display().to_string(), "id": 8}));
    assert_eq!(caller.hear(|word| word["file"] == json!(8), 8.0), Some(json!({"file": 8, "error": "этого файла нет в очереди"})));
}

#[test]
fn the_phone_is_told_whether_the_mac_rests_or_is_gone() {
    let mut far = Far::new();
    let mut caller = far.come();
    assert_eq!(caller.room(of_mac, 8.0), Some(json!({"mac": "gone", "holds": true})));
    let first = far.case.queue(&["--title", "Вход", "--check", "1. Раз"]);
    far.case.window_says(json!({"active": first, "order": [first], "marks": [{"label": "1", "note": ""}]}));
    far.case.set("CONVEYOR_PHONE_IDLE_EXIT", Some("0.3"));
    far.case.serve();
    assert_eq!(caller.room(of_mac, 8.0), Some(json!({"mac": "here"})));
    // Killed with a check waiting: it is gone, and nobody said why.
    far.case.kill();
    assert_eq!(caller.room(of_mac, 8.0), Some(json!({"mac": "gone"})));
    // Leaving by itself with nothing in the queue: it says so first.
    for entry in std::fs::read_dir(far.case.home.join("queue")).unwrap().flatten() {
        std::fs::remove_file(entry.path()).unwrap();
    }
    far.case.serve();
    assert_eq!(caller.room(of_mac, 8.0), Some(json!({"mac": "here"})));
    assert_eq!(far.case.ended(10), Some(0), "{}", far.case.log());
    assert_eq!(caller.room(of_mac, 8.0), Some(json!({"mac": "resting"})));
}

#[test]
fn what_the_phone_said_while_the_mac_was_away_is_held_and_comes_to_the_window_as_kept() {
    // His pick of 2026-10-16: with no way to the Mac, a press is kept on the server if it is there to reach.
    let mut far = Far::new();
    let first = far.case.queue(&["--title", "Вход", "--check", "1. Плагин входит"]);
    far.case.window_says(json!({"active": first, "order": [first], "marks": [{"label": "1", "note": ""}]}));
    let mut caller = far.come();
    assert_eq!(caller.room(of_mac, 8.0), Some(json!({"mac": "gone", "holds": true})));
    let an_hour_ago = hitl_host_core::now() - 3600.0; // pressed while the Mac slept: past the ten minutes a frame was taken for
    caller.say_at(&far.hello(json!({"front": false})), an_hour_ago);
    caller.say_at(&json!({"do": "note", "ticket": first, "label": "1", "text": "без связи", "late": true}), an_hour_ago);
    caller.say_at(&json!({"do": "answer", "ticket": first, "label": "1", "number": 0, "late": true}), an_hour_ago);
    let held: Vec<Option<Value>> = (0..3).map(|_| caller.room(|word| word.get("held").is_some(), 8.0)).collect();
    assert_eq!(held, vec![Some(json!({"held": 1})); 3]);
    assert_eq!(far.case.presses(), Vec::<Value>::new());
    caller.wire.close();
    far.case.serve();
    assert_eq!(far.case.wait_presses(2, 8), vec![json!({"do": "note", "ticket": first, "label": "1", "text": "без связи", "late": true}),
                                                 json!({"do": "answer", "ticket": first, "label": "1", "number": 0, "late": true})]);
    assert_eq!(far.server.held(), Vec::<Vec<u8>>::new());
    // A day and more is too late: held, and not taken by the Mac.
    let mut stale = far.come();
    stale.room(mac_here, 8.0);
    far.case.kill();
    assert_eq!(stale.room(of_mac, 8.0), Some(json!({"mac": "gone"})));
    let a_day_ago = hitl_host_core::now() - 25.0 * 3600.0;
    stale.say_at(&far.hello(json!({"front": false})), a_day_ago);
    stale.say_at(&json!({"do": "answer", "ticket": first, "label": "1", "number": 1, "late": true}), a_day_ago);
    let held: Vec<Option<Value>> = (0..2).map(|_| stale.room(|word| word.get("held").is_some(), 8.0)).collect();
    assert_eq!(held, vec![Some(json!({"held": 1})); 2]);
    stale.wire.close();
    far.case.serve();
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(far.case.presses().len(), 2);
}

#[test]
fn the_phone_learns_of_the_server_by_cable_and_at_home() {
    let mut far = Far::new();
    assert_eq!(word::phone_word().unwrap()["relay"], far.far);
    // What is kept of a word put on the phone has a print of the room and none of its keys.
    let kept = word::told_of(&word::phone_word().unwrap()).to_string();
    for secret in [&far.far["key"], &far.far["token"]] {
        assert!(!kept.contains(secret.as_str().unwrap()));
    }
    let mut without = word::phone_word().unwrap();
    without["relay"] = Value::Null;
    let mut paired = json!({"at": 1});
    for (key, value) in word::told_of(&without).as_object().unwrap() {
        paired[key] = value.clone();
    }
    write(&far.case.home.join("phone/paired.json"), &paired);
    assert!(!word::is_paired()); // a phone that was told before the server was set up is to be told again
    let first = far.case.queue(&["--title", "Вход", "--check", "1. Раз"]);
    far.case.window_says(json!({"active": first, "order": [first], "marks": [{"label": "1", "note": ""}]}));
    far.case.serve();
    let mut at_home = far.case.call();
    assert_eq!(at_home.hear(|word| word.get("welcome").is_some(), 8.0).expect("welcome")["welcome"], json!({"mac": "mac", "relay": far.far}));
}

/// A tool of these tests that is the app's own code, `tests/NAME.swift` with files of the app's queue, built for this
/// Mac once per version of its source and kept with the window the tests build. None without a compiler.
fn app_tool(name: &str, of_the_app: &[&str]) -> Option<PathBuf> {
    let compiler = hitl_host_core::which("swiftc")?;
    let mut sources: Vec<PathBuf> = of_the_app.iter().map(|file| root().join("demo/ConveyorDemo/Queue").join(file)).collect();
    sources.push(root().join("tests").join(format!("{name}.swift")));
    let all: Vec<u8> = sources.iter().flat_map(|source| std::fs::read(source).expect("a source")).collect();
    let version = &codec::hex(&codec::sha256(&all))[..16];
    let kept = std::env::temp_dir().join("conveyor-tests");
    std::fs::create_dir_all(&kept).ok()?;
    let binary = kept.join(format!("{name}-{version}"));
    if !binary.exists() {
        let fresh = kept.join(format!("{name}-{version}.{}", std::process::id()));
        // As the app is built: Swift 6, and everything on the main actor unless it says otherwise.
        let built = Command::new(compiler).args(["-O", "-parse-as-library", "-swift-version", "6", "-default-isolation", "MainActor"])
            .args(&sources).arg("-o").arg(&fresh).output().expect("swiftc runs");
        assert!(built.status.success(), "the app's code for {name} does not build:\n{}", String::from_utf8_lossy(&built.stderr));
        std::fs::rename(&fresh, &binary).expect("kept");
    }
    Some(binary)
}

#[test]
fn the_app_reads_a_link_in_a_text_by_the_rule_the_command_reads_it_by() {
    // A chat writes [слова](адрес); `hitl check` counts the words, the window and the app make a link of them. The
    // three read one text the same way, or he would see brackets on one screen and a link on the other.
    let Some(binary) = app_tool("linked-words", &["QueueWords.swift", "RelayLink.swift", "RelaySeal.swift"]) else {
        eprintln!("no Swift compiler on this Mac: skipped");
        return;
    };
    let texts = ["Форма после [team/320](https://realize.day/t/team/320) закрывается рывком",
                 "См. [план работ](/Users/a/Мой план.md), [заметку](obsidian://open?path=a) и [PR](https://github.com/o/r/pull/1).",
                 "[Вики](https://ru.wikipedia.org/wiki/Конвейер_(значения)) в начале",
                 "массив[0] и [так] (не ссылка), [слова](не адрес), [](https://example.test)",
                 "Правка в **одном** месте, [ PR #412 ]( https://example.test/412 ) её не задевает",
                 "без ссылок"];
    let mut app = Command::new(&binary).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().expect("the app's code");
    app.stdin.take().unwrap().write_all((texts.join("\n") + "\n").as_bytes()).unwrap();
    let said = app.wait_with_output().unwrap();
    assert!(said.status.success());
    let read: Vec<Vec<(String, Option<String>)>> = String::from_utf8(said.stdout).unwrap().lines()
        .map(|line| serde_json::from_str(line).expect("pieces")).collect();
    assert_eq!(read.len(), texts.len());
    for (text, pieces) in texts.iter().zip(&read) {
        assert_eq!(pieces.iter().map(|(words, _)| words.as_str()).collect::<String>(), hitl::common::read_text(text), "{text}");
        let linked: Vec<(String, String)> = pieces.iter().filter_map(|(words, link)| Some((words.clone(), link.clone()?))).collect();
        let by_the_command: Vec<(String, String)> = hitl::common::text_links(text).into_iter()
            .filter(|(words, target)| !words.is_empty() && hitl::common::is_address(target)).collect();
        assert_eq!(linked, by_the_command, "{text}");
    }
    // a file's path with a space in it
    assert_eq!(read[1][1], ("план работ".to_string(), Some("/Users/a/Мой план.md".to_string())));
}

#[test]
fn the_apps_own_code_is_heard_through_the_server() {
    let Some(binary) = app_tool("far-phone", &["RelaySeal.swift", "RelayLink.swift"]) else {
        eprintln!("no Swift compiler on this Mac: skipped");
        return;
    };
    let mut far = Far::new();
    let picture = far.case.home.join("screen.png");
    std::fs::write(&picture, [png(), codec::random(300_000)].concat()).unwrap();
    let first = far.case.queue(&["--title", "Вход", "--check", "1. Плагин входит", "--image", &picture.display().to_string()]);
    far.case.window_says(json!({"active": first, "order": [first], "marks": [{"label": "1", "note": ""}]}));
    far.case.serve();
    let text_of = |key: &str| far.far[key].as_str().unwrap().to_string();
    let mut app = Command::new(&binary).args([text_of("url"), text_of("room"), text_of("token"), text_of("key")])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().expect("the far phone");
    let printed = Arc::new(Mutex::new(Vec::<Value>::new()));
    let output = app.stdout.take().unwrap();
    let reading = Arc::clone(&printed);
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(output).lines().map_while(Result::ok) {
            if let Ok(word) = serde_json::from_str(&line) {
                reading.lock().unwrap().push(word);
            }
        }
    });
    let heard = |wanted: &dyn Fn(&Value) -> bool, seconds: u64| -> Option<Value> {
        let until = Instant::now() + Duration::from_secs(seconds);
        while Instant::now() < until {
            let mut words = printed.lock().unwrap();
            if let Some(at) = words.iter().position(wanted) {
                let found = words.remove(at);
                words.drain(..at);
                return Some(found);
            }
            words.clear();
            drop(words);
            std::thread::sleep(Duration::from_millis(50));
        }
        None
    };
    let mut stdin = app.stdin.take().unwrap();
    let mut say = |word: Value| {
        stdin.write_all(format!("{word}\n").as_bytes()).unwrap();
        stdin.flush().unwrap();
    };
    assert_eq!(heard(&of_mac, 10), Some(json!({"mac": "here"})));
    say(far.hello(json!({"device": "айфон"})));
    assert_eq!(heard(&|word| word.get("welcome").is_some(), 10).expect("welcome")["welcome"]["mac"], text("mac"));
    assert_eq!(heard(&|word| word.get("queue").is_some(), 10).expect("the queue")["queue"]["checks"][0]["items"][0]["text"],
               text("Плагин входит"));
    say(json!({"do": "note", "ticket": first, "label": "1", "text": format!("не входит: {}", "ж".repeat(200_000))})); // longer than a frame
    say(json!({"do": "answer", "ticket": first, "label": "1", "number": 1}));
    say(json!({"get": picture.canonicalize().unwrap().display().to_string(), "id": 4}));
    let got = heard(&|word| word["file"] == json!(4), 20).expect("the picture");
    let bytes = std::fs::read(&picture).unwrap();
    assert_eq!((&got["name"], &got["bytes"], &got["print"]), (&text("screen.png"), &json!(bytes.len()), &json!(codec::hex(&codec::sha256(&bytes)))));
    let presses = far.case.wait_presses(2, 8);
    let _ = app.kill();
    let _ = app.wait();
    let done: Vec<Value> = presses.iter().map(|press| press["do"].clone()).collect();
    assert_eq!(done, vec![text("note"), text("answer")]);
    // It came whole, in parts; the window is handed as much of a comment as it keeps.
    assert!(presses[0]["text"].as_str().unwrap().starts_with("не входит: жжж"));
}

fn hitl(arguments: &[&str]) -> (i32, String) {
    let done = Command::new(root().join("hitl")).args(arguments).stdin(Stdio::null()).output().expect("hitl runs");
    (done.status.code().unwrap_or(-1), String::from_utf8_lossy(&done.stdout).to_string() + &String::from_utf8_lossy(&done.stderr))
}

#[test]
fn the_server_is_put_into_his_account_and_its_keys_are_kept_on_the_mac() {
    let mut far = Far::new();
    let relay_file = far.case.home.join("phone/relay.json");
    std::fs::remove_file(&relay_file).unwrap();
    // A made-up wrangler: it says the address it would put the server at, and keeps the word it is handed.
    let (tool, calls, word_file) = (far.case.home.join("wrangler.sh"), far.case.home.join("wrangler.calls"), far.case.home.join("wrangler.word"));
    std::fs::write(&tool, format!("#!/bin/sh\necho \"$*\" >> \"{}\"\ncase \"$1\" in\n\
                                   \x20 deploy) echo \"Uploaded conveyor-relay\"; echo \"  https://conveyor-relay.someone.workers.dev\";;\n\
                                   \x20 secret) cat > \"{}\";;\n\x20 delete) echo \"Successfully deleted conveyor-relay\";;\nesac\n",
                                  calls.display(), word_file.display())).unwrap();
    chmod(&tool, 0o755);
    let watched = word_file.clone();
    *far.server.room.word.lock().unwrap() = Box::new(move || std::fs::read_to_string(&watched).unwrap_or_else(|_| "нет".into()));
    let address = far.server.address();
    far.case.set("CONVEYOR_RELAY_TOOL", Some(&tool.display().to_string()));
    far.case.set("CONVEYOR_RELAY_URL", Some(&address));
    let (code, said) = hitl(&["phone", "relay", "up"]);
    assert_eq!(code, 0, "{said}");
    assert!(said.contains("пускает этот Мак"), "{said}");
    let kept = read(&relay_file);
    assert_eq!(kept["url"], json!(address));
    let token = std::fs::read_to_string(&word_file).unwrap();
    assert_eq!((kept["room"].as_str().unwrap().len(), codec::unhex(kept["key"].as_str().unwrap()).unwrap().len(), kept["token"].as_str().unwrap()),
               (32, 32, token.as_str()));
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(&relay_file).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(std::fs::read_to_string(&calls).unwrap().lines().collect::<Vec<_>>(),
               vec!["deploy --name conveyor-relay", "secret put KEY --name conveyor-relay"]);
    assert!(!said.contains(kept["token"].as_str().unwrap()) && !said.contains(kept["key"].as_str().unwrap()));
    // Put there again, it keeps the room and the keys the phone already knows.
    hitl(&["phone", "relay", "up"]);
    let again = read(&relay_file);
    assert_eq!((&again["room"], &again["key"], &again["token"]), (&kept["room"], &kept["key"], &kept["token"]));
    let (code, said) = hitl(&["phone", "relay"]);
    assert_eq!(code, 0);
    assert!(said.contains("отвечает") && !said.contains("не отвечает"), "{said}");
    // A look takes nobody's place: a Mac that is in the room stays in it.
    let inside = Wire::open(&format!("{}/room/{}/mac", address, kept["room"].as_str().unwrap()),
                            &[("Authorization", &format!("Bearer {}", kept["token"].as_str().unwrap()))]).unwrap();
    assert_eq!(inside.receive(3.0).unwrap(), Some(Message::Text(br#"{"phone": "gone"}"#.to_vec())));
    assert_eq!(hitl_phone::commands::relay_answers(&kept), (true, String::new()));
    assert_eq!(inside.receive(0.5).unwrap(), None);
    let mut wrong = kept.clone();
    wrong["token"] = text("не то слово");
    assert!(!hitl_phone::commands::relay_answers(&wrong).0);
    let (code, _) = hitl(&["phone", "relay", "down"]);
    assert_eq!(code, 0);
    assert!(!relay_file.exists());
    assert_eq!(word::relay_word(), None);
}

// ----------------------------------------------------------------------------------------------------------------------
// The Mac's name

#[test]
fn forty_looks_at_the_queue_ask_the_system_for_the_name_once() {
    // The audit of 2026-10-16, O02: 40 snapshots started `scutil` 40 times.
    let mut case = Case::new();
    let calls = case.home.join("calls");
    let bin = case.home.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join("scutil"), format!("#!/bin/sh\necho \"$@\" >> '{}'\necho studio\n", calls.display())).unwrap();
    chmod(&bin.join("scutil"), 0o755);
    case.set("CONVEYOR_PHONE_NAME", None);
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
    case.set("PATH", Some(&path));
    word::forget_mac_name();
    let count = || std::fs::read_to_string(&calls).map(|text| text.lines().count()).unwrap_or(0);
    let said: Vec<Value> = (0..40).map(|_| shown::snapshot().0["mac"].clone()).collect();
    assert!(said.iter().all(|name| *name == text("studio")));
    assert_eq!(count(), 1);
    // a minute on, the name is asked again: he may have renamed the Mac
    word::mac_name_at(Instant::now() + Duration::from_secs(61), false);
    assert_eq!(count(), 2);
    word::mac_name(true); // what is put on the phone by cable is always asked anew
    assert_eq!(count(), 3);
    word::forget_mac_name();
}

// ----------------------------------------------------------------------------------------------------------------------
// The chat's name, as the window says it

/// A chat of the Claude app in a project, the name hitl keeps for it, and a check it queued that still says the
/// chat's old name.
fn named_chat(case: &mut Case) -> Value {
    let folder = case.home.join("project");
    std::fs::create_dir_all(&folder).unwrap();
    for (name, place) in [("CONVEYOR_CLAUDE_SESSIONS_DIR", "desktop"), ("CONVEYOR_CLAUDE_PROJECTS_DIR", "projects"),
                          ("CONVEYOR_CODEX_STATE", "absent.sqlite"), ("CONVEYOR_OPENCODE_STATE", "absent-oc.sqlite")] {
        let value = case.home.join(place).display().to_string();
        case.set(name, Some(&value));
    }
    let folder_text = folder.display().to_string();
    write(&case.home.join("desktop/a/b/owner.json"),
          &json!({"sessionId": "owner-chat", "cliSessionId": "current-alias", "title": "native", "cwd": folder_text}));
    let key = codec::hex(&codec::sha256(br#"["Claude", "owner-chat"]"#));
    write(&case.home.join("chats/names").join(format!("{key}.json")),
          &json!({"version": 1, "app": "Claude", "id": "owner-chat", "mode": "manual", "revision": 1, "generation": 0, "title": "roadmap",
                  "decisions": {}}));
    json!({"id": "fixture-check", "agent": "Claude", "chat": "old label", "chatIds": ["owner-chat"], "cwd": folder_text,
           "project": folder_text, "createdAt": 1, "tag": "", "title": "Fixture", "items": []})
}

#[test]
fn the_phone_names_a_chat_as_the_window_does_and_the_check_stays_as_it_was() {
    let mut case = Case::new();
    let ticket = named_chat(&mut case);
    let before = ticket.to_string();
    let seen = shown::shown(&ticket);
    assert_eq!(seen["chat"], text("roadmap"));
    assert_eq!(ticket.to_string(), before);
    // A check of a chat queued from another folder keeps the name it came with.
    let mut foreign = ticket.clone();
    foreign["cwd"] = text("/foreign");
    assert_eq!(shown::shown(&foreign)["chat"], text("old label"));
    assert_eq!(seen["items"][0]["opens"], Value::Null); // a check of no items offers nothing
    let mut one = ticket.clone();
    one["items"] = json!([{"label": "1", "text": "Раз"}]);
    // The chat opens in its own app: a chat of Claude without an address of the app's has nothing to open, and
    // never the `conveyor://` of the app that is gone.
    assert_eq!(shown::shown(&one)["items"][0]["opens"], json!([]));
    // The setting of the button says which addresses need a command of the Mac; the phone names no program.
    let setting = case.home.join("chat-button.json");
    std::fs::write(&setting, r#"{"open": {"my-chat": ["/bin/true", "{host}", "{path}"]}}"#).unwrap();
    case.set("CONVEYOR_CHAT_BUTTON", Some(&setting.display().to_string()));
    one["chatLink"] = text("my-chat://Some/abc");
    assert_eq!(shown::shown(&one)["items"][0]["opens"], json!([])); // a chat in a terminal is on the Mac, not on the phone
    case.set("CONVEYOR_CHAT_BUTTON", Some("/nonexistent/chat-button.json"));
    assert_eq!(shown::shown(&one)["items"][0]["opens"], json!([{"title": "Чат", "link": "my-chat://Some/abc", "kind": "app"}])); // with no setting, an address as any
    one["chatLink"] = text("claude://code/session_owner");
    assert_eq!(shown::shown(&one)["items"][0]["opens"], json!([{"title": "Чат", "link": "claude://code/session_owner", "kind": "app"}]));
}

// ----------------------------------------------------------------------------------------------------------------------
// The window itself, with presses of the phone laid out where it takes them from

const PAGE: &str = r#"<!doctype html><meta charset="utf-8"><body><script>hitl.set({ширина: 520, поля: 14})</script>страница</body>"#;

/// The window's program, built once for every test here and kept where other suites keep theirs; one suite builds,
/// the others wait for what it built.
fn window_program(case: &mut Case) -> PathBuf {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT.get_or_init(|| {
        let kept = std::env::temp_dir().join("conveyor-tests");
        std::fs::create_dir_all(&kept).expect("a folder");
        let lock = std::fs::File::create(kept.join("build.lock")).expect("a lock");
        unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&lock), libc::LOCK_EX) };
        let own = std::env::var("CONVEYOR_HOME").ok();
        let own_hitl = std::env::var("HITL_HOME").ok();
        case.set("CONVEYOR_HOME", Some(&kept.display().to_string()));
        case.set("HITL_HOME", Some(&kept.display().to_string()));
        let mut sources: Vec<PathBuf> = std::fs::read_dir(root().join("window")).expect("the window's sources").flatten()
            .map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|extension| extension == "swift")).collect();
        sources.sort();
        let built = hitl::window::built_panel(&sources, false);
        case.set("CONVEYOR_HOME", own.as_deref());
        case.set("HITL_HOME", own_hitl.as_deref());
        built.unwrap_or_else(|said| panic!("the window does not build: {said}"))
    }).clone()
}

impl Case {
    /// One press of the phone, as `hitl phone` leaves it for the window.
    fn press(&self, word: Value) {
        let inbox = self.home.join("window/remote/in");
        std::fs::create_dir_all(&inbox).unwrap();
        let count = std::fs::read_dir(&inbox).map(|entries| entries.count()).unwrap_or(0);
        std::fs::write(inbox.join(format!("{:020}.json", count + 1)), word.to_string()).unwrap();
    }

    /// The window run through a script of steps, off the screen.
    fn play(&mut self, steps: &[String]) {
        let program = window_program(self);
        let script = self.home.join("script.txt");
        std::fs::write(&script, steps.join("\n") + "\n").unwrap();
        let mut window = Command::new(program).env("CONVEYOR_HOME", &self.home).env("CONVEYOR_WINDOW_SCRIPT", &script)
            .stdout(Stdio::null()).stderr(Stdio::null()).spawn().expect("the window starts");
        let until = Instant::now() + Duration::from_secs(60);
        let status = loop {
            if let Some(status) = window.try_wait().unwrap() {
                break status;
            }
            if Instant::now() > until {
                let _ = window.kill();
                panic!("the window did not end its script in a minute");
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(status.success(), "the window's script failed: {status}");
    }

    /// What the window dumped, held to never having taken the keyboard or come forward.
    fn seen(&self, name: &str) -> Value {
        let seen = read(&self.home.join(name));
        assert_eq!(seen["hasKeyboard"], json!(false), "a scripted window must never hold the keyboard");
        assert_eq!(seen["timesForward"], json!(0), "a scripted window must never be made the active app, even for a moment");
        seen
    }

    fn verdict(&self, id: &str) -> Value {
        read(&self.home.join("done").join(format!("{id}.json")))
    }
}

fn steps(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

fn marks(verdict: &Value, keys: &[&str]) -> Vec<Vec<Value>> {
    verdict["results"].as_array().unwrap().iter()
        .map(|mark| keys.iter().map(|key| mark.get(*key).cloned().unwrap_or(Value::Null)).collect()).collect()
}

#[test]
fn an_answer_on_the_phone_is_the_answer_of_the_window() {
    let mut case = Case::new();
    let id = case.queue(&["--title", "Вход", "--kind", "result", "--check", "1. Раз", "--check", "2. Два", "--check", "3. Три"]);
    case.press(json!({"do": "answer", "ticket": id, "label": "1", "number": 0}));
    case.press(json!({"do": "answer", "ticket": id, "label": "2", "number": 1}));
    case.press(json!({"do": "note", "ticket": id, "label": "2", "text": "кнопка мелковата"}));
    case.press(json!({"do": "answer", "ticket": id, "label": "3", "number": 2}));
    case.press(json!({"do": "finish", "ticket": id, "how": "done"}));
    let at = case.home.display().to_string();
    case.play(&steps(&["remote 1", &format!("dump {at}/one.json"), "remote 1", &format!("dump {at}/two.json"), "remote 1",
                       &format!("dump {at}/noted.json"), "remote 1", "remote", &format!("dump {at}/after.json")]));
    assert_eq!(case.seen("one.json")["index"], json!(1)); // a yes moves on, as by its digit
    let two = case.seen("two.json"); // a no stays for its comment, which is written on the phone: nothing is edited here
    assert_eq!((&two["index"], &two["editing"], &two["marks"][1]["state"]), (&json!(1), &json!(false), &json!("problem")));
    assert_eq!(case.seen("noted.json")["note"], json!("кнопка мелковата"));
    let verdict = case.verdict(&id);
    assert_eq!((&verdict["how"], &verdict["endedOn"]), (&json!("done"), &json!("phone")));
    assert_eq!(marks(&verdict, &["label", "state", "note"]), vec![vec![json!("1"), json!("ok"), json!("")],
                                                                  vec![json!("2"), json!("problem"), json!("кнопка мелковата")],
                                                                  vec![json!("3"), json!("skipped"), json!("")]]);
    let text = hitl::verdict::verdict_text(&verdict, false);
    assert!(text.contains("Как закончена: кнопкой «Готово» в приложении «Конвейер» на телефоне."), "{text}");
    assert!(text.contains("Не ок: 2"), "{text}");
    assert!(!case.ticket(&id).exists());
}

#[test]
fn every_yes_on_the_phone_is_his_ok_for_the_commit() {
    let mut case = Case::new();
    let id = case.queue(&["--title", "Вход", "--kind", "result", "--check", "1. Раз", "--check", "2. Два"]);
    let asked = read(&case.ticket(&id));
    case.press(json!({"do": "answer", "ticket": id, "label": "1", "number": 0}));
    case.press(json!({"do": "finish", "ticket": id, "how": "done"})); // not every item has its answer yet: nothing ends
    case.press(json!({"do": "answer", "ticket": id, "label": "2", "number": 0}));
    case.press(json!({"do": "finish", "ticket": id, "how": "done"}));
    let at = case.home.display().to_string();
    case.play(&steps(&["remote 2", &format!("dump {at}/early.json"), "remote"]));
    assert_eq!(case.seen("early.json")["id"], json!(id));
    let verdict = case.verdict(&id);
    assert!(hitl::verdict::is_green(&verdict));
    assert_eq!((&verdict["commit"], &verdict["branch"]), (&asked["commit"], &asked["branch"]));
    let history = std::fs::read_to_string(case.home.join("history.jsonl")).unwrap();
    let lines: Vec<Value> = history.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(lines.len(), 1);
    assert_eq!((&lines[0]["id"], &lines[0]["endedOn"]), (&json!(id), &json!("phone")));
}

#[test]
fn options_ticks_and_his_own_answer() {
    let mut case = Case::new();
    let id = case.queue(&["--title", "Вопросы", "--kind", "question", "--check", "1. Где?", "--option", "Свой", "--option", "В Realize",
                          "--check", "2. Что берём?", "--option", "Раз", "--option", "Два", "--option", "Три", "--many",
                          "--check", "3. А это?", "--option", "Так", "--option", "Иначе"]);
    case.press(json!({"do": "answer", "ticket": id, "label": "1", "number": 1}));
    case.press(json!({"do": "answer", "ticket": id, "label": "2", "number": 0}));
    case.press(json!({"do": "answer", "ticket": id, "label": "2", "number": 2}));
    case.press(json!({"do": "answer", "ticket": id, "label": "2", "number": 0})); // the tick comes off
    case.press(json!({"do": "answer", "ticket": id, "label": "3", "number": -1}));
    case.press(json!({"do": "note", "ticket": id, "label": "3", "text": "ни то ни другое"}));
    case.press(json!({"do": "finish", "ticket": id, "how": "done"}));
    let at = case.home.display().to_string();
    case.play(&steps(&["remote 4", &format!("dump {at}/ticks.json"), "remote"]));
    let ticks = case.seen("ticks.json");
    assert_eq!((&ticks["index"], &ticks["ticked"]), (&json!(1), &json!(["Три"]))); // where several may hold, he goes on himself
    let verdict = case.verdict(&id);
    assert_eq!(marks(&verdict, &["state", "choice", "choices"]), vec![vec![json!("ok"), json!("В Realize"), Value::Null],
                                                                     vec![json!("ok"), Value::Null, json!(["Три"])],
                                                                     vec![json!("problem"), Value::Null, Value::Null]]);
    let text = hitl::verdict::verdict_text(&verdict, false);
    assert!(text.contains("Свой ответ, не из вариантов: 3"), "{text}");
}

#[test]
fn he_did_not_understand_an_item_on_the_phone() {
    let mut case = Case::new();
    let id = case.queue(&["--title", "План", "--kind", "plan", "--check", "1. Раз", "--check", "2. Два"]);
    case.press(json!({"do": "answer", "ticket": id, "label": "1", "number": -2}));
    case.press(json!({"do": "note", "ticket": id, "label": "1", "text": "что за раз"}));
    case.press(json!({"do": "answer", "ticket": id, "label": "2", "number": 0}));
    case.press(json!({"do": "finish", "ticket": id, "how": "done"}));
    let at = case.home.display().to_string();
    case.play(&steps(&["remote 1", &format!("dump {at}/unclear.json"), "remote"]));
    let unclear = case.seen("unclear.json"); // as a "no", the item stays for its comment, and nothing is edited on the Mac
    assert_eq!((&unclear["index"], &unclear["editing"], &unclear["marks"][0]["unclear"]), (&json!(0), &json!(false), &json!(true)));
    let verdict = case.verdict(&id);
    assert_eq!(marks(&verdict, &["state", "unclear", "note"]), vec![vec![json!("problem"), json!(true), json!("что за раз")],
                                                                    vec![json!("ok"), Value::Null, json!("")]]);
    assert!(!hitl::verdict::is_green(&verdict));
    let text = hitl::verdict::verdict_text(&verdict, false);
    assert!(text.contains("Не понял: 1"), "{text}");
}

#[test]
fn a_press_names_its_check_and_its_item() {
    let mut case = Case::new();
    let first = case.queue(&["--title", "Первая", "--check", "1. Раз", "--check", "2. Два", "--check", "3. Три"]);
    let second = case.queue(&["--title", "Вторая", "--check", "1. Раз"]);
    case.press(json!({"do": "go", "ticket": first, "label": "3"}));
    case.press(json!({"do": "take", "ticket": second}));
    case.press(json!({"do": "answer", "ticket": first, "label": "2", "number": 0})); // the other check is up: this one comes back
    case.press(json!({"do": "postpone", "ticket": first}));
    case.press(json!({"do": "go", "ticket": "нет такой", "label": "1"}));
    let at = case.home.display().to_string();
    case.play(&steps(&["remote 1", &format!("dump {at}/gone.json"), "remote 1", &format!("dump {at}/taken.json"), "remote 1",
                       &format!("dump {at}/back.json"), "remote", &format!("dump {at}/later.json")]));
    assert_eq!(case.seen("gone.json")["index"], json!(2));
    assert_eq!(case.seen("taken.json")["id"], json!(second));
    let back = case.seen("back.json");
    assert_eq!((&back["id"], &back["marks"][1]["state"]), (&json!(first), &json!("ok")));
    assert_eq!(case.seen("later.json")["id"], json!(second));
    let said = read(&case.home.join("window/remote/state.json"));
    assert_eq!((&said["active"], &said["order"]), (&json!(second), &json!([second, first])));
    assert_eq!(said["kept"], json!([{"id": first, "marked": 1}]));
}

#[test]
fn what_was_set_on_the_page_on_the_phone_goes_to_the_chat() {
    let mut case = Case::new();
    let page = case.home.join("sizes.html");
    std::fs::write(&page, PAGE).unwrap();
    let id = case.queue(&["--title", "Размеры", "--kind", "question", "--check", "1. Взять эти размеры?", "--page",
                          &page.display().to_string(), "--option", "Взять эти", "--option", "Оставить"]);
    case.press(json!({"do": "values", "ticket": id, "label": "1", "values": [["ширина", "480"], ["поля", "10"]]}));
    case.press(json!({"do": "answer", "ticket": id, "label": "1", "number": 0}));
    case.press(json!({"do": "finish", "ticket": id, "how": "done"}));
    let at = case.home.display().to_string();
    case.play(&steps(&[&format!("dump {at}/own.json"), "remote 1", &format!("dump {at}/set.json"), "remote"]));
    assert_eq!(case.seen("own.json")["sent"], json!("Уйдёт чату с ответом: ширина 520 · поля 14"));
    assert_eq!(case.seen("set.json")["sent"], json!("Уйдёт чату с ответом: ширина 480 · поля 10"));
    assert_eq!(case.verdict(&id)["results"][0]["values"], json!([["ширина", "480"], ["поля", "10"]]));
}

#[test]
fn a_picture_added_on_the_phone_is_under_the_comment() {
    let mut case = Case::new();
    let id = case.queue(&["--title", "Вход", "--check", "1. Раз"]);
    let shot = case.home.join("shots").join(format!("{id}-phone-1.png"));
    std::fs::create_dir_all(shot.parent().unwrap()).unwrap();
    std::fs::write(&shot, png()).unwrap();
    case.press(json!({"do": "shot", "ticket": id, "label": "1", "path": shot.display().to_string()}));
    case.press(json!({"do": "shot", "ticket": id, "label": "1", "path": "/etc/hosts"})); // not one of the window's own: left alone
    case.press(json!({"do": "answer", "ticket": id, "label": "1", "number": 1}));
    case.press(json!({"do": "finish", "ticket": id, "how": "done"}));
    case.play(&steps(&["remote"]));
    assert_eq!(case.verdict(&id)["results"][0]["shots"], json!([shot.display().to_string()]));
}
