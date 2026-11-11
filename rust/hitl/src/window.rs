//! The window on the Mac: its folders, its program (compiled from `window/*.swift`, or shipped built), its process.
//! The port of the window part of `src/hitl/check.py`.
//!
//! Nothing is installed: the window is compiled from window/*.swift into the home's window/panel on first use, or
//! comes built with the program (`window/CheckPanel` beside it, as his team's archive ships it), runs while checks
//! wait and then exits.
#![allow(dead_code)]

use crate::common;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// The checks that wait for the window, a file each.
pub fn queue_folder() -> PathBuf { crate::common::home().join("queue") }
pub fn done_folder() -> PathBuf { crate::common::home().join("done") }
pub fn shots_folder() -> PathBuf { crate::common::home().join("shots") }
/// What the window keeps for itself: its program, its process, what is marked in it, where it stands.
pub fn window_folder() -> PathBuf { crate::common::home().join("window") }
pub fn progress_file() -> PathBuf { window_folder().join("progress.json") }

/// The window's sources, and its program where it comes built.
pub fn sources_folder() -> PathBuf {
    common::root().join("window")
}

/// The files of a folder whose names end so, in the order Python's `sorted(folder.glob("*" + ending))` gives.
fn named(folder: &Path, ending: &str) -> Vec<PathBuf> {
    common::sorted_dir(folder).into_iter()
        .filter(|path| path.file_name().is_some_and(|name| name.to_string_lossy().ends_with(ending)))
        .collect()
}

/// The checks that wait, in their order (check.queued). A check that is on the phone is shown in the window too, but
/// it is the phone's: its own queue keeps it.
pub fn queued() -> Vec<Value> {
    queued_in(&queue_folder())
}

fn queued_in(folder: &Path) -> Vec<Value> {
    let mut tickets: Vec<Value> = named(folder, ".json").iter().filter_map(|path| common::read_json(path))
        .filter(|ticket| ticket.is_object() && common::truthy(ticket.get("id")) && ticket.get("screen") != Some(&json!("iphone")))
        .collect();
    let created = |ticket: &Value| ticket.get("createdAt").and_then(Value::as_f64).unwrap_or(0.0);
    let id = |ticket: &Value| match ticket.get("id") {
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    };
    tickets.sort_by(|left, right| created(left).partial_cmp(&created(right)).unwrap_or(Ordering::Equal).then_with(|| id(left).cmp(&id(right))));
    tickets
}

/// The window's program, built once per version of its source; None when CONVEYOR_WINDOW="" (check.panel_binary).
/// One that comes built with the program is used as it is, and nothing is compiled.
pub fn panel_binary() -> Option<PathBuf> {
    if let Some(named) = common::var("CONVEYOR_WINDOW") {
        return if named.is_empty() { None } else { Some(PathBuf::from(named)) };
    }
    let shipped = sources_folder().join("CheckPanel");
    if shipped.is_file() {
        return Some(shipped);
    }
    match built_panel(&named(&sources_folder(), ".swift"), true) {
        Ok(binary) => Some(binary),
        Err(said) => {
            common::say(&said);
            common::say(&format!("{} check: окно проверок не собралось; проверка стоит в очереди, но показать её нечем.", common::tool()));
            common::exit(common::EXIT_REFUSED)
        }
    }
}

/// The name of the window's program for these sources: CheckPanel- and the first 16 hex digits of a digest of every
/// source's name and text.
pub fn panel_name(sources: &[PathBuf]) -> std::io::Result<String> {
    let mut digest = Sha256::new();
    for source in sources {
        let name = source.file_name().map(|name| name.as_encoded_bytes().to_vec()).unwrap_or_default();
        digest.update(&name);
        digest.update(b"\0");
        digest.update(std::fs::read(source)?);
        digest.update(b"\0");
    }
    let hex: String = digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(format!("CheckPanel-{}", &hex[..16]))
}

