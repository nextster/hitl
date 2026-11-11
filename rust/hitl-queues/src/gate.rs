//! `hitl gate`: what ios-run.sh asks before it uses the shared iPhone or the Mac's test host (gate.py).
//!
//! ios-run.sh knows no queue. Before a run on a physical device, and before a test run on the Mac, it calls the
//! program named "ios-run-gate" beside it: /Users/example/Code/scripts/ios-run-gate is a link to this project's
//! command, which answers as the gate when it is called by that name. The calls are listed in `ios-run.sh --help`,
//! under "Shared hardware". The gate answers them from the queues.
//!
//! The native and system layer (the pid's birth time, the list of mounts, Xcode's metadata, `ps` and `lsof`, Disk
//! Arbitration) and the turn of the line sit behind the trait [`Host`], so a test can replace them: the Python tests
//! patch `runner_birth`, `mount_reports`, `mount_project`, `mount_unused`, `unmount_app` and `app_mounts`; here the
//! test's host answers instead. [`RealHost`] is the one `main` uses.
#![allow(dead_code)]

use hitl_host_core::process::{Options, run};
use hitl_host_core::{EXIT_REFUSED, chat_tag, home, read_json, say, var, write_json};
use serde_json::{Value, json};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant};

/// The module's own text, printed to stderr when the gate is called with fewer than two words.
const DOC: &str = "`hitl gate`: what ios-run.sh asks before it uses the shared iPhone or the Mac's test host.

ios-run.sh knows no queue. Before a run on a physical device, and before a
test run on the Mac, it calls the program named \"ios-run-gate\" beside it:
/Users/example/Code/scripts/ios-run-gate is a link to this project's command,
which answers as the gate when it is called by that name. The calls are listed
in `ios-run.sh --help`, under \"Shared hardware\". The gate answers them from the
queues:

  A phone is used through the queue of hand checks. An install or a launch
  that did not come through `hitl iphone --probe` is refused, with the two
  ways onto the phone spelled out. A probe, and every test run on a device,
  waits until no check is on the phone or queued, and holds it until
  ios-run.sh exits.

  Test runs on the Mac take turns, whoever starts them and from whichever
  checkout: each one installs the app, and the install ends every running
  copy of it. Before giving the turn back, the gate normally unmounts only
  new .XCInstall wrapper attachments verified against this run's project.
  Public Disk Arbitration does that without sudo, force or a daemon. Old,
  foreign, busy and changed mounts are preserved; uncertainty is reported.

`hitl iphone` says how a run came through it in the environment:
CONVEYOR_PROBE=1 for the caller's own run, CONVEYOR_FORCE=1 for one that takes
the phone out of turn.";

// The layer a test replaces

/// What a program run for the gate answered.
pub struct Ran {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// Starts a program for the gate; an error when it could not start or did not end in its five seconds.
pub type Runner<'a> = &'a dyn Fn(&[&str]) -> Result<Ran, String>;

/// `subprocess.run(..., timeout=5)`.
pub fn real_run(argv: &[&str]) -> Result<Ran, String> {
    let (program, rest) = argv.split_first().ok_or("нет программы")?;
    let done = run(*program, rest, &Options::new("gate.run").timeout(5.0)).map_err(|error| error.to_string())?;
    if done.timed_out {
        return Err(format!("{program}: не ответила за 5 секунд"));
    }
    Ok(Ran { code: done.code, stdout: done.out(), stderr: done.err() })
}

/// Everything the gate does that touches the system or another module: the real one is [`RealHost`].
pub trait Host {
    // --- the native layer (the names of gate.py's functions)
    /// Exact Darwin identity of a process; a recycled PID must never inherit teardown.
    fn runner_birth(&self, pid: i64) -> Result<String, String>;
    fn mount_reports(&self) -> PathBuf {
        home().join("turns/mac-tests/mount-cleanup")
    }
    fn app_mounts(&self) -> Result<Vec<Value>, String>;
    fn mount_project(&self, mount: &Value) -> Result<String, String>;
    fn mount_unused(&self, mount: &Value) -> Result<bool, String>;
    /// Ordinary unmount through public Disk Arbitration: `Ok(None)` is a callback that did not arrive.
    fn unmount_app(&self, target: &str) -> Result<Option<i32>, String>;

    // --- the two pieces the gate itself puts together (a test patches them too)
    fn begin_mount_tracking(&self, pid: i64, project: &str) -> Result<Value, String> {
        begin_mount_tracking(self, pid, project)
    }
    fn clean_test_mounts(&self, snapshot: &Value, pid: i64) -> Result<Value, String> {
        clean_test_mounts(self, snapshot, pid)
    }

    // --- the queues
    /// `queues.main(argv)`.
    fn queue(&self, arguments: &[String]) -> i32;
    /// The state of the Mac's test line, changed under its lock; an error from the change is passed back.
    fn locked(&self, change: &mut dyn FnMut(&mut Value) -> Result<(), String>) -> Result<(), String>;
    fn leave(&self, pid: i64);

