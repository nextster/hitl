//! What the tests of the window share (the port of `WindowTests` of tests/check-tests.py).
//!
//! The window is started only with CONVEYOR_WINDOW_SCRIPT: then it is never put on the screen, never takes the
//! keyboard and opens nothing, and it takes non-keyboard actions from a file of steps. Every test has a made-up
//! HOME, CONVEYOR_HOME and HITL_HOME of its own in the temporary folder, and every window it starts is killed, with
//! what the window started, when the test ends, also on a failure. Nothing here changes the environment of the test
//! process: what a child needs it is given.
#![allow(dead_code)]

use serde_json::Value;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

/// What the task is, for every check a test queues.
pub const ABOUT: &str = "Плагин Obsidian для Realize: делаю ему вход без пароля.";
/// The chat every check of a test comes from.
pub const CHAT: &str = "r/66 · API для плагина";
/// The program of this crate: `check`, `drop`, `_panel build`, and `check --presentation` and `journal _stream` for
/// the window, which calls back what CONVEYOR_COMMAND names.
pub const HITL: &str = env!("CARGO_BIN_EXE_hitl");

static COUNT: AtomicUsize = AtomicUsize::new(0);

/// The window's sources in this tree.
fn sources() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("window")
}

/// What every child gets of the environment: a plain PATH, the made-up home, the temporary folder.
fn plain(command: &mut Command, user: &Path) {
    command.env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("HOME", user)
        .env("TMPDIR", std::env::temp_dir());
    for name in ["USER", "LOGNAME"] {
        if let Ok(value) = std::env::var(name) {
            command.env(name, value);
        }
    }
}

/// The window as the command builds it (`hitl _panel build`, that is `hitl::window::built_panel`), once per version
/// of its source. The build is kept between the runs, in the folder the Python suite keeps it in, under the same
/// name and the same lock: one builds, the others wait for what it built.
pub fn panel() -> &'static Path {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT.get_or_init(build)
}

fn build() -> PathBuf {
    let kept = std::env::temp_dir().join("conveyor-tests");
    fs::create_dir_all(&kept).unwrap();
    let lock = fs::OpenOptions::new().create(true).truncate(false).write(true).open(kept.join("build.lock")).unwrap();
    assert_eq!(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) }, 0, "the lock of the window's build");
    let mut command = Command::new(HITL);
    plain(&mut command, &kept);
    command.args(["_panel", "build"]).arg(sources())
        .env("HITL_TOOL", "hitl-host").env("CONVEYOR_HOME", &kept).env("HITL_HOME", &kept).env("CONVEYOR_JOURNAL", "0");
    let built = command.output().expect("hitl _panel build");
    assert!(built.status.success(), "the window did not build:\n{}", String::from_utf8_lossy(&built.stderr));
    let binary = PathBuf::from(String::from_utf8_lossy(&built.stdout).trim());
    assert!(binary.is_file(), "no window at {}", binary.display());
    // Touched: the build of this version is in use.
    let named = std::ffi::CString::new(binary.as_os_str().as_encoded_bytes()).unwrap();
    unsafe { libc::utimes(named.as_ptr(), std::ptr::null()) };
    let now = SystemTime::now();
    // The windows of other versions, once nobody has run them for a day.
    if let Ok(entries) = fs::read_dir(binary.parent().unwrap()) {
        for other in entries.flatten().map(|entry| entry.path()) {
            let named = other.file_name().is_some_and(|name| name.to_string_lossy().starts_with("CheckPanel-"));
            let old = fs::metadata(&other).and_then(|meta| meta.modified()).ok()
                .and_then(|modified| now.duration_since(modified).ok()).is_some_and(|age| age > Duration::from_secs(24 * 3600));
            if named && other != binary && old {
                let _ = fs::remove_file(&other);
            }
        }
    }
    drop(lock);
    binary
}

/// One test's folders: the made-up hitl home, the made-up HOME, a repository the checks are queued from, and
/// what the window writes for the test (its dumps and pictures), all under one folder that goes with the test.
pub struct Case {
    pub root: PathBuf,
    pub home: PathBuf,
    pub user: PathBuf,
    pub project: PathBuf,
    /// More of the environment for the window and for `hitl`.
    pub more: Vec<(String, String)>,
}