/// The window's program for these sources, compiled unless it is there already; or what the compiler said.
///
/// `only`: the programs of other versions go. `hitl update` builds the next version beside the one that runs,
/// before its sources are in place, and keeps both.
pub fn built_panel(sources: &[PathBuf], only: bool) -> Result<PathBuf, String> {
    built_panel_in(&window_folder().join("panel"), sources, only, "swiftc")
}

fn built_panel_in(folder: &Path, sources: &[PathBuf], only: bool, compiler: &str) -> Result<PathBuf, String> {
    let binary = folder.join(panel_name(sources).map_err(|error| error.to_string())?);
    if binary.exists() {
        return Ok(binary);
    }
    std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    common::say("==> Собираю окно проверок (один раз на версию).");
    let temporary = folder.join(format!(".build-{}", std::process::id()));
    let built = Command::new(compiler).arg("-O").args(sources).arg("-o").arg(&temporary).output();
    let failed = match built {
        Ok(done) if done.status.success() => None,
        Ok(done) => Some(String::from_utf8_lossy(&done.stderr).to_string()),
        Err(error) => Some(error.to_string()),
    };
    if let Some(said) = failed {
        let _ = std::fs::remove_file(&temporary);
        let characters: Vec<char> = said.chars().collect();
        return Err(characters[characters.len().saturating_sub(2000)..].iter().collect());
    }
    std::fs::rename(&temporary, &binary).map_err(|error| error.to_string())?;
    if only {
        for old in common::sorted_dir(folder) {
            let other = old.file_name().is_some_and(|name| name.to_string_lossy().starts_with("CheckPanel-"));
            if other && old != binary {
                let _ = std::fs::remove_file(&old);
            }
        }
    }
    Ok(binary)
}

/// The window that is up: its process and the program it runs, or (None, "") (check.panel_process).
///
/// The window writes panel.pid itself. A number left there by a window that is gone may by now belong to another
/// program, so the process is asked its name.
pub fn panel_process() -> (Option<i32>, String) {
    let saved = common::read_json(&window_folder().join("panel.pid"));
    let pid = match &saved {
        Some(Value::Object(map)) => map.get("pid").and_then(Value::as_i64),
        Some(other) => other.as_i64(),
        None => None,
    };
    let Some(pid) = pid else { return (None, String::new()) };
    let Some(done) = common::run(Command::new("ps").args(["-p", &pid.to_string(), "-o", "comm="]), None, 5.0) else {
        return (None, String::new());
    };
    let program = done.stdout.trim().to_string();
    let Ok(pid) = i32::try_from(pid) else { return (None, String::new()) };
    if !program.contains("CheckPanel") {
        return (None, String::new());
    }
    let binary = match &saved {
        Some(Value::Object(map)) => map.get("binary").and_then(Value::as_str).filter(|binary| !binary.is_empty()).map(str::to_string),
        _ => None,
    };
    (Some(pid), binary.unwrap_or(program))
}

/// How many items of the check on the window are answered (check.answered). A window from before 2026-10-11 said
/// that as `marked`, which is now everything the window holds.
pub fn answered(progress: &Value) -> i64 {
    let zero = json!(0);
    let found = progress.get("answered").or_else(|| progress.get("marked")).unwrap_or(&zero);
    found.as_i64().or_else(|| found.as_f64().map(|value| value as i64)).unwrap_or(0)
}

/// `os.path.realpath`: what is there with its links resolved, the rest as written.
fn realpath(path: &Path) -> PathBuf {
    if let Ok(found) = path.canonicalize() {
        return found;
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map(|here| here.join(path)).unwrap_or_else(|_| path.to_path_buf())
    };
    let mut out = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => {
                out.push(other);
                if let Ok(found) = out.canonicalize() {
                    out = found;
                }
            }
        }
    }
    out
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default()
}

/// The name Python gives the error of a signal not sent.
fn error_name(error: &std::io::Error) -> &'static str {
    match error.raw_os_error() {
        Some(libc::ESRCH) => "ProcessLookupError",
        Some(libc::EPERM) => "PermissionError",
        _ => "OSError",
    }
}

