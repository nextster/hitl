//! The process that talks to the phone: one at a time, for as long as checks wait.
//!
//!   the window  writes ~/.conveyor/window/remote/state.json and takes presses from remote/in/
//!   this        listens for the phone on the home network, sends it the queue with the window's word,
//!               hands its presses to the window, the files an item names to the phone, and tells Apple
//!               to show a notification when a check waits and he is not at the Mac
//!   the app     finds the Mac by what was put on the phone by cable: the Mac's name, a port, a key
//!
//! Away from home, and with a VPN on, the two meet in a room of a server of his own (`relay.rs`, `relay/`): this
//! process comes there as well as listening at home, and a phone that comes there is heard as one at home is, by the
//! same key, what is said being sealed so that the server cannot read it.

use crate::relay::{self, Message, Seal, Wire};
use crate::shown::{self, hand_to_window, keep_shot, pressed};
use crate::{codec, push, word};
use crate::word::{folder, log, relay_word};
use hitl_host_core::{now, read_json, var, write_json};
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

/// He is at the Mac while its keyboard and mouse were touched this long ago or less: the window is in front of him
/// there, and the phone stays quiet.
const AWAY_SECONDS: f64 = 60.0;
/// With nothing in the queue for this long, this process ends.
const IDLE_SECONDS: f64 = 30.0;
const MOST_FILE_BYTES: u64 = 25_000_000;
const MOST_LINE_BYTES: usize = 40_000_000;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn line_of(word: &Value) -> Vec<u8> {
    let mut line = word.to_string().into_bytes();
    line.push(b'\n');
    line
}

/// One connection of the app. Its own thread reads it and writes to it; everything else leaves words for that thread,
/// so that a phone that stopped listening holds nothing up but itself. A phone that came by the server has no
/// connection of its own: the server's is the Mac's.
pub struct Phone {
    pub place: String,
    known: AtomicBool,
    front: AtomicBool,
    gone: AtomicBool,
    /// The words in their order, and the queue as it is now: of several not sent yet only the last is worth sending.
    waiting: Mutex<(Vec<u8>, Option<Vec<u8>>)>,
}

impl Phone {
    fn new(place: &str) -> Arc<Phone> {
        Arc::new(Phone { place: place.to_string(), known: AtomicBool::new(false), front: AtomicBool::new(true),
                         gone: AtomicBool::new(false), waiting: Mutex::new((Vec::new(), None)) })
    }

    fn send(&self, word: &Value) {
        lock(&self.waiting).0.extend(line_of(word));
    }

    fn show(&self, line: &[u8]) {
        lock(&self.waiting).1 = Some(line.to_vec());
    }

    /// What to write next: the words in their order, then the queue.
    fn next(&self) -> Vec<u8> {
        let mut waiting = lock(&self.waiting);
        let mut out = std::mem::take(&mut waiting.0);
        if let Some(queue) = waiting.1.take() {
            out.extend(queue);
        }
        out
    }

    fn close(&self) {
        self.gone.store(true, Ordering::SeqCst);
    }

    fn is_gone(&self) -> bool {
        self.gone.load(Ordering::SeqCst)
    }
}

/// When the program this process runs was last changed: the newest of the sources it is built of. A change ends the
/// process, and the next check starts the new one.
fn code_time() -> Option<SystemTime> {
    let rust = hitl_host_core::root().join("rust");
    if !rust.join("hitl").is_dir() {
        return None;
    }
    let mut crates = vec!["hitl".to_string()];
    let mut at = 0;
    while at < crates.len() {
        let manifest = std::fs::read_to_string(rust.join(&crates[at]).join("Cargo.toml")).unwrap_or_default();
        for piece in manifest.split("path = \"../").skip(1) {
            let name = piece.split('"').next().unwrap_or("").to_string();
            if !name.is_empty() && !crates.contains(&name) {
                crates.push(name);
            }
        }
        at += 1;
    }
    fn newest(path: &Path, best: &mut Option<SystemTime>) {
        let Ok(meta) = std::fs::symlink_metadata(path) else { return };
        if meta.is_dir() {
            if path.file_name().is_some_and(|name| name == "target") {
                return;
            }
            for entry in std::fs::read_dir(path).into_iter().flatten().flatten() {
                newest(&entry.path(), best);
            }
        } else if let Ok(modified) = meta.modified() {
            let name = path.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default();
            if name.ends_with(".rs") || name.starts_with("Cargo.") {
                *best = (*best).max(Some(modified));
            }
        }
    }
    let mut best = None;
    for name in crates {
        newest(&rust.join(name), &mut best);
    }
    best
}