    // --- the process and the words
    fn say(&self, text: &str) {
        say(text)
    }
    fn darwin(&self) -> bool {
        cfg!(target_os = "macos")
    }
    fn pid(&self) -> i64 {
        std::process::id() as i64
    }
    fn parent_pid(&self) -> i64 {
        unsafe { libc::getppid() as i64 }
    }
    fn chat_tag(&self) -> String {
        chat_tag()
    }
    /// `git -C project branch --show-current`, "" when it says nothing.
    fn branch_of(&self, project: &str) -> String {
        match run("git", &["-C", project, "branch", "--show-current"], &Options::new("gate.branch")) {
            Ok(done) => done.out().trim().to_string(),
            Err(_) => String::new(),
        }
    }
}

/// The gate as it runs for ios-run.sh.
pub struct RealHost;

impl Host for RealHost {
    fn runner_birth(&self, pid: i64) -> Result<String, String> {
        runner_birth(pid)
    }
    fn app_mounts(&self) -> Result<Vec<Value>, String> {
        app_mounts(&real_run)
    }
    fn mount_project(&self, mount: &Value) -> Result<String, String> {
        mount_project(mount)
    }
    fn mount_unused(&self, mount: &Value) -> Result<bool, String> {
        mount_unused(mount, &real_run)
    }
    fn unmount_app(&self, target: &str) -> Result<Option<i32>, String> {
        unmount_app(target, 10.0)
    }
    fn queue(&self, arguments: &[String]) -> i32 {
        crate::commands::run(arguments)
    }
    fn locked(&self, change: &mut dyn FnMut(&mut Value) -> Result<(), String>) -> Result<(), String> {
        crate::lines::mac_tests().locked(|state| change(state))
    }
    fn leave(&self, pid: i64) {
        crate::lines::mac_tests().leave(pid)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Darwin: the birth of a process

#[cfg(target_os = "macos")]
unsafe extern "C" {
    // libSystem carries libproc; the Python reaches the same function through /usr/lib/libproc.dylib.
    fn proc_pidinfo(pid: i32, flavor: i32, arg: u64, buffer: *mut c_void, size: i32) -> i32;
}

/// `struct proc_bsdinfo` of <sys/proc_info.h> (flavor 3, PROC_PIDTBSDINFO).
#[cfg(target_os = "macos")]
#[repr(C)]
struct BsdInfo {
    ints: [u32; 12],
    comm: [u8; 16],
    name: [u8; 32],
    more: [u32; 6],
    seconds: u64,
    micros: u64,
}

/// Exact Darwin identity: the second and microsecond a process started; a recycled PID must never inherit teardown.
#[cfg(target_os = "macos")]
pub fn runner_birth(pid: i64) -> Result<String, String> {
    let mut value = BsdInfo { ints: [0; 12], comm: [0; 16], name: [0; 32], more: [0; 6], seconds: 0, micros: 0 };
    let size = std::mem::size_of::<BsdInfo>() as i32;
    let got = unsafe { proc_pidinfo(pid as i32, 3, 0, &mut value as *mut BsdInfo as *mut c_void, size) };
    if got != size || value.ints[3] as i64 != pid || value.ints[5] != unsafe { libc::getuid() } || value.seconds == 0 {
        return Err("не установлено время запуска владельца теста".into());
    }
    Ok(format!("{}:{}", value.seconds, value.micros))
}

#[cfg(not(target_os = "macos"))]
pub fn runner_birth(_pid: i64) -> Result<String, String> {
    Err("не установлено время запуска владельца теста".into())
}

// ---------------------------------------------------------------------------------------------------------------
// Mounts

/// `Path.resolve()` without `strict`: symlinks of the part that exists are followed, the rest is kept as it is.
fn resolve_loose(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    let mut here = absolute.as_path();
    loop {
        if let Ok(real) = std::fs::canonicalize(here) {
            return rest.iter().rev().fold(real, |built, part| built.join(part));
        }
        match (here.parent(), here.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_os_string());
                here = parent;
            }
            _ => return absolute,
        }
    }
}

fn is_uuid_name(name: &str) -> bool {
    name.len() == 36 && name.chars().all(|letter| letter.is_ascii_hexdigit() || letter == '-')
}

/// `(.+/\.XCInstall/[^/]+\.app/Wrapper)` as a whole text.
fn is_wrapper_source(source: &str) -> bool {
    let Some(rest) = source.strip_suffix(".app/Wrapper") else { return false };
    let Some(slash) = rest.rfind('/') else { return false };
    let (before, name) = (&rest[..slash], &rest[slash + 1..]);
    !name.is_empty() && before.len() > "/.XCInstall".len() && before.ends_with("/.XCInstall")
}

/// One line of /sbin/mount as `(.+/\.XCInstall/[^/]+\.app/Wrapper) on (.+) \((nullfs, .+)\)`: (source, target, flags).
fn parse_mount_line(line: &str) -> Option<(&str, &str, &str)> {
    let mut from = line.len();
    while let Some(at) = line[..from].rfind(" on ") {
        from = at;
        let (source, rest) = (&line[..at], &line[at + 4..]);
        if !is_wrapper_source(source) {
            continue;
        }
        let Some(inner) = rest.strip_suffix(')') else { continue };
        let mut end = inner.len();
        while let Some(open) = inner[..end].rfind(" (nullfs, ") {
            end = open;
            let flags = &inner[open + 2..];
            if open > 0 && flags.len() > "nullfs, ".len() {
                return Some((source, &inner[..open], flags));
            }
        }
    }
    None
}

fn identity_of(path: &Path) -> std::io::Result<(i64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let found = std::fs::metadata(path)?;
    Ok((found.dev() as i64, found.ino()))
}

/// Only this user's Xcode iOS-on-Mac wrapper attachments, never real disks.
pub fn app_mounts(run: Runner) -> Result<Vec<Value>, String> {
    let asked = run(&["/usr/bin/getconf", "DARWIN_USER_DIR"])?;
    if asked.code != Some(0) || asked.stdout.trim().is_empty() {
        return Err("не найден системный каталог пользователя".into());
    }
    let root = resolve_loose(Path::new(asked.stdout.trim())).parent().map(|up| up.join("X")).unwrap_or_default();
    let listed = run(&["/sbin/mount"])?;
    if listed.code != Some(0) {
        return Err("не прочитан список подключений".into());
    }
    let mut found = Vec::new();
    for line in listed.stdout.lines() {
        let Some((source, target, flags)) = parse_mount_line(line) else { continue };
        let path = Path::new(target);
        let named = path.file_name().and_then(|name| name.to_str()).is_some_and(is_uuid_name);
        if path.parent() != Some(root.as_path()) || !named || !flags.split(", ").any(|flag| flag == "read-only") {
            continue;
        }
        let (device, inode) = identity_of(path).map_err(|error| error.to_string())?;
        let original = identity_of(Path::new(source)).ok();
        found.push(json!({
            "source": source, "target": target, "device": device, "inode": inode,
            "sourceDevice": original.map(|(device, _)| device), "sourceInode": original.map(|(_, inode)| inode),
        }));
    }
    Ok(found)
}

/// What names a mount: where it is from, where it is, and the identity of both.
fn mount_key(mount: &Value) -> Vec<Value> {
    ["source", "target", "device", "inode", "sourceDevice", "sourceInode"]
        .iter()
        .map(|key| mount.get(*key).cloned().unwrap_or(Value::Null))
        .collect()
}

fn mount_text<'a>(mount: &'a Value, key: &str) -> &'a str {
    mount.get(key).and_then(Value::as_str).unwrap_or("")
}

/// The project folder a wrapper was built for, read from Xcode's `info.plist` of its DerivedData folder.
pub fn mount_project(mount: &Value) -> Result<String, String> {
    use std::os::unix::fs::MetadataExt;
    let io = |error: std::io::Error| error.to_string();
    let source = Path::new(mount_text(mount, "source"));
    let home_folder = var("HOME").filter(|value| !value.is_empty()).unwrap_or_else(|| "/".into());
    let derived = resolve_loose(&Path::new(&home_folder).join("Library/Developer/Xcode/DerivedData"));
    let real = std::fs::canonicalize(source).map_err(io)?;
    if !real.starts_with(&derived) {
        return Err(format!("'{}' is not in the subpath of '{}'", real.display(), derived.display()));
    }
    if std::fs::metadata(source).map_err(io)?.uid() != unsafe { libc::getuid() } {
        return Err("foreign wrapper owner".into());
    }
    let folder = source
        .ancestors()
        .skip(1)
        .find(|path| path.parent() == Some(derived.as_path()))
        .ok_or_else(|| "missing Xcode workspace".to_string())?;
    let info = folder.join("info.plist");
    if std::fs::metadata(&info).map_err(io)?.len() > 65536 {
        return Err("oversized Xcode metadata".into());
    }
    let workspace = plist_string(&info, "WorkspacePath")?;
    let suffix = Path::new(&workspace).extension().and_then(|text| text.to_str());
    if !matches!(suffix, Some("xcodeproj") | Some("xcworkspace")) {
        return Err("missing Xcode workspace".into());
    }
    let resolved = std::fs::canonicalize(&workspace).map_err(io)?;
    Ok(resolved.parent().map(|up| up.display().to_string()).unwrap_or_default())
}

/// A string at the top of a property list. There is no plist crate and a file of Xcode's may be XML or binary, so
/// the system's own `plutil` reads it; asking for the one key (and `-expect string`) keeps a date or a data field
/// elsewhere in the file, which `plutil -convert json` refuses, from mattering.
fn plist_string(file: &Path, key: &str) -> Result<String, String> {
    let file = file.to_string_lossy().to_string();
    let done = run("plutil", &["-extract", key, "raw", "-expect", "string", "-o", "-", "--", file.as_str()],
                   &Options::new("gate.plist").timeout(5.0))
        .map_err(|error| error.to_string())?;
    if !done.success() {
        return Err("missing Xcode workspace".into());
    }
    Ok(done.out().strip_suffix('\n').unwrap_or(&done.out()).to_string())
}

/// Whether nothing visible uses the mount. A visible host or file descriptor is enough to refuse; normal Disk
/// Arbitration additionally lets the OS/other clients refuse a busy volume.
pub fn mount_unused(mount: &Value, run: Runner) -> Result<bool, String> {
    let listed = run(&["ps", "-ww", "-axo", "pid=,comm="])?;
    if listed.code != Some(0) {
        return Err("не проверены процессы теста".into());
    }
    let target = mount_text(mount, "target");
    let source = mount_text(mount, "source");
    let prefixes = [format!("{}/", target.trim_end_matches('/')), format!("{}/", source.trim_end_matches('/'))];
    for line in listed.stdout.lines() {
        let mut fields = line.trim().splitn(2, char::is_whitespace);
        let (number, command) = (fields.next().unwrap_or(""), fields.next().map(str::trim_start));
        let Some(command) = command.filter(|_| !number.is_empty() && number.chars().all(|digit| digit.is_ascii_digit()))
        else {
            return Err("не распознан список процессов".into());
        };
        if prefixes.iter().any(|prefix| command.starts_with(prefix.as_str())) {
            return Ok(false);
        }
    }
    let files = run(&["/usr/sbin/lsof", "-n", "-P", "-S2", "+f", "--", target])?;
    if files.code == Some(0) {
        return Ok(false);
    }
    if files.code != Some(1) || !files.stdout.is_empty() || !files.stderr.is_empty() {
        return Err("не проверены открытые файлы подключения".into());
    }
    Ok(true)
}