impl Drop for Case {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

impl Case {
    pub fn new(name: &str) -> Case {
        // The window is built before anything of the test is set up, as the Python suite built it before its first
        // test: a drop asked with a deadline of seconds must not wait out the build.
        panel();
        let root = std::env::temp_dir().join(format!("hitl-window-{name}-{}-{}", std::process::id(), COUNT.fetch_add(1, Ordering::SeqCst)));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let (home, user, project) = (root.join("home"), root.join("user"), root.join("project"));
        for folder in [&home, &user, &project] {
            fs::create_dir_all(folder).unwrap();
        }
        let case = Case { root, home, user, project, more: Vec::new() };
        // A check says the folder and the branch it was asked from: a repository of its own, on a branch.
        case.git(&["init", "-q", "-b", "nextster/window-tests"]);
        case.git(&["-c", "user.name=Tests", "-c", "user.email=tests@example.invalid", "commit", "-q", "--allow-empty", "-m", "start"]);
        case
    }

    fn git(&self, arguments: &[&str]) {
        let mut command = Command::new("git");
        plain(&mut command, &self.user);
        let done = command.args(arguments).current_dir(&self.project)
            .env("GIT_CONFIG_NOSYSTEM", "1").env("GIT_CONFIG_GLOBAL", "/dev/null").output().expect("git");
        assert!(done.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&done.stderr));
    }

    /// A file of the test, beside the home: a dump, a picture, a script.
    pub fn at(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    /// The environment of hitl's own `check`, as the Python suite's `load` set it: a chat of its own, no window
    /// brought up by the command, nothing handed to the Codex, Claude or OpenCode of whoever runs the tests.
    fn configure_host(&self, command: &mut Command) {
        plain(command, &self.user);
        command.env("HITL_TOOL", "hitl-host").env("CONVEYOR_HOME", &self.home).env("HITL_HOME", &self.home)
            .env("CONVEYOR_NOTIFY", "0").env("CONVEYOR_CHAT", CHAT).env("CONVEYOR_CARDS", self.home.join("vault").join("cards"))
            .env("CONVEYOR_CODEX", "").env("CONVEYOR_CLAUDE", "").env("CONVEYOR_OPENCODE", "").env("CONVEYOR_JOURNAL", "0");
        command.envs(self.more.iter().map(|(key, value)| (key.as_str(), value.as_str())));
    }

    /// `hitl ARGUMENTS` as the chat of the test runs it, from its repository, with no window.
    pub fn hitl(&self, arguments: &[&str]) -> Output {
        let mut command = Command::new(HITL);
        self.configure_host(&mut command);
        command.env("CONVEYOR_WINDOW", "").args(arguments).current_dir(&self.project).stdin(Stdio::null());
        command.output().expect("hitl")
    }

    /// A check queued as a chat queues it without waiting; its number.
    pub fn queue(&self, arguments: &[&str]) -> String {
        let mut all = vec!["check", "--about", ABOUT];
        all.extend_from_slice(arguments);
        all.push("--no-wait");
        let done = self.hitl(&all);
        assert_eq!(done.status.code(), Some(75), "hitl check {arguments:?}: {}", String::from_utf8_lossy(&done.stderr));
        let ticket = String::from_utf8_lossy(&done.stdout).trim().to_string();
        assert!(!ticket.is_empty() && self.home.join("queue").join(format!("{ticket}.json")).is_file(), "no check {ticket:?} in the queue");
        ticket
    }

    /// The window, started with a script of these steps; it goes when the test is done with it.
    pub fn start(&self, steps: &[String], screen: &str) -> Window {
        let script = self.at("script.txt");
        fs::write(&script, steps.join("\n") + "\n").unwrap();
        let mut command = Command::new(panel());
        self.configure_host(&mut command);
        command.env("CONVEYOR_WINDOW_SCRIPT", &script).env("CONVEYOR_COMMAND", HITL);
        if !screen.is_empty() {
            command.env("CONVEYOR_WINDOW_SCREEN", screen);
        }
        let number = COUNT.fetch_add(1, Ordering::SeqCst);
        let (out, errors) = (self.at(&format!("window-{number}.out")), self.at(&format!("window-{number}.err")));
        command.current_dir(&self.root).stdin(Stdio::null())
            .stdout(fs::File::create(&out).unwrap()).stderr(fs::File::create(&errors).unwrap())
            // A group of its own: whatever the window starts goes with it.
            .process_group(0);
        let child = command.spawn().expect("the window");
        Window { child: Some(child), out, errors }
    }

    /// The window run through the steps to its end, as `play` of the Python suite: it exits by itself, with 0.
    pub fn play(&self, steps: &[String]) {
        self.play_on(steps, "")
    }

    /// `screen`: a screen that stands in for this Mac's own, its points as «1400x900», for what depends on its size.
    pub fn play_on(&self, steps: &[String], screen: &str) {
        let mut window = self.start(steps, screen);
        let status = window.wait(Duration::from_secs(60));
        assert!(status.success(), "the window ended with {status}: {}", window.said());
    }

    /// A dump the window wrote: it says the window never held the keyboard and was never made the active app.
    pub fn seen(&self, name: &str) -> Value {
        let seen = read(&self.at(name));
        assert_eq!(seen["hasKeyboard"], Value::Bool(false), "a scripted window must never hold the keyboard");
        assert_eq!(seen["timesForward"], Value::from(0), "a scripted window must never be made the active app, even for a moment");
        seen
    }

    pub fn verdict(&self, ticket: &str) -> Value {
        read(&self.home.join("done").join(format!("{ticket}.json")))
    }

    pub fn queued(&self, ticket: &str) -> PathBuf {
        self.home.join("queue").join(format!("{ticket}.json"))
    }

    /// A press in the app on the phone, as the phone's process hands it to the window.
    pub fn press(&self, name: &str, word: &Value) {
        let inbox = self.home.join("window").join("remote").join("in");
        fs::create_dir_all(&inbox).unwrap();
        fs::write(inbox.join(format!("{name}.json")), word.to_string()).unwrap();
    }

    /// A drop asked of the window while it is up, as `drop` asks it.
    pub fn drop_request(&self, token: &str, word: &Value) -> PathBuf {
        let folder = self.home.join("window").join("drops");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join(format!("{token}.request.json")), word.to_string()).unwrap();
        folder.join(format!("{token}.reply.json"))
    }