pub struct Bridge {
    known: Value,
    phones: Mutex<Vec<Arc<Phone>>>,
    allowed: Mutex<HashSet<String>>,
    /// The queue as last sent, as its line.
    said: Mutex<Vec<u8>>,
    device: Mutex<Value>,
    /// The checks the phone was told of, or showed: kept in a file, so that a start of this process in the middle of a
    /// queue does not tell of them again.
    pushed: Mutex<HashMap<String, f64>>,
    badge: Mutex<usize>,
    idle: Mutex<(f64, f64)>,
    met: AtomicBool,
    paired: Mutex<(f64, u32)>,
    /// The server's room, while this process is in it.
    wire: Mutex<Option<Arc<Wire>>>,
    over: AtomicBool,
    /// Whether this process is on its way into the server's room: it does not leave halfway in.
    entering: AtomicBool,
}

impl Bridge {
    pub fn new() -> Result<Arc<Bridge>, String> {
        let known = word::pairing()?;
        let device = read_json(&folder().join("device.json")).filter(Value::is_object).unwrap_or_else(|| json!({}));
        let pushed = read_json(&folder().join("pushed.json")).and_then(|told| told.as_object().cloned()).unwrap_or_default()
            .into_iter().filter_map(|(key, value)| Some((key, value.as_f64()?))).collect();
        Ok(Arc::new(Bridge {
            known, phones: Mutex::new(Vec::new()), allowed: Mutex::new(HashSet::new()), said: Mutex::new(Vec::new()),
            device: Mutex::new(device), pushed: Mutex::new(pushed), badge: Mutex::new(0), idle: Mutex::new((0.0, 0.0)),
            met: AtomicBool::new(false), paired: Mutex::new((0.0, 0)), wire: Mutex::new(None), over: AtomicBool::new(false),
            entering: AtomicBool::new(false),
        }))
    }

    fn port(&self) -> u16 {
        self.known.get("port").and_then(Value::as_u64).and_then(|port| u16::try_from(port).ok()).unwrap_or(0)
    }

    // The phone's side

    fn listen(self: &Arc<Bridge>) -> Result<TcpListener, String> {
        let config = crate::tls::server(&folder().join("cert.pem"), &folder().join("key.pem"))?;
        let listener = bind(&var("CONVEYOR_PHONE_BIND").unwrap_or_else(|| "::".into()), self.port())
            .map_err(|error| format!("port {}: {error}", self.port()))?;
        let accepting = listener.try_clone().map_err(|error| error.to_string())?;
        let bridge = Arc::clone(self);
        std::thread::spawn(move || {
            for plain in accepting.incoming() {
                let Ok(plain) = plain else { return };
                let place = plain.peer_addr().map(|place| place.ip().to_string()).unwrap_or_default();
                let place = place.strip_prefix("::ffff:").map(str::to_string).unwrap_or(place);
                let (bridge, config) = (Arc::clone(&bridge), Arc::clone(&config));
                std::thread::spawn(move || bridge.meet(config, plain, &place));
            }
        });
        Ok(listener)
    }