// ---------------------------------------------------------------------------------------------------------------
// Disk Arbitration

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFRunLoopDefaultMode: *const c_void;
    fn CFURLCreateFromFileSystemRepresentation(allocator: *const c_void, buffer: *const u8, length: isize, directory: u8)
        -> *const c_void;
    fn CFRunLoopGetCurrent() -> *const c_void;
    fn CFRunLoopRunInMode(mode: *const c_void, seconds: f64, return_after_source: u8) -> i32;
    fn CFRelease(value: *const c_void);
}

#[cfg(target_os = "macos")]
#[link(name = "DiskArbitration", kind = "framework")]
unsafe extern "C" {
    fn DASessionCreate(allocator: *const c_void) -> *const c_void;
    fn DADiskCreateFromVolumePath(allocator: *const c_void, session: *const c_void, url: *const c_void) -> *const c_void;
    fn DASessionScheduleWithRunLoop(session: *const c_void, run_loop: *const c_void, mode: *const c_void);
    fn DASessionUnscheduleFromRunLoop(session: *const c_void, run_loop: *const c_void, mode: *const c_void);
    fn DADissenterGetStatus(dissenter: *const c_void) -> i32;
    fn DADiskUnmount(disk: *const c_void, options: u32,
                     callback: extern "C" fn(*const c_void, *const c_void, *mut c_void), context: *mut c_void);
}

/// "The callback has not come".
const NOT_YET: i64 = i64::MIN;

#[cfg(target_os = "macos")]
extern "C" fn unmount_completed(_disk: *const c_void, dissenter: *const c_void, context: *mut c_void) {
    let slot = unsafe { &*(context as *const AtomicI64) };
    let status = if dissenter.is_null() { 0 } else { unsafe { DADissenterGetStatus(dissenter) } };
    slot.store(status as i64, Ordering::SeqCst);
}

/// Public Disk Arbitration, ordinary unmount. No sudo, force or private SPI.
///
/// `Ok(None)` means the callback did not arrive: preserve a pending receipt so the next run of this project cannot
/// race a late daemon completion.
#[cfg(target_os = "macos")]
pub fn unmount_app(target: &str, timeout: f64) -> Result<Option<i32>, String> {
    // Leaked on purpose (eight bytes): a callback that comes after the timeout still finds its slot.
    let slot: &'static AtomicI64 = Box::leak(Box::new(AtomicI64::new(NOT_YET)));
    let bytes = target.as_bytes();
    unsafe {
        let session = DASessionCreate(std::ptr::null());
        let url = CFURLCreateFromFileSystemRepresentation(std::ptr::null(), bytes.as_ptr(), bytes.len() as isize, 1);
        let mut disk: *const c_void = std::ptr::null();
        let mut scheduled: Option<(*const c_void, *const c_void)> = None;
        let outcome = (|| {
            if session.is_null() || url.is_null() {
                return Err("Disk Arbitration не создал сеанс".to_string());
            }
            disk = DADiskCreateFromVolumePath(std::ptr::null(), session, url);
            if disk.is_null() {
                return Err("Disk Arbitration не нашёл подключение".to_string());
            }
            let run_loop = CFRunLoopGetCurrent();
            let mode = kCFRunLoopDefaultMode;
            DASessionScheduleWithRunLoop(session, run_loop, mode);
            scheduled = Some((run_loop, mode));
            DADiskUnmount(disk, 0, unmount_completed, slot as *const AtomicI64 as *mut c_void);
            let deadline = Instant::now() + Duration::from_secs_f64(timeout.max(0.0));
            while slot.load(Ordering::SeqCst) == NOT_YET && Instant::now() < deadline {
                let left = deadline.saturating_duration_since(Instant::now()).as_secs_f64();
                CFRunLoopRunInMode(mode, left.min(0.25), 0);
            }
            let got = slot.load(Ordering::SeqCst);
            Ok(if got == NOT_YET { None } else { Some(got as i32) })
        })();
        if let Some((run_loop, mode)) = scheduled {
            DASessionUnscheduleFromRunLoop(session, run_loop, mode);
        }
        for value in [disk, url, session] {
            if !value.is_null() {
                CFRelease(value);
            }
        }
        outcome
    }
}

#[cfg(not(target_os = "macos"))]
pub fn unmount_app(_target: &str, _timeout: f64) -> Result<Option<i32>, String> {
    Err("Disk Arbitration есть только на macOS".into())
}

// ---------------------------------------------------------------------------------------------------------------
// The cleanup of a test's mounts

pub fn mount_reports() -> PathBuf {
    home().join("turns/mac-tests/mount-cleanup")
}

/// A sha256 digest, hex: the receipt's file is named by the first sixteen letters of the digest of the birth.
fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98,
        0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
        0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8,
        0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819,
        0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
        0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut state: [u32; 8] =
        [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut padded = data.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in padded.chunks(64) {
        let mut w = [0u32; 64];
        for (index, word) in block.chunks(4).enumerate() {
            w[index] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7) ^ w[index - 15].rotate_right(18) ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17) ^ w[index - 2].rotate_right(19) ^ (w[index - 2] >> 10);
            w[index] = w[index - 16].wrapping_add(s0).wrapping_add(w[index - 7]).wrapping_add(s1);
        }
        let mut v = state;
        for index in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let choice = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let one = v[7].wrapping_add(s1).wrapping_add(choice).wrapping_add(K[index]).wrapping_add(w[index]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let majority = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let two = s0.wrapping_add(majority);
            v = [one.wrapping_add(two), v[0], v[1], v[2], v[3].wrapping_add(one), v[4], v[5], v[6]];
        }
        for (slot, add) in state.iter_mut().zip(v) {
            *slot = slot.wrapping_add(add);
        }
    }
    state.iter().map(|word| format!("{word:08x}")).collect()
}

/// What to know at the start of a test to clean up after it: who owns it, for which project, which mounts were
/// there before. Completed receipts are audit history, not a directory to rescan on every test; only uncertain
/// native requests (`*.pending.json`) can block a later run.
pub fn begin_mount_tracking<H: Host + ?Sized>(host: &H, pid: i64, project: &str) -> Result<Value, String> {
    let project = std::fs::canonicalize(project).map_err(|error| error.to_string())?.display().to_string();
    let birth = host.runner_birth(pid)?;
    let mounts = host.app_mounts()?;
    let mut files: Vec<PathBuf> = match std::fs::read_dir(host.mount_reports()) {
        Ok(entries) => entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.file_name().is_some_and(|name| name.to_string_lossy().ends_with(".pending.json")))
            .collect(),
        Err(_) => Vec::new(),
    };
    files.sort();
    for file in files {
        let unreadable = || "не прочитан отчёт об отложенном отключении; новый тест не начат".to_string();
        let report = read_json(&file).ok_or_else(unreadable)?;
        let items = report.get("items").and_then(Value::as_array);
        let valid = report.get("project").is_some_and(Value::is_string)
            && items.is_some_and(|rows| {
                rows.iter().all(|row| {
                    row.is_object() && row.get("target").is_some_and(Value::is_string)
                        && row.get("status").is_some_and(Value::is_string)
                })
            });
        if !valid {
            return Err(unreadable());
        }
        if report["project"].as_str() == Some(project.as_str()) {
            let pending: Vec<&str> = items
                .into_iter()
                .flatten()
                .filter(|row| row["status"] == "pending")
                .filter_map(|row| row["target"].as_str())
                .collect();
            if mounts.iter().any(|row| pending.contains(&mount_text(row, "target"))) {
                return Err("предыдущее отключение этого проекта ещё не подтверждено; новый тест не начат".into());
            }
        }
    }
    Ok(json!({"version": 1, "pid": pid, "birth": birth, "project": project, "baseline": mounts}))
}

/// Whether to write the receipt after a mount (`continue` of the loop in gate.py skips that write).
enum Flow {
    Skip,
    Done,
}

fn set_status(report: &mut Value, index: usize, status: &str) {
    report["items"][index]["status"] = json!(status);
}