    /// A check that is on the phone, in the window's queue as the phone's queue puts it there; the folder where the
    /// two screens leave word for each other.
    pub fn phone_check(&self, ticket: &str) -> PathBuf {
        let folder = self.home.join("iphone").join("UDID").join("tickets").join(ticket);
        fs::create_dir_all(&folder).unwrap();
        let items: Vec<Value> = ["Первый", "Второй", "Третий"].iter().enumerate()
            .map(|(number, text)| serde_json::json!({"label": (number + 1).to_string(), "text": text})).collect();
        let ticket_json = serde_json::json!({
            "id": ticket, "screen": "iphone", "device": "XYZC", "udid": "UDID", "folder": folder.display().to_string(), "tag": "r/38",
            "chat": "r/38 · Бары внизу", "title": "Бары", "about": "", "kind": "checklist", "kindTitle": "на телефоне",
            "answers": ["работает", "не работает", "пропустить"], "project": "", "branch": "", "commit": "",
            "items": items, "createdAt": 4e9});
        let queue = self.home.join("queue");
        fs::create_dir_all(&queue).unwrap();
        fs::write(queue.join(format!("{ticket}.json")), ticket_json.to_string()).unwrap();
        folder
    }
}

/// A window a test started. Dropped, it is killed with its whole group if it is still up: a failed test leaves
/// no window behind.
pub struct Window {
    child: Option<Child>,
    out: PathBuf,
    errors: PathBuf,
}

