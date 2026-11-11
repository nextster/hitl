//! Checks of `hitl update`, with a made-up GitHub and a made-up folder of the tools: what main has is put into the
//! folder whole, another chat's uncommitted work is left alone, and the window is not replaced under another chat's
//! check. The command is run through the entry, as a chat runs it; no window is put on any screen.

mod common;

use common::{Temp, hitl, environment, git, read, write};
use serde_json::json;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

struct Made {
    _top: Temp,
    home: PathBuf,
    live: PathBuf,
    work: PathBuf,
}

impl Made {
    fn new() -> Made {
        let top = Temp::new("update");
        let (home, live, work, origin) = (top.0.join("home"), top.0.join("live"), top.0.join("work"), top.0.join("origin.git"));
        std::fs::create_dir_all(&home).unwrap();
        git(&top.0, &["init", "--quiet", "--bare", "--initial-branch=main", origin.to_str().unwrap()]);
        git(&top.0, &["clone", "--quiet", origin.to_str().unwrap(), work.to_str().unwrap()]);
        for (path, text) in [
            ("hitl", "#!/bin/sh\n"),
            ("src/tool.py", "first = 1\n"),
            ("docs/guide.md", "the guide\n"),
            ("window/main.swift", "print(\"first\")\n"),
            ("window/Views.swift", "// views\n"),
        ] {
            write(&work.join(path), text);
        }
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(work.join("hitl"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let made = Made { _top: top, home, live: live.clone(), work };
        made.land("The tools as they were");
        git(&made._top.0, &["clone", "--quiet", origin.to_str().unwrap(), live.to_str().unwrap()]);
        made
    }

    /// Whatever lies in the other clone goes into main on the made-up GitHub.
    fn land(&self, title: &str) -> String {
        git(&self.work, &["add", "--all"]);
        git(&self.work, &["commit", "--quiet", "-m", title]);
        git(&self.work, &["push", "--quiet", "origin", "HEAD:main"]);
        git(&self.work, &["rev-parse", "HEAD"])
    }

    fn env(&self, chat: &str, window: Option<&str>) -> std::collections::BTreeMap<String, String> {
        let mut all = environment();
        for (key, value) in [("CONVEYOR_LIVE", self.live.to_str().unwrap()), ("CONVEYOR_HOME", self.home.to_str().unwrap()), ("CONVEYOR_CHAT", chat), ("CONVEYOR_NOTIFY", "0")] {
            all.insert(key.into(), value.into());
        }
        if let Some(window) = window {
            all.insert("CONVEYOR_WINDOW".into(), window.into());
        }
        all
    }

    fn update(&self, arguments: &[&str]) -> (i32, String) {
        self.update_as("мой чат", Some(""), arguments)
    }

    fn update_as(&self, chat: &str, window: Option<&str>, arguments: &[&str]) -> (i32, String) {
        let mut all = vec!["update"];
        all.extend_from_slice(arguments);
        hitl(&all, &self.env(chat, window))
    }

    /// The window shows a check of this chat, and no other waits.
    fn shows(&self, tag: &str, number: u32) {
        let _ = std::fs::remove_dir_all(self.home.join("queue"));
        std::fs::create_dir_all(self.home.join("queue")).unwrap();
        std::fs::create_dir_all(self.home.join("window")).unwrap();
        let id = format!("1010-120000-00{number}-x");
        let ticket = json!({"id": id, "screen": "mac", "tag": tag, "title": "План", "items": [], "createdAt": 1.0});
        write(&self.home.join("queue").join(format!("{id}.json")), &ticket.to_string());
        write(&self.home.join("window/progress.json"), &json!({"id": id, "marked": 0, "total": 3}).to_string());
    }

    fn live_text(&self, path: &str) -> String {
        read(&self.live.join(path))
    }

    fn head(&self) -> String {
        git(&self.live, &["rev-parse", "HEAD"])
    }
}

#[test]
fn what_main_has_is_put_into_the_folder_whole() {
    let made = Made::new();
    write(&made.work.join("src/tool.py"), "second = 2\n");
    write(&made.work.join("src/new.py"), "new = True\n");
    std::fs::remove_file(made.work.join("docs/guide.md")).unwrap();
    write(&made.work.join("window/main.swift"), "print(\"second\")\n");
    write(&made.work.join("window/Preview.swift"), "// a new file of the window\n");
    std::fs::remove_file(made.work.join("window/Views.swift")).unwrap();
    let merged = made.land("Change the tools and the window");
    let (code, said) = made.update(&["--dry-run"]);
    assert_eq!((code, said.contains("Сухой прогон"), said.contains("окно проверок пересоберётся")), (0, true, true), "{said}");
    assert_eq!(made.live_text("src/tool.py"), "first = 1\n", "a dry run changes nothing");
    let (code, said) = made.update(&[]);
    assert_eq!(code, 0, "{said}");
    assert!(said.contains(&format!("обновлена до {} («Change the tools and the window»): 6 файлов", &merged[..7])), "{said}");
    assert_eq!(made.live_text("src/tool.py"), "second = 2\n");
    assert_eq!(made.live_text("src/new.py"), "new = True\n");
    assert!(!made.live.join("docs/guide.md").exists());
    let mut window: Vec<String> = std::fs::read_dir(made.live.join("window")).unwrap().flatten().map(|entry| entry.file_name().to_string_lossy().to_string()).collect();
    window.sort();
    assert_eq!(window, ["Preview.swift", "main.swift"]);
    use std::os::unix::fs::PermissionsExt;
    assert_ne!(std::fs::metadata(made.live.join("hitl")).unwrap().permissions().mode() & 0o111, 0, "the entry stays executable");
    // The folder is at main as git sees it too, with nothing left over.
    assert_eq!((made.head(), git(&made.live, &["status", "--porcelain"])), (merged, String::new()));
    assert!(std::fs::read_dir(made.live.join(".git")).unwrap().flatten().all(|entry| !entry.file_name().to_string_lossy().starts_with("conveyor-update-")));
    let (code, said) = made.update(&[]);
    assert_eq!((code, said.contains("Обновлять нечего")), (0, true), "{said}");
}

#[test]
fn two_updates_at_once_do_not_mix_one_versions_files_with_the_others() {
    // The audit of 2026-10-16, H05. The part about an update that looked at T1 and placed after another had brought the
    // folder to T2 is a unit test of the crate (`update::tests`): it drives the program's own calls.
    // Two run at once: the second waits for the first, or with --no-wait says so, and nothing is mixed.
    let made = Made::new();
    write(&made.work.join("src/tool.py"), "T3\n");
    let t3 = made.land("T3");
    let held = std::fs::OpenOptions::new().create(true).append(true).open(made.live.join(".git/conveyor-update.lock")).unwrap();
    use std::os::fd::AsRawFd;
    assert_eq!(unsafe { libc::flock(held.as_raw_fd(), libc::LOCK_EX) }, 0);
    let (code, said) = made.update(&["--no-wait"]);
    assert_eq!(code, 75, "{said}");
    assert!(said.contains("обновляет другой hitl update"), "{said}");
    drop(held);
    let (code, said) = made.update(&[]);
    assert_eq!((code, made.head()), (0, t3), "{said}");
}

#[test]
fn another_chats_uncommitted_work_is_not_touched() {
    let made = Made::new();
    write(&made.work.join("src/tool.py"), "second = 2\n");
    let merged = made.land("Change a tool");
    // Work in a file main does not change stays where it lies, and the folder moves on around it.
    write(&made.live.join("docs/guide.md"), "the guide, being rewritten by another chat\n");
    git(&made.live, &["add", "docs/guide.md"]);
    write(&made.live.join("notes.txt"), "a file git does not know\n");
    let (code, said) = made.update(&[]);
    assert_eq!(code, 0, "{said}");
    assert_eq!(made.live_text("src/tool.py"), "second = 2\n");
    assert_eq!(made.head(), merged);
    assert_eq!(git(&made.live, &["status", "--porcelain"]), "M  docs/guide.md\n?? notes.txt", "staged as it was");
    // Work in a file main changes: refused, with the file named, and nothing moved.
    write(&made.work.join("docs/guide.md"), "the guide, as main has it now\n");
    made.land("Change the guide");
    let (code, said) = made.update(&[]);
    assert_eq!(code, 10, "{said}");
    assert!(said.contains("незакоммиченная работа в файлах, которые меняет main: docs/guide.md"), "{said}");
    assert_eq!((made.head(), made.live_text("docs/guide.md")), (merged, "the guide, being rewritten by another chat\n".to_string()));
}

#[test]
fn the_windows_folder_goes_in_whole_or_not_at_all() {
    let made = Made::new();
    write(&made.work.join("window/main.swift"), "print(\"second\")\n");
    write(&made.work.join("src/tool.py"), "second = 2\n");
    made.land("Change the window");
    let before = made.head();
    // Another chat's new file lies in the window's folder: a swap of the folder would lose it.
    write(&made.live.join("window/LivePage.swift"), "// another chat's work\n");
    let (code, said) = made.update(&[]);
    assert_eq!(code, 10, "{said}");
    assert!(said.contains("window/LivePage.swift"), "{said}");
    assert_eq!((made.head(), made.live_text("src/tool.py")), (before, "first = 1\n".to_string()), "nothing of the change went in, the other files neither");
    std::fs::remove_file(made.live.join("window/LivePage.swift")).unwrap();
    let (code, said) = made.update(&[]);
    assert_eq!((code, made.live_text("window/main.swift")), (0, "print(\"second\")\n".to_string()), "{said}");
}

#[test]
fn the_programs_folder_goes_in_whole_or_not_at_all() {
    let made = Made::new();
    write(&made.work.join("rust/one/src/lib.rs"), "// first\n");
    write(&made.work.join("rust/two/src/lib.rs"), "// first\n");
    made.land("A program");
    assert_eq!(made.update(&[]).0, 0);
    // Another chat's work in the program's folder, in a file main does not change: the folder is swapped whole.
    write(&made.live.join("rust/two/src/draft.rs"), "// theirs\n");
    write(&made.work.join("rust/one/src/lib.rs"), "// second\n");
    let merged = made.land("Change the program");
    let (code, said) = made.update(&[]);
    assert_eq!(code, 10, "{said}");
    assert!(said.contains("rust/two/src/draft.rs"), "{said}");
    assert_eq!(made.live_text("rust/one/src/lib.rs"), "// first\n");
    std::fs::remove_file(made.live.join("rust/two/src/draft.rs")).unwrap();
    let (code, said) = made.update(&["--dry-run"]);
    assert!(code == 0 && said.contains("программа пересоберётся"), "{said}");
    let (code, said) = made.update(&[]);
    assert_eq!(code, 0, "{said}");
    assert!(said.contains(&merged[..7]) && said.contains("новая программа собрана заранее"), "{said}");
    assert_eq!(made.live_text("rust/one/src/lib.rs"), "// second\n");
    assert_eq!(made.live_text("rust/two/src/lib.rs"), "// first\n");
}

#[test]
fn the_window_is_not_replaced_under_another_chats_check() {
    let made = Made::new();
    write(&made.work.join("window/main.swift"), "print(\"second\")\n");
    made.land("Change the window");
    let before = made.head();
    made.shows("другой чат", 1);
    let (code, said) = made.update(&["--no-wait"]);
    assert_eq!(code, 75, "{said}");
    assert!(said.contains("проверка другого чата (другой чат «План»)"), "{said}");
    assert_eq!(made.head(), before);
    // The caller's own check is no reason to wait: the chat itself asked for the new window.
    let (code, said) = made.update_as("другой чат · задача", Some(""), &["--no-wait"]);
    assert_eq!((code, made.live_text("window/main.swift")), (0, "print(\"second\")\n".to_string()), "{said}");
    // With --no-wait gone it waits for that check to end, not for the window to be empty: another chat's next check is
    // on the window by then, and comes up in the new one.
    write(&made.work.join("window/main.swift"), "print(\"third\")\n");
    made.land("Change the window again");
    made.shows("третий чат", 1);
    let mut env = made.env("мой чат", Some(""));
    env.insert("CONVEYOR_UPDATE_POLL_SECONDS".into(), "0.1".into());
    let mut waiting = Command::new("sh")
        .args(["-c", "exec \"$0\" update 2>&1"])
        .arg(common::root().join("hitl"))
        .env_clear()
        .envs(&env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut reader = BufReader::new(waiting.stdout.take().unwrap());
    let mut said = String::new();
    while !said.contains("Жду, пока Проверяющий её закончит") {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap() == 0 {
            break;
        }
        said.push_str(&line);
    }
    std::thread::sleep(Duration::from_millis(500)); // several of its looks at the window
    assert!(waiting.try_wait().unwrap().is_none(), "it waits while that check is on the window: {said}");
    assert_eq!(made.live_text("window/main.swift"), "print(\"second\")\n");
    made.shows("четвёртый чат", 2); // he ended it, and the next one is on the window
    let began = Instant::now();
    let code = loop {
        if let Some(status) = waiting.try_wait().unwrap() {
            break status.code();
        }
        assert!(began.elapsed() < Duration::from_secs(30), "it goes on once that check has ended");
        std::thread::sleep(Duration::from_millis(100));
    };
    let mut rest = String::new();
    let _ = std::io::Read::read_to_string(&mut reader, &mut rest);
    assert_eq!((code, made.live_text("window/main.swift")), (Some(0), "print(\"third\")\n".to_string()), "{said}{rest}");
    // A change that leaves the window alone replaces no window, whoever's check is on it.
    write(&made.work.join("src/tool.py"), "second = 2\n");
    made.land("Change a tool");
    let (code, said) = made.update(&["--no-wait"]);
    assert_eq!((code, made.live_text("src/tool.py")), (0, "second = 2\n".to_string()), "{said}");
}

#[test]
fn a_folder_that_is_not_simply_behind_main_is_left_alone() {
    let made = Made::new();
    write(&made.work.join("src/tool.py"), "second = 2\n");
    made.land("Change a tool");
    write(&made.live.join("src/own.py"), "own = True\n");
    git(&made.live, &["add", "src/own.py"]);
    git(&made.live, &["commit", "--quiet", "-m", "A commit made in the folder itself"]);
    let (code, said) = made.update(&[]);
    assert_eq!(code, 10, "{said}");
    assert!(said.contains("есть коммиты, которых нет в main"), "{said}");
    git(&made.live, &["checkout", "--quiet", "-b", "side"]);
    let (code, said) = made.update(&[]);
    assert_eq!((code, said.contains("стоит на ветке side")), (10, true), "{said}");
    let (code, said) = made.update(&["--force"]);
    assert_eq!(code, 64, "{said}");
}

/// The window is built off screen, beside the one that runs, and a window that does not compile leaves the folder as it was.
#[test]
fn the_new_window_is_built_beside_the_one_that_runs() {
    if !cfg!(target_os = "macos") || Command::new("swiftc").arg("--version").output().is_err() {
        return;
    }
    let made = Made::new();
    let running = made.home.join("window/panel/CheckPanel-0000000000000000");
    write(&running, "the window that is up\n");
    write(&made.work.join("window/main.swift"), "print(\"second\")\n");
    made.land("Change the window");
    let (code, said) = made.update_as("мой чат", None, &[]);
    assert_eq!(code, 0, "{said}");
    let built: Vec<String> = std::fs::read_dir(running.parent().unwrap())
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name.starts_with("CheckPanel-"))
        .collect();
    assert!(built.len() == 2 && built.contains(&"CheckPanel-0000000000000000".to_string()), "the one that runs is not taken away: {built:?}");
    // A window that does not compile leaves the folder as it was.
    write(&made.work.join("window/main.swift"), "this is not Swift\n");
    made.land("Break the window");
    let before = made.head();
    let (code, said) = made.update_as("мой чат", None, &[]);
    assert_eq!(code, 10, "{said}");
    assert!(said.contains("не собралось"), "{said}");
    assert_eq!((made.head(), made.live_text("window/main.swift")), (before, "print(\"second\")\n".to_string()));
}