/// One mount of `clean_test_mounts`: the body of its `try`.
fn clean_one<H: Host + ?Sized>(
    host: &H, mount: &Value, snapshot: &Value, pid: i64, receipt: &Path, report: &mut Value, item: &mut Option<usize>,
) -> Result<Flow, String> {
    if host.mount_project(mount)? != mount_text(snapshot, "project") {
        return Ok(Flow::Skip);
    }
    let mut checking = mount.clone();
    checking["status"] = json!("checking");
    let list = report["items"].as_array_mut().expect("items");
    list.push(checking);
    let index = list.len() - 1;
    *item = Some(index);
    if !host.mount_unused(mount)? {
        set_status(report, index, "busy");
        return Ok(Flow::Skip);
    }
    // Re-read immediately before the native effect. A changed/reused mount and a changed runner do not inherit the
    // earlier proof.
    let birth = mount_text(snapshot, "birth");
    let still = host.runner_birth(pid)? == birth
        && host.app_mounts()?.iter().any(|row| mount_key(row) == mount_key(mount))
        && host.mount_unused(mount)?;
    if !still {
        set_status(report, index, "changed");
        return Ok(Flow::Skip);
    }
    set_status(report, index, "pending");
    write_json(receipt, report).map_err(|error| error.to_string())?; // durable before async native dispatch
    let code = host.unmount_app(mount_text(mount, "target"))?;
    let present = host.app_mounts()?.iter().any(|row| mount_key(row) == mount_key(mount));
    let status = if code == Some(0) && !present {
        "removed"
    } else if code.is_none() {
        "pending"
    } else {
        "refused"
    };
    set_status(report, index, status);
    report["items"][index]["nativeStatus"] = json!(code);
    Ok(Flow::Done)
}

/// Unmounts the new attachments of this run's project that nothing uses; keeps a receipt of every step.
pub fn clean_test_mounts<H: Host + ?Sized>(host: &H, snapshot: &Value, pid: i64) -> Result<Value, String> {
    let changed = "владелец очистки изменился";
    if snapshot.get("version").and_then(Value::as_i64) != Some(1)
        || snapshot.get("pid").and_then(Value::as_i64) != Some(pid)
        || snapshot.get("birth").and_then(Value::as_str) != Some(host.runner_birth(pid)?.as_str())
    {
        return Err(changed.into());
    }
    let birth = mount_text(snapshot, "birth").to_string();
    let mut report = json!({"pid": pid, "birth": birth, "project": snapshot["project"], "items": []});
    let baseline: Vec<&str> = snapshot["baseline"].as_array().into_iter().flatten().map(|row| mount_text(row, "target")).collect();
    let mounts: Vec<Value> =
        host.app_mounts()?.into_iter().filter(|row| !baseline.contains(&mount_text(row, "target"))).collect();
    if mounts.len() > 16 {
        return Err("слишком много новых подключений: автоматическая очистка отказана".into());
    }
    let receipt = host.mount_reports().join(format!("{pid}-{}.pending.json", &sha256_hex(birth.as_bytes())[..16]));
    for mount in &mounts {
        let mut item: Option<usize> = None;
        match clean_one(host, mount, snapshot, pid, &receipt, &mut report, &mut item) {
            Ok(Flow::Skip) => continue,
            Ok(Flow::Done) => {}
            Err(error) => {
                // Unknown ownership is never grounds to detach a mount. Keep a concrete receipt instead of turning
                // an error into a success.
                let index = match item {
                    Some(index) => {
                        if report["items"][index]["status"] != "pending" {
                            set_status(&mut report, index, "unverified");
                        }
                        index
                    }
                    None => {
                        let mut row = mount.clone();
                        row["status"] = json!("unverified");
                        let list = report["items"].as_array_mut().expect("items");
                        list.push(row);
                        list.len() - 1
                    }
                };
                report["items"][index]["error"] = json!(error);
            }
        }
        write_json(&receipt, &report).map_err(|error| error.to_string())?;
    }
    write_json(&receipt, &report).map_err(|error| error.to_string())?;
    let pending = report["items"].as_array().into_iter().flatten().any(|row| row["status"] == "pending");
    if !pending {
        let name = receipt.file_name().map(|name| name.to_string_lossy().replace(".pending.json", ".json")).unwrap_or_default();
        std::fs::rename(&receipt, receipt.with_file_name(name)).map_err(|error| error.to_string())?;
    }
    Ok(report)
}

// ---------------------------------------------------------------------------------------------------------------
// The calls

/// The options argparse gave `main`: the ones the gate knows, from a call of a newer ios-run.sh that may say more.
#[derive(Default, Debug)]
pub struct Arguments {
    pub action: String,
    pub udid: String,
    pub name: String,
    pub core_id: String,
    pub pid: i64,
    pub project: String,
    pub test_plan: String,
    pub only_testing: Vec<String>,
    pub no_wait: bool,
}

const KNOWN: [(&str, bool); 9] = [
    ("--action", true), ("--udid", true), ("--name", true), ("--core-id", true), ("--pid", true),
    ("--project", true), ("--test-plan", true), ("--only-testing", true), ("--no-wait", false),
];

fn looks_like_option(text: &str) -> bool {
    text.starts_with('-') && text != "-" && !text.contains(' ')
        && !(text[1..].parse::<f64>().is_ok() && text[1..].chars().next().is_some_and(|c| c.is_ascii_digit() || c == '.'))
}

/// `parser.parse_known_args(argv)` of gate.py: unknown words are left alone, a known option wants its value.
pub fn parse_arguments(words: &[String]) -> Result<Arguments, String> {
    let mut out = Arguments::default();
    let mut index = 0;
    while index < words.len() {
        let word = &words[index];
        index += 1;
        if !word.starts_with("--") || word == "--" {
            continue;
        }
        let (given, inline) = match word.split_once('=') {
            Some((name, value)) => (name, Some(value.to_string())),
            None => (word.as_str(), None),
        };
        let exact = KNOWN.iter().find(|(name, _)| *name == given);
        let found = match exact {
            Some(known) => Some(known),
            None => {
                let near: Vec<_> = KNOWN.iter().filter(|(name, _)| name.starts_with(given)).collect();
                match near.len() {
                    0 => None,
                    1 => Some(near[0]),
                    _ => {
                        let names: Vec<&str> = near.iter().map(|(name, _)| *name).collect();
                        return Err(format!("ambiguous option: {given} could match {}", names.join(", ")));
                    }
                }
            }
        };
        let Some(&(name, takes_value)) = found else { continue };
        if !takes_value {
            if inline.is_some() {
                return Err(format!("argument {name}: ignored explicit argument '{}'", inline.unwrap_or_default()));
            }
            out.no_wait = true;
            continue;
        }
        let value = match inline {
            Some(value) => value,
            None => match words.get(index) {
                Some(next) if !looks_like_option(next) => {
                    index += 1;
                    next.clone()
                }
                _ => return Err(format!("argument {name}: expected one argument")),
            },
        };
        match name {
            "--action" => out.action = value,
            "--udid" => out.udid = value,
            "--name" => out.name = value,
            "--core-id" => out.core_id = value,
            "--project" => out.project = value,
            "--test-plan" => out.test_plan = value,
            "--only-testing" => out.only_testing.push(value),
            _ => {
                out.pid = value.trim().trim_start_matches('+').parse().map_err(|_| {
                    format!("argument --pid: invalid int value: '{value}'")
                })?;
            }
        }
    }
    Ok(out)
}

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

pub fn ask_device<H: Host + ?Sized>(host: &H, options: &Arguments) -> i32 {
    if (options.action == "install" || options.action == "launch") && var("CONVEYOR_PROBE").as_deref() != Some("1") {
        host.say(&crate::items::device_modes_text());
        return 2;
    }
    // Said before the build, so a caller that cannot wait learns it now.
    host.queue(&strings(&["peek", "--udid", &options.udid]))
}

pub fn begin_device<H: Host + ?Sized>(host: &H, options: &Arguments) -> i32 {
    let mut arguments = strings(&["probe-begin", "--udid", &options.udid, "--name", &options.name, "--pid",
                                  &options.pid.to_string(), "--what", &options.action]);
    if !options.core_id.is_empty() {
        arguments.extend(strings(&["--core-id", &options.core_id]));
    }
    let tag = host.chat_tag();
    if !tag.is_empty() {
        arguments.extend(strings(&["--tag", &tag]));
    }
    if options.no_wait {
        arguments.push("--no-wait".into());
    }
    if var("CONVEYOR_FORCE").as_deref() == Some("1") {
        arguments.push("--force".into());
    }
    let status = host.queue(&arguments);
    if status == 0 {
        host.say("==> Проба: телефон твой до конца этого запуска; проверка из очереди её заменит. \
                  Проверяющему эту сборку на проверку не отдавай.");
    }
    status
}