    fn meet(self: &Arc<Bridge>, config: Arc<rustls::ServerConfig>, mut plain: TcpStream, place: &str) {
        // An app that was ended without a word, by a new build put over it, leaves its connection standing: the system
        // asks the phone now and then whether anyone is still there, and lets go when nobody is.
        keepalive(&plain);
        let _ = plain.set_read_timeout(Some(Duration::from_secs(10)));
        let _ = plain.set_write_timeout(Some(Duration::from_secs(10)));
        let Ok(mut link) = rustls::ServerConnection::new(config) else { return };
        while link.is_handshaking() {
            if link.complete_io(&mut plain).is_err() {
                return;
            }
        }
        if plain.set_nonblocking(true).is_err() {
            return;
        }
        let phone = Phone::new(place);
        let came = Instant::now();
        let (mut heard, mut writing) = (Vec::new(), Vec::new());
        let mut buffer = vec![0u8; 1 << 16];
        'talk: while !phone.is_gone() {
            if writing.is_empty() && !link.wants_write() {
                writing = phone.next();
            }
            // rustls seals what it is given into its own buffer, a part at a time.
            while !writing.is_empty() {
                match link.writer().write(&writing) {
                    Ok(0) | Err(_) => break,
                    Ok(taken) => {
                        writing.drain(..taken);
                    }
                }
            }
            let (readable, writable) = wait_for(&plain, link.wants_write(), 50);
            // Until it has said the key a phone has ten seconds.
            if !phone.known.load(Ordering::SeqCst) && came.elapsed() > Duration::from_secs(10) {
                break;
            }
            if writable {
                while link.wants_write() {
                    match link.write_tls(&mut plain) {
                        Ok(_) => {}
                        Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                        Err(_) => break 'talk,
                    }
                }
            }
            // What has come on the socket, all of it.
            if readable {
                loop {
                    match link.read_tls(&mut plain) {
                        Ok(0) => break 'talk,
                        Ok(_) if link.process_new_packets().is_err() => {
                            let _ = link.write_tls(&mut plain);
                            break 'talk;
                        }
                        Ok(_) => {}
                        Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                        Err(_) => break 'talk,
                    }
                }
            }
            // Everything the connection holds, whether it came now or with the handshake: the app says its hello at
            // once, and the hello may come in one piece with the handshake's end, unseen by poll since.
            loop {
                match link.reader().read(&mut buffer) {
                    Ok(0) => break 'talk,
                    Ok(count) => heard.extend_from_slice(&buffer[..count]),
                    Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                    Err(_) => break 'talk,
                }
                // Before it has said the key a phone has a line to say it in, and no more.
                if heard.len() > (if phone.known.load(Ordering::SeqCst) { MOST_LINE_BYTES } else { 4096 }) {
                    break 'talk;
                }
            }
            while let Some(end) = heard.iter().position(|&byte| byte == b'\n') {
                let line: Vec<u8> = heard.drain(..=end).collect();
                if let Ok(said) = serde_json::from_slice::<Value>(&line[..line.len() - 1]) {
                    if said.is_object() {
                        self.hear(&phone, &said);
                    }
                }
            }
        }
        phone.close();
        let _ = plain.shutdown(std::net::Shutdown::Both);
        self.forget(&phone);
    }

    fn hear(self: &Arc<Bridge>, phone: &Arc<Phone>, said: &Value) {
        if !phone.known.load(Ordering::SeqCst) {
            let hello = said.get("hello").filter(|hello| hello.is_object());
            let token = self.known.get("token").map(crate::py::str_of).unwrap_or_default();
            let said_token = hello.and_then(|hello| hello.get("token")).and_then(Value::as_str);
            if !said_token.is_some_and(|said| codec::same(said.as_bytes(), token.as_bytes())) {
                log(&format!("a phone at {} did not say the key", phone.place));
                phone.close();
                return;
            }
            let hello = hello.expect("a hello");
            phone.known.store(true, Ordering::SeqCst);
            phone.front.store(hello.get("front") != Some(&Value::Bool(false)), Ordering::SeqCst);
            self.met.store(true, Ordering::SeqCst);
            self.remember(hello);
            log(&format!("the phone at {} came", phone.place));
            // At home the phone is told of the server's room too: a phone that has not been on the cable since the
            // server was set up learns of it here.
            let mut welcome = Map::new();
            welcome.insert("mac".into(), json!(word::mac_name(false)));
            if let Some(far) = relay_word() {
                welcome.insert("relay".into(), far);
            }
            phone.send(&json!({"welcome": welcome}));
            let mut phones = lock(&self.phones);
            // The app has one connection. Another from the same phone means the earlier one is gone, whether or not it
            // said so: left standing, it would count as an app that is open.
            for earlier in phones.iter().filter(|earlier| earlier.place == phone.place) {
                earlier.close();
            }
            phones.push(Arc::clone(phone));
            let said = lock(&self.said);
            if !said.is_empty() {
                phone.show(&said);
            }
            return;
        }
        if let Some(front) = said.get("front") {
            phone.front.store(hitl_host_core::truthy(Some(front)), Ordering::SeqCst);
        }
        if said.get("push").is_some() {
            self.remember(said);
        }
        if said.get("get").is_some() {
            let (bridge, phone, said) = (Arc::clone(self), Arc::clone(phone), said.clone());
            std::thread::spawn(move || bridge.give(&phone, &said));
        }
        if let Some(shot) = said.get("shot").filter(|shot| shot.is_object()) {
            if let Some(press) = keep_shot(shot) {
                let _ = hand_to_window(&press);
            }
        }
        if said.get("do").is_some() {
            if let Some(press) = pressed(said) {
                let _ = hand_to_window(&press);
            }
        }
    }

    // The phone's side, by the server

    /// Stays in the server's room for as long as this process lives, coming back when the way there is lost.
    fn meet_far(self: &Arc<Bridge>) {
        let (mut wait, mut told) = (1.0f64, false);
        while !self.over.load(Ordering::SeqCst) {
            let Some(known) = relay_word() else { return };
            let text = |key: &str| known.get(key).and_then(Value::as_str).unwrap_or("").to_string();
            let address = format!("{}/room/{}/mac", text("url").trim_end_matches('/'), text("room"));
            let authorization = format!("Bearer {}", text("token"));
            self.entering.store(true, Ordering::SeqCst);
            let opened = codec::unhex(&text("key")).ok_or_else(|| "non-hexadecimal number found in fromhex() arg".to_string())
                .and_then(|key| Seal::new(&key, relay::MAC, relay::PHONE, Some(&folder().join("relay-heard.json"))))
                .and_then(|seal| Wire::open(&address, &[("Authorization", &authorization)]).map(|wire| (wire, seal))
                          .map_err(|error| error.to_string()));
            let (wire, mut seal) = match opened {
                Ok(opened) => opened,
                Err(error) => {
                    self.entering.store(false, Ordering::SeqCst);
                    if !told {
                        log(&format!("the server is not reached: {error}"));
                    }
                    told = true;
                    std::thread::sleep(Duration::from_secs_f64(wait));
                    wait = (wait * 2.0).min(30.0);
                    continue;
                }
            };
            (wait, told) = (1.0, false);
            log("in the server's room");
            let wire = Arc::new(wire);
            *lock(&self.wire) = Some(Arc::clone(&wire));
            self.entering.store(false, Ordering::SeqCst);
            if let Err(error) = self.hear_far(&wire, &mut seal) {
                if !self.over.load(Ordering::SeqCst) {
                    log(&format!("the server's room was lost: {error}"));
                }
            }
            *lock(&self.wire) = None;
            wire.close();
            std::thread::sleep(Duration::from_secs_f64(wait));
        }
    }

    fn hear_far(self: &Arc<Bridge>, wire: &Wire, seal: &mut Seal) -> Result<(), String> {
        let mut phone: Option<Arc<Phone>> = None;
        let (mut said, mut heard) = (Instant::now(), Instant::now());
        let result = (|| -> Result<(), String> {
            while !self.over.load(Ordering::SeqCst) {
                if phone.as_ref().is_some_and(|phone| phone.is_gone()) {
                    self.forget(&phone.take().expect("a phone"));
                }
                if let Some(phone) = phone.as_ref() {
                    for line in phone.next().split(|&byte| byte == b'\n').filter(|line| !line.is_empty()) {
                        for frame in seal.frames(line) {
                            wire.send(&frame).map_err(|error| error.to_string())?;
                            said = Instant::now();
                        }
                    }
                }
                let got = wire.receive(0.05).map_err(|error| error.to_string())?;
                let Some(got) = got else {
                    // A word now and then keeps the way open, and a server that answers nothing is gone.
                    if said.elapsed() > Duration::from_secs(25) {
                        wire.say("ping").map_err(|error| error.to_string())?;
                        said = Instant::now();
                    }
                    if heard.elapsed() > Duration::from_secs(80) {
                        return Err("it has been silent".into());
                    }
                    continue;
                };
                heard = Instant::now();
                match got {
                    Message::Closed(_) => return Err("it closed the door".into()),
                    Message::Text(text) => {
                        // The server's own words: whether the phone is in the room.
                        let Ok(word) = serde_json::from_slice::<Value>(&text) else { continue };
                        let state = word.get("phone").and_then(Value::as_str);
                        if matches!(state, Some("here" | "gone")) {
                            if let Some(earlier) = phone.take() {
                                self.forget(&earlier);
                            }
                        }
                        if state == Some("here") {
                            phone = Some(Phone::new("the server"));
                        }
                    }
                    Message::Binary(payload) => {
                        let Some(line) = seal.line(&payload) else { continue };
                        let Ok(told) = serde_json::from_slice::<Value>(&line) else { continue };
                        let current = phone.get_or_insert_with(|| Phone::new("the server"));
                        if told.is_object() {
                            self.hear(&Arc::clone(current), &told);
                        }
                    }
                }
            }
            Ok(())
        })();
        if let Some(phone) = phone {
            self.forget(&phone);
        }
        result
    }

    fn forget(&self, phone: &Arc<Phone>) {
        phone.close();
        let mut phones = lock(&self.phones);
        if let Some(at) = phones.iter().position(|known| Arc::ptr_eq(known, phone)) {
            phones.remove(at);
            log(&format!("the phone at {} left", phone.place));
        }
    }

    /// Leaves the server's room, saying first when it is for an empty queue: the phone is then told that nothing
    /// waits, not that the Mac is gone.
    fn leave_far(&self, resting: bool) {
        // A way into the room begun a moment ago is waited for, so that the phone hears the Mac rest and not vanish.
        let until = Instant::now() + Duration::from_secs(2);
        while resting && self.entering.load(Ordering::SeqCst) && lock(&self.wire).is_none() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        // Said before the thread in the room is told to stop: stopped, it closes the way, and a word said after that is
        // lost, and the phone is told the Mac is gone.
        let wire = lock(&self.wire).clone();
        if let Some(wire) = wire.as_ref().filter(|_| resting) {
            if wire.say("resting").is_ok() {
                std::thread::sleep(Duration::from_millis(200));
            }
        }
        self.over.store(true, Ordering::SeqCst);
        if let Some(wire) = wire {
            wire.close();
        }
    }

    /// Which phone to notify: the app says its address at Apple's once it has one.
    fn remember(&self, said: &Value) {
        let Some(device) = said.get("push").and_then(Value::as_str) else { return };
        let well_formed = (32..=200).contains(&device.len()) && device.chars().all(|c| c.is_ascii_hexdigit());
        let mut known = lock(&self.device);
        if well_formed && known.get("push").and_then(Value::as_str) != Some(device) {
            let name = crate::py::str_of(&crate::py::or(said.get("device"), json!("")));
            *known = json!({"push": device, "name": name, "at": now()});
            let _ = write_json(&folder().join("device.json"), &known);
        }
    }

    /// A file an item names, for the phone to show.
    fn give(&self, phone: &Phone, said: &Value) {
        let mut answer = Map::new();
        answer.insert("file".into(), said.get("id").cloned().unwrap_or(Value::Null));
        let path = said.get("get").and_then(Value::as_str);
        let allowed = path.is_some_and(|path| lock(&self.allowed).contains(path));
        match path.map(PathBuf::from) {
            Some(_) if !allowed => {
                answer.insert("error".into(), json!("этого файла нет в очереди"));
            }
            None => {
                answer.insert("error".into(), json!("этого файла нет в очереди"));
            }
            Some(file) if !shown::is_file(&file) => {
                answer.insert("error".into(), json!("файла уже нет на Маке"));
            }
            Some(file) if std::fs::metadata(&file).map(|meta| meta.len()).unwrap_or(0) > MOST_FILE_BYTES => {
                answer.insert("error".into(), json!(format!("файл больше {} МБ: открой на Маке", MOST_FILE_BYTES / 1_000_000)));
            }
            Some(file) => match std::fs::read(&file) {
                Ok(bytes) => {
                    answer.insert("name".into(), json!(crate::py::name(&file.display().to_string())));
                    answer.insert("data".into(), json!(codec::base64(&bytes)));
                }
                Err(error) => {
                    answer.insert("error".into(), json!(error.to_string()));
                }
            },
        }
        phone.send(&Value::Object(answer));
    }

    // The loop

    /// Sends the queue to the phone when it has changed; the checks as they lie in the queue.
    fn tell(&self) -> Vec<Value> {
        let (said, allowed, tickets) = shown::snapshot();
        *lock(&self.allowed) = allowed;
        let line = line_of(&json!({"queue": said}));
        let mut last = lock(&self.said);
        if *last != line {
            *last = line.clone();
            drop(last);
            for phone in lock(&self.phones).iter() {
                phone.show(&line);
            }
        }
        tickets
    }

    fn in_front(&self) -> bool {
        lock(&self.phones).iter().any(|phone| phone.front.load(Ordering::SeqCst) && !phone.is_gone())
    }

    /// Tells the phone of the checks that wait, once he is not at the Mac and the app is not open.
    fn notify(self: &Arc<Bridge>, tickets: &[Value]) {
        let waiting: Vec<&Value> = tickets.iter().filter(|ticket| ticket.get("screen").and_then(Value::as_str) != Some("iphone")).collect();
        let device = lock(&self.device).get("push").and_then(Value::as_str).filter(|device| !device.is_empty()).map(str::to_string);
        let Some(device) = device else { return };
        if push::push_key().is_none() {
            return;
        }
        if waiting.is_empty() {
            let mut badge = lock(&self.badge);
            if *badge > 0 {
                *badge = 0;
                self.push_later(device, json!({"aps": {"badge": 0}}));
            }
            return;
        }
        let id_of = |ticket: &Value| ticket.get("id").and_then(Value::as_str).unwrap_or("").to_string();
        let fresh: Vec<&Value> = {
            let pushed = lock(&self.pushed);
            waiting.iter().copied().filter(|ticket| {
                !pushed.contains_key(&id_of(ticket)) && now() - ticket.get("createdAt").and_then(Value::as_f64).unwrap_or(0.0) >= 3.0
            }).collect()
        };
        if fresh.is_empty() {
            return;
        }
        if self.in_front() {
            let mut pushed = lock(&self.pushed);
            for ticket in &fresh {
                pushed.insert(id_of(ticket), now()); // it is on the phone's screen already
            }
            drop(pushed);
            self.keep_pushed(&waiting);
            return;
        }
        let idle = {
            let mut idle = lock(&self.idle);
            if now() - idle.0 >= 5.0 {
                *idle = (now(), push::seconds_idle());
            }
            idle.1
        };
        if idle < AWAY_SECONDS {
            return;
        }
        *lock(&self.badge) = waiting.len();
        for (at, ticket) in fresh.iter().enumerate() {
            lock(&self.pushed).insert(id_of(ticket), now());
            if at < 3 {
                self.push_later(device.clone(), push::notice(ticket, waiting.len()));
            }
        }
        self.keep_pushed(&waiting);
    }

    /// Writes down which of the waiting checks the phone knows of; the ones that are gone are forgotten.
    fn keep_pushed(&self, waiting: &[&Value]) {
        let ids: HashSet<&str> = waiting.iter().filter_map(|ticket| ticket.get("id").and_then(Value::as_str)).collect();
        let mut pushed = lock(&self.pushed);
        pushed.retain(|key, _| ids.contains(key.as_str()));
        let kept: Map<String, Value> = pushed.iter().map(|(key, value)| (key.clone(), json!(value))).collect();
        let _ = write_json(&folder().join("pushed.json"), &Value::Object(kept));
    }

    fn push_later(self: &Arc<Bridge>, device: String, payload: Value) {
        let bridge = Arc::clone(self);
        std::thread::spawn(move || bridge.push(&device, &payload));
    }

    fn push(&self, device: &str, payload: &Value) {
        let (code, said) = match push::send_push(device, payload, "alert") {
            Ok(answered) => answered,
            Err(error) => {
                log(&format!("a notification was not sent: {error}"));
                return;
            }
        };
        if code == 200 {
            let what = payload.get("ticket").filter(|ticket| hitl_host_core::truthy(Some(ticket))).map(crate::py::str_of)
                .unwrap_or_else(|| "the count on the icon".into());
            log(&format!("a notification went to Apple: {what}"));
        } else {
            log(&format!("Apple did not take a notification: {code} {said}"));
            if code == 410 || said.contains("BadDeviceToken") || said.contains("Unregistered") {
                *lock(&self.device) = json!({});
                let _ = std::fs::remove_file(folder().join("device.json"));
            }
        }
    }

    /// Until a phone has come, the Mac's word is put on the phone again from time to time: the app may have been
    /// installed after this process started, or the Mac's address at home may have changed.
    fn keep_paired(&self) {
        if self.met.load(Ordering::SeqCst) || var("CONVEYOR_PHONE_PAIR").as_deref() == Some("0") {
            return;
        }
        let mut paired = lock(&self.paired);
        let (tried, tries) = *paired;
        // At once, in twenty seconds, in a minute, then every ten minutes: a phone that is away is not asked for all
        // day. A phone that was told the same before is left alone for the first ten minutes: it knows.
        let wait = [0.0, 20.0, 60.0].get(tries as usize).copied().unwrap_or(600.0);
        if now() - tried < wait {
            return;
        }
        if tries == 0 && tried == 0.0 {
            paired.0 = now();
            if word::is_paired() {
                paired.1 = 3;
                return;
            }
        }
        *paired = (now(), tries + 1);
        std::thread::spawn(|| {
            let _ = word::pair(true);
        });
    }

    fn run(self: &Arc<Bridge>) -> Result<i32, String> {
        let listener = self.listen()?;
        log(&format!("listening for the phone on port {}", self.port()));
        if relay_word().is_some() {
            self.entering.store(true, Ordering::SeqCst);
            let bridge = Arc::clone(self);
            std::thread::spawn(move || bridge.meet_far());
        }
        let code = code_time();
        let mut code_asked = Instant::now();
        let mut empty_since: Option<f64> = None;
        let idle_exit = var("CONVEYOR_PHONE_IDLE_EXIT").and_then(|value| value.trim().parse::<f64>().ok()).unwrap_or(IDLE_SECONDS);
        let mut resting = false;
        loop {
            // Whatever lies in the queue, the phone is still listened for.
            let looked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let tickets = self.tell();
                self.notify(&tickets);
                tickets
            }));
            let tickets = match looked {
                Ok(tickets) => tickets,
                Err(panic) => {
                    let what = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string()))
                        .unwrap_or_default();
                    log(&format!("a look at the queue failed: {what}"));
                    std::thread::sleep(Duration::from_secs(1));
                    continue;
                }
            };
            if tickets.is_empty() {
                let since = *empty_since.get_or_insert_with(now);
                if now() - since > idle_exit {
                    resting = true;
                    break;
                }
            } else {
                empty_since = None;
                self.keep_paired();
            }
            if code_asked.elapsed() >= Duration::from_secs(1) {
                code_asked = Instant::now();
                if code_time() != code {
                    log("the code changed: ending, the next check starts the new one");
                    break;
                }
            }
            let listening = !lock(&self.phones).is_empty();
            std::thread::sleep(Duration::from_millis(if listening { 50 } else { 500 }));
        }
        self.leave_far(resting);
        drop(listener);
        for phone in lock(&self.phones).iter() {
            phone.close();
        }
        Ok(0)
    }
}