/// The window up, the phone's process too on hitl (check.ensure_panel).
pub fn ensure_panel() {
    let Some(binary) = panel_binary() else {
        common::emit("hitl.panel.unavailable", json!({"source": "hitl.lifecycle", "outcome": "unavailable",
                                                      "context": {"reason": "no_binary"}}));
        return;
    };
    let name = file_name(&binary);
    // The same queue is shown by the app on the phone: what carries it there lives beside the window. hitl has no
    // phone yet: «телефон пока можно скипнуть».
    if common::is_host() {
        common::hook("panel", &json!({}), false);
    }
    let (pid, program) = panel_process();
    if let Some(pid) = pid {
        if realpath(Path::new(&program)) == realpath(&binary) {
            common::emit("hitl.panel.reused", json!({"source": "hitl.lifecycle", "subject": pid.to_string(), "outcome": "running",
                                                     "context": {"binary": name}}));
            return;
        }
        // A window of another version is up. The answers live in its memory, so it is replaced only while it holds
        // none; the next call tries again. `marked` is every item that holds an answer, a comment or a screenshot,
        // in the check on the window and in the ones stepped away from: counted for the check on the window alone,
        // it let the window go with his answers to another one (2026-10-11).
        let progress = common::read_json(&progress_file()).filter(Value::is_object).unwrap_or_else(|| json!({}));
        let request = progress.get("id").cloned().unwrap_or_else(|| json!(""));
        let marked = progress.get("marked").cloned();
        if common::truthy(marked.as_ref()) {
            common::emit("hitl.panel.replacement_deferred", json!({
                "source": "hitl.lifecycle", "subject": pid.to_string(), "outcome": "deferred", "request": request,
                "context": {"reason": "marked_answers", "marked": marked, "binary": name}}));
            return;
        }
        common::emit("hitl.panel.replacement_requested", json!({
            "source": "hitl.lifecycle", "subject": pid.to_string(), "outcome": "requested", "request": request,
            "context": {"reason": "binary_changed", "binary": name, "old_binary": file_name(Path::new(&program))}}));
        let sent = if pid > 0 && unsafe { libc::kill(pid, libc::SIGTERM) } == 0 {
            Ok(())
        } else if pid > 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Err(std::io::Error::from_raw_os_error(libc::ESRCH))
        };
        match sent {
            Ok(()) => common::emit("hitl.panel.signal", json!({
                "source": "hitl.lifecycle", "subject": pid.to_string(), "request": request, "outcome": "sent",
                "context": {"signal": "SIGTERM", "reason": "binary_changed"}})),
            Err(error) => common::emit("hitl.panel.signal", json!({
                "source": "hitl.lifecycle", "subject": pid.to_string(), "request": request, "outcome": "error",
                "error": error_name(&error), "context": {"signal": "SIGTERM", "reason": "binary_changed"}})),
        }
        let deadline = common::now() + 3.0;
        while common::now() < deadline && panel_process().0 == Some(pid) {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    let _ = std::fs::create_dir_all(window_folder());
    let log = std::fs::OpenOptions::new().create(true).append(true).open(window_folder().join("panel.log"));
    let (out, errors) = match log.and_then(|file| Ok((file.try_clone()?, file))) {
        Ok((out, errors)) => (Stdio::from(out), Stdio::from(errors)),
        Err(_) => (Stdio::null(), Stdio::null()),
    };
    let mut command = Command::new(&binary);
    command.stdin(Stdio::null()).stdout(out).stderr(errors)
        .env("CONVEYOR_HOME", common::home()).env("CONVEYOR_COMMAND", common::entry())
        .env("CONVEYOR_CHAT_BUTTON", crate::chatbutton::settings_file(&common::env()));
    // Its own session, as Python's start_new_session: the window outlives the command that brought it up.
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let launched = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            common::say(&format!("{} check: окно проверок не запустилось ({}): {error}", common::tool(), binary.display()));
            common::exit(1)
        }
    };
    let launched_pid = launched.id();
    // Reaped when it ends, so that a window that is gone does not stand as a zombie the next look takes for it.
    std::thread::spawn(move || {
        let mut launched = launched;
        let _ = launched.wait();
    });
    common::emit("hitl.panel.launch", json!({"source": "hitl.lifecycle", "subject": launched_pid.to_string(), "outcome": "spawned",
                                             "context": {"reason": "ensure_panel", "binary": name}}));
    // Two chats may get here at once: the window lets only one of itself run.
    let deadline = common::now() + 2.0;
    while common::now() < deadline && panel_process().0.is_none() {
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("hitl-window-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn the_queue_is_in_the_order_the_checks_came() {
        let queue = folder("queue");
        let write = |name: &str, ticket: Value| std::fs::write(queue.join(name), ticket.to_string()).unwrap();
        write("a.json", json!({"id": "b", "createdAt": 5}));
        write("b.json", json!({"id": "a", "createdAt": 5.0}));
        write("c.json", json!({"id": "c", "createdAt": 1.5}));
        write("d.json", json!({"id": "d", "createdAt": 0, "screen": "iphone"}));
        write("e.json", json!({"id": "", "createdAt": 0}));
        write(".f.json", json!({"id": "f"}));
        write("g.txt", json!({"id": "g"}));
        std::fs::write(queue.join("h.json"), "{").unwrap();
        let ids: Vec<String> = queued_in(&queue).iter().map(|ticket| ticket["id"].as_str().unwrap().to_string()).collect();
        assert_eq!(ids, ["f", "c", "a", "b"]);
        let _ = std::fs::remove_dir_all(&queue);
    }

    #[test]
    fn the_window_is_built_once_per_version_of_its_source() {
        let place = folder("build");
        let (first, second) = (place.join("a.swift"), place.join("b.swift"));
        std::fs::write(&first, "x").unwrap();
        std::fs::write(&second, "yz").unwrap();
        let sources = vec![first, second];
        // sha256(b"a.swift\0x\0b.swift\0yz\0"), as Python digests it.
        assert_eq!(panel_name(&sources).unwrap(), "CheckPanel-61fe4a37a0a432e3");
        let panel = place.join("panel");
        // A compiler that fails says why, and leaves nothing behind.
        let compiler = place.join("swiftc");
        std::fs::write(&compiler, "#!/bin/sh\necho \"$@\" >&2\nexit 1\n").unwrap();
        std::fs::set_permissions(&compiler, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let said = built_panel_in(&panel, &sources, true, &compiler.display().to_string()).unwrap_err();
        assert!(said.starts_with("-O ") && said.contains("a.swift") && said.trim_end().ends_with(&format!(".build-{}", std::process::id())));
        assert_eq!(common::sorted_dir(&panel).len(), 0);
        // One that works is kept under the name of the version, and the others go.
        std::fs::write(&compiler, "#!/bin/sh\nfor last; do :; done\necho built > \"$last\"\n").unwrap();
        std::fs::write(panel.join("CheckPanel-old"), "").unwrap();
        let built = built_panel_in(&panel, &sources, true, &compiler.display().to_string()).unwrap();
        assert_eq!(built, panel.join("CheckPanel-61fe4a37a0a432e3"));
        assert_eq!(common::sorted_dir(&panel), vec![built.clone()]);
        // Built already: nothing is compiled.
        assert_eq!(built_panel_in(&panel, &sources, true, "/no/such/compiler").unwrap(), built);
        let _ = std::fs::remove_dir_all(&place);
    }

    #[test]
    fn answers_are_counted_as_the_window_says_them() {
        assert_eq!(answered(&json!({"answered": 2, "marked": 5})), 2);
        assert_eq!(answered(&json!({"marked": 3})), 3);
        assert_eq!(answered(&json!({})), 0);
    }

    #[test]
    fn a_path_is_resolved_as_far_as_it_is_there() {
        let place = folder("real");
        let real = place.canonicalize().unwrap();
        assert_eq!(realpath(&place.join("gone/../x")), real.join("x"));
        assert_eq!(realpath(&place), real);
        let _ = std::fs::remove_dir_all(&place);
    }
}