pub fn begin_mac_tests<H: Host + ?Sized>(host: &H, options: &Arguments) -> i32 {
    if host.darwin() && options.pid != host.pid() && options.pid != host.parent_pid() {
        host.say("==> Отказ: gate вызван не владельцем тестового запуска.");
        return EXIT_REFUSED;
    }
    let mut tag = host.chat_tag();
    if tag.is_empty() && !options.project.is_empty() {
        let branch = host.branch_of(&options.project);
        tag = if branch.is_empty() {
            Path::new(&options.project).file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default()
        } else {
            branch
        };
    }
    let what = match options.only_testing.first() {
        Some(first) => {
            let mut what = first.split_once('/').map(|(_, after)| after).unwrap_or(first).to_string();
            if options.only_testing.len() > 1 {
                what += &format!(" и ещё {}", options.only_testing.len() - 1);
            }
            what
        }
        None if options.test_plan.is_empty() => "весь план".to_string(),
        None => format!("весь план {}", options.test_plan),
    };
    let mut arguments = strings(&["mac-begin", "--pid", &options.pid.to_string(), "--tag",
                                  if tag.is_empty() { "?" } else { &tag }, "--what", &what]);
    if options.no_wait {
        arguments.push("--no-wait".into());
    }
    let status = host.queue(&arguments);
    if status != 0 || !host.darwin() || options.project.is_empty() {
        return status;
    }
    let tracked = host.locked(&mut |state| {
        let run = state.get("running").filter(|run| run.is_object() && !run.as_object().is_some_and(|map| map.is_empty()));
        match run {
            Some(run) if run.get("pid").and_then(Value::as_i64) == Some(options.pid) => {
                if run.get("mountSnapshot").is_none() {
                    let snapshot = host.begin_mount_tracking(options.pid, &options.project)?;
                    state["running"]["mountSnapshot"] = snapshot;
                }
                Ok(())
            }
            _ => Err("очередь теста сменила владельца".to_string()),
        }
    });
    match tracked {
        Ok(()) => 0,
        Err(error) => {
            host.leave(options.pid);
            host.say(&format!("==> Тест не начат: не подготовлена безопасная очистка подключений: {error}"));
            EXIT_REFUSED
        }
    }
}

pub fn end_mac_tests<H: Host + ?Sized>(host: &H, options: &Arguments) -> i32 {
    if host.darwin() && options.pid != host.pid() && options.pid != host.parent_pid() {
        host.say("==> Отказ: освободить очередь может только владелец тестового запуска.");
        return EXIT_REFUSED;
    }
    let mut status = 0;
    let cleaned = host.locked(&mut |state| {
        let run = state.get("running");
        let snapshot = match run {
            Some(run) if run.get("pid").and_then(Value::as_i64) == Some(options.pid) => run.get("mountSnapshot").cloned(),
            _ => None,
        };
        let Some(snapshot) = snapshot.filter(|snapshot| snapshot.as_object().is_some_and(|map| !map.is_empty())) else {
            return Ok(());
        };
        let report = host.clean_test_mounts(&snapshot, options.pid)?;
        let items = report["items"].as_array().cloned().unwrap_or_default();
        let unfinished = items.iter().filter(|item| item["status"] != "removed").count();
        if unfinished > 0 {
            status = 1;
            host.say(&format!(
                "==> Очистка подключений теста не завершена ({unfinished}); чужие и занятые подключения сохранены. Отчёт: {}",
                host.mount_reports().display()
            ));
        } else if !items.is_empty() {
            host.say(&format!("==> Сняты подключения завершённого теста: {}.", items.len()));
        }
        Ok(())
    });
    if let Err(error) = cleaned {
        status = 1;
        host.say(&format!("==> Ошибка очистки подключений теста, успех не подтверждён: {error}"));
    }
    host.leave(options.pid);
    status
}

/// gate.main: `ios-run-gate VERB TARGET [options]`.
pub fn main_with<H: Host + ?Sized>(host: &H, argv: &[String]) -> i32 {
    if argv.len() < 2 {
        eprintln!("{DOC}");
        return EXIT_REFUSED;
    }
    let (verb, target) = (argv[0].as_str(), argv[1].as_str());
    let options = match parse_arguments(&argv[2..]) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("gate: error: {error}");
            return 2;
        }
    };
    if target == "device" && !options.udid.is_empty() {
        match verb {
            "ask" => return ask_device(host, &options),
            "begin" => return begin_device(host, &options),
            "end" => return host.queue(&strings(&["probe-end", "--udid", &options.udid, "--pid", &options.pid.to_string()])),
            _ => {}
        }
    }
    if target == "mac-tests" {
        match verb {
            "begin" => return begin_mac_tests(host, &options),
            "end" => return end_mac_tests(host, &options),
            _ => {}
        }
    }
    // What this gate does not keep a line for goes ahead.
    0
}

pub fn main(argv: &[String]) -> i32 {
    main_with(&RealHost, argv)
}