impl Window {
    /// Its end, within the time given; a window that outstays it is killed and the test fails.
    pub fn wait(&mut self, within: Duration) -> ExitStatus {
        let until = Instant::now() + within;
        loop {
            let child = self.child.as_mut().expect("a window that runs");
            if let Some(status) = child.try_wait().expect("the window's state") {
                self.child = None;
                return status;
            }
            if Instant::now() >= until {
                self.kill();
                panic!("the window did not end within {within:?}: {}", self.said());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// What it wrote to its output and errors.
    pub fn said(&self) -> String {
        let out = fs::read_to_string(&self.out).unwrap_or_default();
        let errors = fs::read_to_string(&self.errors).unwrap_or_default();
        format!("out: {out:?}, errors: {errors:?}")
    }

    fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            if child.try_wait().ok().flatten().is_none() {
                // Its own group, whose number is its own: killed before it is reaped, so the number is still its.
                unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
                let _ = child.kill();
            }
            let _ = child.wait();
        }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        self.kill();
    }
}

pub fn read(path: &Path) -> Value {
    let text = fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// Waits for a file the window writes to be there whole.
pub fn written(path: &Path, within: Duration) -> Value {
    let until = Instant::now() + within;
    loop {
        if let Some(value) = fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()) {
            return value;
        }
        assert!(Instant::now() < until, "{} was not written within {within:?}", path.display());
        std::thread::sleep(Duration::from_millis(10));
    }
}

// --------------------------------------------------------------------------
// A picture of one colour

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn chunk(kind: &[u8], data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut summed = kind.to_vec();
    summed.extend_from_slice(data);
    out.extend_from_slice(&crc32(&summed).to_be_bytes());
    out
}

/// zlib's frame around one stored block of deflate: what a PNG of a pixel needs, with no compressor.
fn stored(data: &[u8]) -> Vec<u8> {
    let length = data.len() as u16;
    let mut out = vec![0x78, 0x01, 0x01];
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(&(!length).to_le_bytes());
    out.extend_from_slice(data);
    let (mut a, mut b) = (1u32, 0u32);
    for byte in data {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    out.extend_from_slice(&((b << 16) | a).to_be_bytes());
    out
}

/// A picture of one pixel of one colour, as a PNG; its bytes.
pub fn one_colour(path: &Path, rgb: [u8; 3]) -> Vec<u8> {
    let mut header = Vec::new();
    header.extend_from_slice(&1u32.to_be_bytes());
    header.extend_from_slice(&1u32.to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
    data.extend(chunk(b"IHDR", &header));
    data.extend(chunk(b"IDAT", &stored(&[0, rgb[0], rgb[1], rgb[2]])));
    data.extend(chunk(b"IEND", b""));
    fs::write(path, &data).unwrap();
    data
}

// --------------------------------------------------------------------------
// A server on this Mac that must hear nothing

/// An HTTP server on the loopback that writes down every path it is asked for and answers «ok» to anyone.
pub struct Listener {
    pub port: u16,
    pub asked: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Listener {
    pub fn start() -> Listener {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (asked, stop) = (Arc::new(Mutex::new(Vec::new())), Arc::new(AtomicBool::new(false)));
        let (heard, stopping) = (asked.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stopping.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(mut stream) = stream else { continue };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut request = Vec::new();
                let mut buffer = [0u8; 4096];
                while !request.windows(4).any(|end| end == b"\r\n\r\n") {
                    match stream.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(count) => request.extend_from_slice(&buffer[..count]),
                    }
                }
                let line = String::from_utf8_lossy(&request).lines().next().unwrap_or("").to_string();
                let path = line.split(' ').nth(1).unwrap_or(&line).to_string();
                heard.lock().unwrap().push(path);
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
            }
        });
        Listener { port, asked, stop, thread: Some(thread) }
    }

    pub fn address(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn heard(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }

    /// Asks it once from here, sees it heard, and forgets it: a server that hears nobody proves nothing.
    pub fn probe(&self) {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        stream.write_all(b"GET /probe HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();
        let mut answer = String::new();
        let _ = stream.read_to_string(&mut answer);
        assert!(answer.ends_with("ok"), "{answer:?}");
        assert_eq!(std::mem::take(&mut *self.asked.lock().unwrap()), ["/probe"]);
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