/// A listening socket on both IPv6 and IPv4, as the phone may come by either.
fn bind(host: &str, port: u16) -> std::io::Result<TcpListener> {
    let address: std::net::IpAddr = host.parse().map_err(|_| std::io::Error::new(ErrorKind::InvalidInput, format!("not an address: {host}")))?;
    let place = std::net::SocketAddr::new(address, port);
    let family = if place.is_ipv6() { libc::AF_INET6 } else { libc::AF_INET };
    let socket = unsafe { libc::socket(family, libc::SOCK_STREAM, 0) };
    if socket < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let listener = unsafe { TcpListener::from_raw_fd(socket) };
    let one: libc::c_int = 1;
    let zero: libc::c_int = 0;
    let size = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    unsafe {
        libc::fcntl(socket, libc::F_SETFD, libc::FD_CLOEXEC);
        libc::setsockopt(socket, libc::SOL_SOCKET, libc::SO_REUSEADDR, (&raw const one).cast(), size);
        if place.is_ipv6() {
            libc::setsockopt(socket, libc::IPPROTO_IPV6, libc::IPV6_V6ONLY, (&raw const zero).cast(), size);
        }
    }
    let bound = match place {
        std::net::SocketAddr::V6(v6) => {
            let mut raw: libc::sockaddr_in6 = unsafe { std::mem::zeroed() };
            raw.sin6_len = std::mem::size_of::<libc::sockaddr_in6>() as u8;
            raw.sin6_family = libc::AF_INET6 as libc::sa_family_t;
            raw.sin6_port = v6.port().to_be();
            raw.sin6_addr.s6_addr = v6.ip().octets();
            raw.sin6_scope_id = v6.scope_id();
            unsafe { libc::bind(socket, (&raw const raw).cast(), std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t) }
        }
        std::net::SocketAddr::V4(v4) => {
            let mut raw: libc::sockaddr_in = unsafe { std::mem::zeroed() };
            raw.sin_len = std::mem::size_of::<libc::sockaddr_in>() as u8;
            raw.sin_family = libc::AF_INET as libc::sa_family_t;
            raw.sin_port = v4.port().to_be();
            raw.sin_addr.s_addr = u32::from_ne_bytes(v4.ip().octets());
            unsafe { libc::bind(socket, (&raw const raw).cast(), std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t) }
        }
    };
    if bound != 0 || unsafe { libc::listen(socket, 4) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(listener)
}

fn keepalive(socket: &TcpStream) {
    let fd = socket.as_raw_fd();
    let size = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    for (level, name, value) in [(libc::SOL_SOCKET, libc::SO_KEEPALIVE, 1), (libc::IPPROTO_TCP, libc::TCP_KEEPALIVE, 10),
                                 (libc::IPPROTO_TCP, libc::TCP_KEEPINTVL, 5), (libc::IPPROTO_TCP, libc::TCP_KEEPCNT, 3)] {
        let value: libc::c_int = value;
        unsafe { libc::setsockopt(fd, level, name, (&raw const value).cast(), size) };
    }
}

/// Whether a socket can be read from, and written to when that is asked, within `milliseconds`.
fn wait_for(socket: &TcpStream, write: bool, milliseconds: i32) -> (bool, bool) {
    let mut asked = libc::pollfd { fd: socket.as_raw_fd(), events: libc::POLLIN | if write { libc::POLLOUT } else { 0 }, revents: 0 };
    if unsafe { libc::poll(&raw mut asked, 1, milliseconds) } <= 0 {
        return (false, false);
    }
    let ended = asked.revents & (libc::POLLHUP | libc::POLLERR) != 0;
    (asked.revents & libc::POLLIN != 0 || ended, asked.revents & libc::POLLOUT != 0)
}

pub fn lock_file() -> PathBuf {
    folder().join("bridge.lock")
}

fn flock(file: &std::fs::File, operation: libc::c_int) -> bool {
    unsafe { libc::flock(file.as_raw_fd(), operation) == 0 }
}

/// The process itself: one at a time, for as long as checks wait.
pub fn serve() -> i32 {
    let _ = std::fs::create_dir_all(folder());
    let Ok(lock) = std::fs::OpenOptions::new().create(true).append(true).open(lock_file()) else { return 0 };
    if !flock(&lock, libc::LOCK_EX | libc::LOCK_NB) {
        return 0;
    }
    let result = Bridge::new().and_then(|bridge| bridge.run());
    drop(lock);
    match result {
        Ok(code) => code,
        Err(error) => {
            log(&format!("the phone is not listened for: {error}"));
            1
        }
    }
}

/// Whether the process that talks to the phone runs: it holds the lock.
pub fn running() -> bool {
    let Ok(lock) = std::fs::OpenOptions::new().create(true).append(true).open(lock_file()) else { return false };
    if !flock(&lock, libc::LOCK_EX | libc::LOCK_NB) {
        return true;
    }
    flock(&lock, libc::LOCK_UN);
    false
}

/// The program that serves: this one. It is signed with his Developer ID by the file `hitl` that builds it, and the
/// Mac's firewall lets signed software be reached from the home network without asking.
fn program() -> Option<PathBuf> {
    var("HITL_PROGRAM").filter(|named| !named.is_empty()).map(PathBuf::from).filter(|path| path.is_file())
        .or_else(|| std::env::current_exe().ok())
}

/// Starts the process that talks to the phone, unless it runs. Called wherever the window is brought up.
pub fn ensure() {
    if var("CONVEYOR_PHONE").as_deref() == Some("0") || var("CONVEYOR_WINDOW").is_some() || var("CONVEYOR_WINDOW_SCRIPT").is_some() {
        return; // this project's own checks talk to no phone
    }
    if std::fs::create_dir_all(folder()).is_err() || running() {
        return;
    }
    let Some(program) = program() else { return };
    let Ok(output) = std::fs::OpenOptions::new().create(true).append(true).open(folder().join("bridge.log")) else { return };
    let Ok(errors) = output.try_clone() else { return };
    let mut command = std::process::Command::new(program);
    command.args(["phone", "_serve"]).stdin(std::process::Stdio::null()).stdout(output).stderr(errors)
        .env("CONVEYOR_HOME", hitl_host_core::home()).env_remove("HITL_CORE");
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let _ = command.spawn();
}