// ---------------------------------------------------------------------------------------------------------------
// Tests: gate.py's unit tests (MacMountCleanupTests, tests/queues-tests.py), with a host that answers in place of
// the native layer and of the queues. No native mounts, no native API calls, no queue.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::testing::Sandbox;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::rc::Rc;

    type Native = Box<dyn FnMut(&str) -> Result<Option<i32>, String>>;
    type Cleaner = Box<dyn Fn(&Value, i64) -> Result<Value, String>>;
    type Tracker = Box<dyn Fn(i64, &str) -> Result<Value, String>>;

    /// The made-up native layer, the made-up queue, and what they were asked.
    struct Fake {
        birth: RefCell<String>,
        reports: PathBuf,
        project: String,
        foreign: Value,
        project_error: RefCell<Option<String>>,
        unused: RefCell<VecDeque<bool>>,
        unused_default: Cell<bool>,
        inventories: RefCell<VecDeque<Vec<Value>>>,
        constant: RefCell<Option<Vec<Value>>>,
        unmount: RefCell<Native>,
        unmounted: RefCell<Vec<String>>,
        said: RefCell<Vec<String>>,
        queue_calls: RefCell<Vec<Vec<String>>>,
        queue_status: Cell<i32>,
        state: RefCell<Value>,
        left: Rc<RefCell<Vec<i64>>>,
        darwin: Cell<bool>,
        pid: i64,
        cleaner: RefCell<Option<Cleaner>>,
        tracker: RefCell<Option<Tracker>>,
        steps: Rc<RefCell<Vec<String>>>,
    }

    impl Host for Fake {
        fn runner_birth(&self, _pid: i64) -> Result<String, String> {
            Ok(self.birth.borrow().clone())
        }
        fn mount_reports(&self) -> PathBuf {
            self.reports.clone()
        }
        fn app_mounts(&self) -> Result<Vec<Value>, String> {
            if let Some(always) = self.constant.borrow().clone() {
                return Ok(always);
            }
            self.inventories.borrow_mut().pop_front().ok_or_else(|| "no more inventories".to_string())
        }
        fn mount_project(&self, mount: &Value) -> Result<String, String> {
            if let Some(error) = self.project_error.borrow().clone() {
                return Err(error);
            }
            Ok(if *mount != self.foreign { self.project.clone() } else { "/other".to_string() })
        }
        fn mount_unused(&self, _mount: &Value) -> Result<bool, String> {
            Ok(self.unused.borrow_mut().pop_front().unwrap_or(self.unused_default.get()))
        }
        fn unmount_app(&self, target: &str) -> Result<Option<i32>, String> {
            self.unmounted.borrow_mut().push(target.to_string());
            (self.unmount.borrow_mut())(target)
        }
        fn begin_mount_tracking(&self, pid: i64, project: &str) -> Result<Value, String> {
            match self.tracker.borrow().as_ref() {
                Some(tracker) => tracker(pid, project),
                None => begin_mount_tracking(self, pid, project),
            }
        }
        fn clean_test_mounts(&self, snapshot: &Value, pid: i64) -> Result<Value, String> {
            match self.cleaner.borrow().as_ref() {
                Some(cleaner) => cleaner(snapshot, pid),
                None => clean_test_mounts(self, snapshot, pid),
            }
        }
        fn queue(&self, arguments: &[String]) -> i32 {
            self.steps.borrow_mut().push("queue".into());
            self.queue_calls.borrow_mut().push(arguments.to_vec());
            self.queue_status.get()
        }
        fn locked(&self, change: &mut dyn FnMut(&mut Value) -> Result<(), String>) -> Result<(), String> {
            let mut state = self.state.borrow().clone();
            let outcome = change(&mut state);
            if outcome.is_ok() {
                *self.state.borrow_mut() = state;
            }
            outcome
        }
        fn leave(&self, pid: i64) {
            self.steps.borrow_mut().push("leave".into());
            self.left.borrow_mut().push(pid);
        }
        fn say(&self, text: &str) {
            self.said.borrow_mut().push(text.to_string());
        }
        fn darwin(&self) -> bool {
            self.darwin.get()
        }
        fn pid(&self) -> i64 {
            self.pid
        }
        fn parent_pid(&self) -> i64 {
            1
        }
        fn chat_tag(&self) -> String {
            "тест".into()
        }
    }

    struct Setup {
        _sandbox: Sandbox,
        root: PathBuf,
        project: String,
        host: Fake,
        old: Value,
        own: Value,
        foreign: Value,
        snapshot: Value,
    }

    fn mount(name: &str, dev: i64) -> Value {
        json!({"source": format!("/dd/{name}/.XCInstall/Demo.app/Wrapper"), "target": format!("/X/{name}"),
               "device": dev, "inode": 2, "sourceDevice": 1, "sourceInode": dev + 100})
    }

    fn with(mount: &Value, key: &str, value: Value) -> Value {
        let mut changed = mount.clone();
        changed[key] = value;
        changed
    }

    fn setup(name: &str) -> Setup {
        let sandbox = Sandbox::new(name);
        let root = std::fs::canonicalize(&sandbox.folder).unwrap();
        let pid = std::process::id() as i64;
        let project = root.display().to_string();
        let (old, own, foreign) = (mount("old", 1), mount("own", 2), mount("foreign", 3));
        let snapshot = json!({"version": 1, "pid": pid, "birth": "10:20", "project": project, "baseline": [old]});
        let host = Fake {
            birth: RefCell::new("10:20".into()),
            reports: root.join("receipts"),
            project: project.clone(),
            foreign: foreign.clone(),
            project_error: RefCell::new(None),
            unused: RefCell::new(VecDeque::new()),
            unused_default: Cell::new(true),
            inventories: RefCell::new(VecDeque::new()),
            constant: RefCell::new(None),
            unmount: RefCell::new(Box::new(|_| Ok(Some(0)))),
            unmounted: RefCell::new(Vec::new()),
            said: RefCell::new(Vec::new()),
            queue_calls: RefCell::new(Vec::new()),
            queue_status: Cell::new(0),
            state: RefCell::new(json!({"running": {"pid": pid}})),
            left: Rc::new(RefCell::new(Vec::new())),
            darwin: Cell::new(true),
            pid,
            cleaner: RefCell::new(None),
            tracker: RefCell::new(None),
            steps: Rc::new(RefCell::new(Vec::new())),
        };
        Setup { _sandbox: sandbox, root, project, host, old, own, foreign, snapshot }
    }

    impl Setup {
        fn pid(&self) -> i64 {
            self.host.pid
        }
        fn clean(&self, inventories: Vec<Vec<Value>>) -> Result<Value, String> {
            *self.host.inventories.borrow_mut() = inventories.into();
            clean_test_mounts(&self.host, &self.snapshot, self.pid())
        }
        fn status(report: &Value) -> String {
            report["items"][0]["status"].as_str().unwrap_or("").to_string()
        }
        fn none_unmounted(&self) {
            assert!(self.host.unmounted.borrow().is_empty());
        }
    }

    fn receipts(folder: &Path) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(folder)
            .map(|entries| entries.flatten().map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|e| e == "json")).collect())
            .unwrap_or_default();
        files.sort();
        files
    }

    #[test]
    fn only_new_owned_mount_is_removed_and_receipt_precedes_effect() {
        let s = setup("gate-removed");
        let folder = s.root.join("receipts");
        let own_target = s.own["target"].as_str().unwrap().to_string();
        *s.host.unmount.borrow_mut() = Box::new(move |target| {
            let found = receipts(&folder);
            assert_eq!(found.len(), 1);
            let written: Value = serde_json::from_str(&std::fs::read_to_string(&found[0]).unwrap()).unwrap();
            assert_eq!(written["items"][0]["status"], "pending");
            assert_eq!(target, own_target);
            Ok(Some(0))
        });
        let before = vec![s.old.clone(), s.own.clone(), s.foreign.clone()];
        let result = s.clean(vec![before.clone(), before, vec![s.old.clone(), s.foreign.clone()]]).unwrap();
        let statuses: Vec<&str> = result["items"].as_array().unwrap().iter().map(|row| row["status"].as_str().unwrap()).collect();
        assert_eq!(statuses, ["removed"]);
        assert_eq!(*s.host.unmounted.borrow(), [s.own["target"].as_str().unwrap().to_string()]);
        // The receipt, no longer uncertain, is renamed from `.pending.json` to `.json`, as old Python reads it.
        let names: Vec<String> = receipts(&s.root.join("receipts")).iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect();
        assert_eq!(names.len(), 1);
        assert!(names[0].ends_with(".json") && !names[0].ends_with(".pending.json"), "{names:?}");
    }

    #[test]
    fn preexisting_target_is_preserved_even_if_its_identity_changed() {
        let s = setup("gate-preexisting");
        let changed = with(&s.old, "device", json!(88));
        let result = s.clean(vec![vec![changed]]).unwrap();
        assert_eq!(result["items"], json!([]));
        s.none_unmounted();
    }

    #[test]
    fn recycled_runner_pid_is_refused() {
        let s = setup("gate-recycled");
        *s.host.birth.borrow_mut() = "11:20".into();
        let error = s.clean(vec![]).unwrap_err();
        assert!(error.contains("владелец"), "{error}");
        s.none_unmounted();
    }

    #[test]
    fn foreign_project_is_preserved() {
        let s = setup("gate-foreign");
        assert_eq!(s.clean(vec![vec![s.foreign.clone()]]).unwrap()["items"], json!([]));
        s.none_unmounted();
    }

    #[test]
    fn busy_mount_is_not_dispatched() {
        let s = setup("gate-busy");
        s.host.unused_default.set(false);
        assert_eq!(Setup::status(&s.clean(vec![vec![s.own.clone()]]).unwrap()), "busy");
        s.none_unmounted();
    }

    #[test]
    fn replaced_mount_or_source_is_not_dispatched() {
        let s = setup("gate-replaced");
        for field in ["device", "sourceInode"] {
            let changed = with(&s.own, field, json!(999));
            let result = s.clean(vec![vec![s.own.clone()], vec![changed]]).unwrap();
            assert_eq!(Setup::status(&result), "changed", "{field}");
        }
        s.none_unmounted();
    }

    #[test]
    fn host_appearing_during_validation_preserves_mount() {
        let s = setup("gate-host-appears");
        *s.host.unused.borrow_mut() = [true, false].into();
        let result = s.clean(vec![vec![s.own.clone()], vec![s.own.clone()]]).unwrap();
        assert_eq!(Setup::status(&result), "changed");
        s.none_unmounted();
    }

    #[test]
    fn native_refusal_is_not_success_and_is_not_retried() {
        let s = setup("gate-refusal");
        *s.host.unmount.borrow_mut() = Box::new(|_| Ok(Some(123)));
        let own = s.own.clone();
        let result = s.clean(vec![vec![own.clone()], vec![own.clone()], vec![own]]).unwrap();
        assert_eq!(Setup::status(&result), "refused");
        assert_eq!(result["items"][0]["nativeStatus"], 123);
        assert_eq!(s.host.unmounted.borrow().len(), 1);
    }

    #[test]
    fn unknown_ownership_is_recorded_without_effect() {
        let s = setup("gate-unknown");
        *s.host.project_error.borrow_mut() = Some("missing info.plist".into());
        let result = s.clean(vec![vec![s.own.clone()]]).unwrap();
        assert_eq!(Setup::status(&result), "unverified");
        assert_eq!(result["items"][0]["error"], "missing info.plist");
        s.none_unmounted();
    }

    #[test]
    fn timeout_keeps_pending_receipt_and_blocks_same_project_reentry() {
        let s = setup("gate-timeout");
        *s.host.unmount.borrow_mut() = Box::new(|_| Ok(None));
        let own = s.own.clone();
        let result = s.clean(vec![vec![own.clone()], vec![own.clone()], vec![own.clone()]]).unwrap();
        assert_eq!(Setup::status(&result), "pending");
        assert!(s.root.join("receipts").read_dir().unwrap().flatten().any(|f| f.file_name().to_string_lossy().ends_with(".pending.json")));
        *s.host.constant.borrow_mut() = Some(vec![own]);
        let error = begin_mount_tracking(&s.host, s.pid(), &s.project).unwrap_err();
        assert!(error.contains("предыдущее отключение"), "{error}");
        // A different project, or a mount that is now gone, is not blocked.
        *s.host.constant.borrow_mut() = Some(vec![]);
        assert_eq!(begin_mount_tracking(&s.host, s.pid(), &s.project).unwrap()["baseline"], json!([]));
    }

    #[test]
    fn failure_during_native_dispatch_keeps_uncertain_state() {
        let s = setup("gate-native-lost");
        *s.host.unmount.borrow_mut() = Box::new(|_| Err("native connection lost".into()));
        let own = s.own.clone();
        let result = s.clean(vec![vec![own.clone()], vec![own]]).unwrap();
        assert_eq!(Setup::status(&result), "pending");
        assert_eq!(result["items"][0]["error"], "native connection lost");
    }

    #[test]
    fn unwritable_receipt_prevents_native_dispatch() {
        let s = setup("gate-unwritable");
        // The receipts' folder cannot be made: a file stands where it should be.
        std::fs::write(s.root.join("receipts"), "a file, not a folder").unwrap();
        let own = s.own.clone();
        assert!(s.clean(vec![vec![own.clone()], vec![own]]).is_err());
        s.none_unmounted();
    }

    #[test]
    fn finished_history_is_not_replayed_and_corrupt_pending_state_is_refused() {
        let s = setup("gate-history");
        let folder = s.root.join("receipts");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("old.json"), "malformed old audit record").unwrap();
        *s.host.constant.borrow_mut() = Some(vec![]);
        assert_eq!(begin_mount_tracking(&s.host, s.pid(), &s.project).unwrap()["baseline"], json!([]));
        std::fs::write(folder.join("uncertain.pending.json"), "malformed native receipt").unwrap();
        let error = begin_mount_tracking(&s.host, s.pid(), &s.project).unwrap_err();
        assert!(error.contains("отложенном отключении"), "{error}");
    }

    #[test]
    fn large_unexplained_delta_has_no_effect() {
        let s = setup("gate-large");
        let many: Vec<Value> = (0..17).map(|i| mount(&i.to_string(), i)).collect();
        let error = s.clean(vec![many]).unwrap_err();
        assert!(error.contains("слишком много"), "{error}");
        s.none_unmounted();
    }

    #[test]
    fn gate_holds_turn_until_cleanup_and_releases_it_on_exception() {
        let s = setup("gate-holds-turn");
        *s.host.state.borrow_mut() = json!({"running": {"pid": s.pid(), "mountSnapshot": s.snapshot}});
        let snapshot = s.snapshot.clone();
        let left_before = s.host.left.clone();
        *s.host.cleaner.borrow_mut() = Some(Box::new(move |given, _pid| {
            // The turn is still held: `leave` has not been called yet.
            assert!(left_before.borrow().is_empty());
            assert_eq!(*given, snapshot);
            Err("read failed".into())
        }));
        let code = main_with(&s.host, &strings(&["end", "mac-tests", "--pid", &s.pid().to_string()]));
        assert_eq!(code, 1);
        assert!(s.host.said.borrow().iter().any(|line| line.contains("успех не подтверждён")));
        assert_eq!(*s.host.left.borrow(), [s.pid()]);
    }

    #[test]
    fn foreign_caller_cannot_release_or_clean_another_turn() {
        let s = setup("gate-foreign-caller");
        let code = main_with(&s.host, &strings(&["end", "mac-tests", "--pid", &(s.pid() + 99999).to_string()]));
        assert_eq!(code, EXIT_REFUSED);
        assert!(s.host.left.borrow().is_empty());
    }

    #[test]
    fn snapshot_is_taken_only_after_the_turn_is_acquired() {
        let s = setup("gate-snapshot-order");
        let steps = s.host.steps.clone();
        let snapshot = s.snapshot.clone();
        *s.host.tracker.borrow_mut() = Some(Box::new(move |_pid, _project| {
            // The queue was asked (and gave the turn) before the snapshot.
            assert_eq!(*steps.borrow(), ["queue"]);
            Ok(snapshot.clone())
        }));
        let code = main_with(&s.host, &strings(&["begin", "mac-tests", "--pid", &s.pid().to_string(), "--project", &s.project]));
        assert_eq!(code, 0);
        assert_eq!(s.host.state.borrow()["running"]["mountSnapshot"], s.snapshot);
    }

    #[test]
    fn unreadable_baseline_refuses_test_and_releases_turn() {
        let s = setup("gate-unreadable");
        *s.host.tracker.borrow_mut() = Some(Box::new(|_, _| Err("no mounts".into())));
        let code = main_with(&s.host, &strings(&["begin", "mac-tests", "--pid", &s.pid().to_string(), "--project", &s.project]));
        assert_eq!(code, EXIT_REFUSED);
        assert_eq!(*s.host.left.borrow(), [s.pid()]);
    }

    fn lines_runner<'a>(answers: &'a RefCell<VecDeque<Ran>>, asked: &'a RefCell<Vec<String>>) -> impl Fn(&[&str]) -> Result<Ran, String> + 'a {
        move |argv| {
            asked.borrow_mut().push(argv.join(" "));
            answers.borrow_mut().pop_front().ok_or_else(|| "nothing more to answer".to_string())
        }
    }

    fn ran(code: i32, stdout: &str, stderr: &str) -> Ran {
        Ran { code: Some(code), stdout: stdout.into(), stderr: stderr.into() }
    }

    #[test]
    fn mount_parser_excludes_other_users_non_wrappers_and_writable_mounts() {
        let s = setup("gate-parser");
        let user = s.root.join("user");
        let target = user.join("X").join("12345678-1234-1234-1234-123456789ABC");
        std::fs::create_dir_all(&target).unwrap();
        let source = s.root.join(".XCInstall/Demo.app/Wrapper");
        std::fs::create_dir_all(&source).unwrap();
        let good = format!("{} on {} (nullfs, local, read-only, nobrowse)", source.display(), target.display());
        let name = target.file_name().unwrap().to_string_lossy().to_string();
        let output = [
            good.clone(),
            good.replace("read-only, ", ""),
            good.replace(&target.display().to_string(), &s.root.join("foreign/X").join(&name).display().to_string()),
            good.replace("/.XCInstall/", "/old-app/"),
        ]
        .join("\n");
        let answers = RefCell::new(VecDeque::from([ran(0, &user.join("0").display().to_string(), ""), ran(0, &output, "")]));
        let asked = RefCell::new(Vec::new());
        let parsed = app_mounts(&lines_runner(&answers, &asked)).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["target"], target.display().to_string());
        assert_eq!(parsed[0]["source"], source.display().to_string());
        assert_eq!(asked.borrow()[0], "/usr/bin/getconf DARWIN_USER_DIR");
        assert_eq!(asked.borrow()[1], "/sbin/mount");
    }

    #[test]
    fn source_metadata_resolves_exact_project_and_rejects_symlink_escape() {
        let mut s = setup("gate-metadata");
        let root = s.root.clone();
        s._sandbox.set("HOME", &root.display().to_string());
        let derived = root.join("Library/Developer/Xcode/DerivedData/Example");
        let source = derived.join("Build/Products/Debug-iphoneos/.XCInstall/Demo.app/Wrapper");
        std::fs::create_dir_all(&source).unwrap();
        let project = root.join("project/Example.xcodeproj");
        std::fs::create_dir_all(&project).unwrap();
        // What plistlib.dumps wrote: an XML property list.
        std::fs::write(derived.join("info.plist"), format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
             \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict>\
             <key>LastAccessedDate</key><date>2026-01-01T00:00:00Z</date>\
             <key>WorkspacePath</key><string>{}</string></dict></plist>\n", project.display())).unwrap();
        let given = json!({"source": source.display().to_string()});
        assert_eq!(mount_project(&given).unwrap(), project.parent().unwrap().display().to_string());
        // The same file as a binary property list.
        let binary = derived.join("binary.plist");
        let converted = std::process::Command::new("plutil").args(["-convert", "binary1", "-o"]).arg(&binary)
            .arg(derived.join("info.plist")).status().unwrap();
        assert!(converted.success());
        std::fs::rename(&binary, derived.join("info.plist")).unwrap();
        assert_eq!(mount_project(&given).unwrap(), project.parent().unwrap().display().to_string());
        let outside = root.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::remove_dir(&source).unwrap();
        std::os::unix::fs::symlink(&outside, &source).unwrap();
        assert!(mount_project(&given).is_err());
    }

    #[test]
    fn visible_host_or_file_holder_preserves_attachment() {
        let s = setup("gate-visible");
        let own = s.own.clone();
        let target = own["target"].as_str().unwrap().to_string();
        let asked = RefCell::new(Vec::new());
        let answers = RefCell::new(VecDeque::from([ran(0, &format!("12 {target}/d/Demo\n"), "")]));
        assert!(!mount_unused(&own, &lines_runner(&answers, &asked)).unwrap());
        assert_eq!(asked.borrow().len(), 1);
        let answers = RefCell::new(VecDeque::from([ran(0, "12 /Applications/Other\n", ""), ran(0, "an open descriptor", "")]));
        assert!(!mount_unused(&own, &lines_runner(&answers, &asked)).unwrap());
        let answers = RefCell::new(VecDeque::from([ran(0, "12 /Applications/Other\n", ""), ran(1, "", "incomplete output")]));
        let error = mount_unused(&own, &lines_runner(&answers, &asked)).unwrap_err();
        assert!(error.contains("открытые файлы"), "{error}");
        // Nothing seen and lsof's "nothing open" (exit 1, silent): unused.
        let answers = RefCell::new(VecDeque::from([ran(0, "12 /Applications/Other\n", ""), ran(1, "", "")]));
        assert!(mount_unused(&own, &lines_runner(&answers, &asked)).unwrap());
    }

    // --- what the Python tests did not need to say, but the port has to keep

    #[test]
    fn the_real_system_layer_answers_without_touching_a_mount() {
        // Reads only: the user's folder, the list of mounts; and a folder that is no mount is not found by Disk
        // Arbitration, so nothing is unmounted.
        assert!(app_mounts(&real_run).is_ok());
        let s = setup("gate-real");
        let error = unmount_app(&s.project, 1.0).unwrap_err();
        assert!(error.contains("Disk Arbitration"), "{error}");
    }

    #[test]
    fn sha256_is_the_real_one() {
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(&sha256_hex(b"10:20")[..16].len(), &16);
    }

    #[test]
    fn a_mount_line_is_split_as_the_regular_expression_split_it() {
        let line = "/a/.XCInstall/Demo.app/Wrapper on /var/X/ID (nullfs, local, read-only)";
        assert_eq!(parse_mount_line(line), Some(("/a/.XCInstall/Demo.app/Wrapper", "/var/X/ID", "nullfs, local, read-only")));
        assert_eq!(parse_mount_line("/dev/disk3s1 on / (apfs, local, read-only)"), None);
        assert_eq!(parse_mount_line("/a/.XCInstall/Demo.app/Wrapper on /x (nullfs, )"), None);
        assert_eq!(parse_mount_line("/.XCInstall/Demo.app/Wrapper on /x (nullfs, a)"), None);
    }

    #[test]
    fn the_runner_birth_of_this_process_is_known_and_a_stranger_is_not() {
        let me = std::process::id() as i64;
        let first = runner_birth(me).unwrap();
        assert_eq!(first, runner_birth(me).unwrap());
        assert!(first.contains(':'));
        assert!(runner_birth(0).is_err() && runner_birth(me + 7_000_000).is_err());
    }

    #[test]
    fn too_few_words_is_a_refusal_and_an_unknown_call_goes_ahead() {
        let s = setup("gate-main");
        assert_eq!(main_with(&s.host, &strings(&["begin"])), EXIT_REFUSED);
        assert_eq!(main_with(&s.host, &strings(&["begin", "somewhere-else"])), 0);
        assert_eq!(main_with(&s.host, &strings(&["ask", "device"])), 0); // no --udid: no line kept for it
        assert!(s.host.queue_calls.borrow().is_empty());
    }

    #[test]
    fn a_phone_install_that_did_not_come_through_a_probe_is_told_the_two_ways() {
        let mut s = setup("gate-device");
        s._sandbox.unset("CONVEYOR_PROBE");
        let code = main_with(&s.host, &strings(&["ask", "device", "--udid", "U", "--action", "install"]));
        assert_eq!(code, 2);
        assert!(s.host.said.borrow()[0].contains("hitl iphone … --probe"));
        assert!(s.host.queue_calls.borrow().is_empty());
        s._sandbox.set("CONVEYOR_PROBE", "1");
        assert_eq!(main_with(&s.host, &strings(&["ask", "device", "--udid", "U", "--action", "install"])), 0);
        assert_eq!(s.host.queue_calls.borrow()[0], ["peek", "--udid", "U"]);
    }

    #[test]
    fn a_probe_begins_with_the_chat_tag_and_the_force_it_was_given() {
        let mut s = setup("gate-probe");
        s._sandbox.set("CONVEYOR_FORCE", "1");
        let code = main_with(&s.host, &strings(&["begin", "device", "--udid", "U", "--name", "N", "--pid", "5",
                                                 "--action", "launch", "--core-id", "C", "--no-wait"]));
        assert_eq!(code, 0);
        assert_eq!(s.host.queue_calls.borrow()[0],
                   ["probe-begin", "--udid", "U", "--name", "N", "--pid", "5", "--what", "launch", "--core-id", "C",
                    "--tag", "тест", "--no-wait", "--force"]);
        assert!(s.host.said.borrow()[0].starts_with("==> Проба: телефон твой"));
        assert_eq!(main_with(&s.host, &strings(&["end", "device", "--udid", "U", "--pid", "5"])), 0);
        assert_eq!(s.host.queue_calls.borrow()[1], ["probe-end", "--udid", "U", "--pid", "5"]);
    }

    #[test]
    fn a_mac_test_is_named_by_what_it_runs() {
        let s = setup("gate-mac-begin");
        s.host.darwin.set(false);
        let code = main_with(&s.host, &strings(&["begin", "mac-tests", "--pid", "9", "--test-plan", "Fast",
                                                 "--only-testing", "Target/Case/test", "--only-testing", "Target/Other"]));
        assert_eq!(code, 0);
        assert_eq!(s.host.queue_calls.borrow()[0], ["mac-begin", "--pid", "9", "--tag", "тест", "--what", "Case/test и ещё 1"]);
        main_with(&s.host, &strings(&["begin", "mac-tests", "--pid", "9", "--test-plan", "Fast"]));
        assert_eq!(s.host.queue_calls.borrow()[1][6], "весь план Fast");
        s.host.queue_status.set(75);
        assert_eq!(main_with(&s.host, &strings(&["begin", "mac-tests", "--pid", "9"])), 75);
        assert_eq!(s.host.queue_calls.borrow()[2][6], "весь план");
    }

    #[test]
    fn a_cleaned_run_says_how_many_mounts_it_took_off() {
        let s = setup("gate-end-says");
        *s.host.state.borrow_mut() = json!({"running": {"pid": s.pid(), "mountSnapshot": s.snapshot}});
        *s.host.cleaner.borrow_mut() = Some(Box::new(|_, _| Ok(json!({"items": [{"status": "removed"}, {"status": "removed"}]}))));
        assert_eq!(main_with(&s.host, &strings(&["end", "mac-tests", "--pid", &s.pid().to_string()])), 0);
        assert_eq!(s.host.said.borrow()[0], "==> Сняты подключения завершённого теста: 2.");
        *s.host.cleaner.borrow_mut() = Some(Box::new(|_, _| Ok(json!({"items": [{"status": "busy"}, {"status": "removed"}]}))));
        assert_eq!(main_with(&s.host, &strings(&["end", "mac-tests", "--pid", &s.pid().to_string()])), 1);
        let said = s.host.said.borrow()[1].clone();
        assert!(said.starts_with("==> Очистка подключений теста не завершена (1); чужие и занятые подключения сохранены. Отчёт: "), "{said}");
        assert_eq!(*s.host.left.borrow(), [s.pid(), s.pid()]);
    }
}
