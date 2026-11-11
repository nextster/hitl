//! Checks of hitl, the check window for his team: the archive `hitl package build` makes (no Python in it, the
//! window built), its program run as a colleague runs it (a made-up home, no window), and hitl's own window
//! commands going through the same library, with their events in hitl's journal. What were the tests of
//! `tests/hitl-tests.py`.

mod common;

use common::{Folder, hitl, environment, project, run};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static ARCHIVE: OnceLock<PathBuf> = OnceLock::new();

/// The folder of the archive goes when the tests end.
extern "C" fn remove_archive() {
    if let Some(folder) = ARCHIVE.get() {
        let _ = std::fs::remove_dir_all(folder);
    }
}

/// One archive for the whole file, built by the command as a chat runs it: building it is the slow part.
fn tree() -> &'static Path {
    static TREE: OnceLock<PathBuf> = OnceLock::new();
    TREE.get_or_init(|| {
        let folder = ARCHIVE.get_or_init(|| std::env::temp_dir().canonicalize().unwrap().join(format!("hitl-distribution-archive-{}", std::process::id())));
        let _ = std::fs::remove_dir_all(folder);
        std::fs::create_dir_all(folder).unwrap();
        // SAFETY: a function of this file with no arguments, run once at the exit of the process.
        unsafe { libc::atexit(remove_archive) };
        let out = folder.join("out");
        let made = folder.join("made-up");
        let done = hitl(&["package", "build", "--version", "1.2.3", "--out", &out.display().to_string()], &environment(&made), &project());
        assert_eq!(done.code, 0, "{}{}", done.out, done.err);
        let archive = out.join("hitl-1.2.3.tar.gz");
        assert_eq!(done.out, format!("{}\n", archive.display()));
        let unpacked = run(Path::new("/usr/bin/tar"), &["-x", "-z", "-f", &archive.display().to_string(), "-C", &folder.display().to_string()],
                           &environment(&made), &folder);
        assert_eq!(unpacked.code, 0, "{}", unpacked.err);
        folder.join("hitl-1.2.3")
    })
}

fn files(folder: &Path, base: &Path, found: &mut Vec<String>) {
    for entry in std::fs::read_dir(folder).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files(&path, base, found);
        } else {
            found.push(path.strip_prefix(base).unwrap().display().to_string());
        }
    }
}

#[test]
fn the_archive_holds_programs_and_the_shared_skill_and_nothing_of_python_or_his() {
    let tree = tree();
    assert_eq!(std::fs::read_to_string(tree.join("EDITION")).unwrap(), "hitl 1.2.3\n");
    assert_eq!(std::fs::read_to_string(tree.join("README.md")).unwrap(), hitl_distribution::README);
    let mut found = Vec::new();
    files(tree, tree, &mut found);
    let foreign: Vec<&String> = found.iter().filter(|name| name.ends_with(".py") || name.ends_with(".swift")).collect();
    assert!(foreign.is_empty(), "{found:?}");
    for program in [tree.join("hitl"), tree.join("window/CheckPanel")] {
        let kind = run(Path::new("/usr/bin/file"), &[&program.display().to_string()], &environment(&tree.join("../made-up")), tree);
        assert!(kind.out.contains("Mach-O 64-bit executable arm64"), "{}", kind.out);
    }
    let mut skill: Vec<String> = std::fs::read_dir(tree.join("skills/hitl"))
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name.ends_with(".md"))
        .collect();
    skill.sort();
    assert_eq!(skill, ["SKILL.md", "explain.md", "page.md", "waiting.md", "window.md"]);
    for name in &skill {
        let text = std::fs::read_to_string(tree.join("skills/hitl").join(name)).unwrap();
        for his in ["/Users/example", "hitl merge", "hitl iphone"] {
            assert!(!text.contains(his), "{name} names {his}");
        }
    }
    assert!(tree.join("skills/hitl/pages").is_dir(), "the skill's own pages come with it");
}

#[test]
fn the_archive_is_owned_by_nobody_and_dated_nowhere() {
    let archive = tree().parent().unwrap().join("out/hitl-1.2.3.tar.gz");
    let bytes = std::fs::read(&archive).unwrap();
    assert_eq!(&bytes[..4], &[0x1f, 0x8b, 8, 0], "gzip with no name");
    assert_eq!(&bytes[4..8], &[0, 0, 0, 0], "gzip with no date");
    let listed = run(Path::new("/usr/bin/tar"), &["-t", "-v", "-z", "-f", &archive.display().to_string()], &environment(&tree().join("../made-up")), tree());
    assert_eq!(listed.code, 0, "{}", listed.err);
    let lines: Vec<&str> = listed.out.lines().collect();
    assert!(lines[0].ends_with(" hitl-1.2.3/"), "the folder first: {}", lines[0]);
    for line in &lines {
        let words: Vec<&str> = line.split_whitespace().collect();
        assert_eq!((words[2], words[3]), ("0", "0"), "{line}");
        assert!(line.contains(" 1970 "), "{line}");
    }
    assert!(lines.iter().any(|line| line.ends_with(" hitl-1.2.3/hitl") && line.starts_with("-rwx")), "{}", listed.out);
}

#[test]
fn a_version_that_is_not_one_is_refused() {
    let folder = Folder::new("version");
    let environment = environment(&folder.0);
    let done = hitl(&["package", "build", "--version", "1.2", "--out", &folder.join("out").display().to_string()], &environment, &project());
    assert_eq!((done.code, done.err.as_str()), (1, "hitl package build: версия '1.2': нужна вида 0.1.0\n"));
    assert!(!folder.join("out").exists());
    let done = hitl(&["package"], &environment, &project());
    assert_eq!(done.code, 0);
    assert!(done.out.starts_with("Usage:\n  hitl package build [--version V] [--out DIR]"), "{}", done.out);
    let done = hitl(&["package", "release"], &environment, &project());
    assert_eq!(done.code, 64);
    assert!(done.err.contains("hitl package release V"), "{}", done.err);
}

// ------------------------------------------------------------------------------------------------ as a colleague

/// The program of the archive as a colleague runs it: a home of its own, no window on the screen, no chat app.
struct Colleague {
    folder: Folder,
    environment: std::collections::BTreeMap<String, String>,
}

impl Colleague {
    fn new() -> Colleague {
        let folder = Folder::new("colleague");
        let mut all = std::collections::BTreeMap::new();
        for name in ["PATH", "LANG"] {
            if let Ok(value) = std::env::var(name) {
                all.insert(name.to_string(), value);
            }
        }
        std::fs::create_dir_all(folder.join("tmp")).unwrap();
        for (name, value) in [
            ("HOME", folder.0.display().to_string()),
            ("HITL_HOME", folder.join(".hitl").display().to_string()),
            ("CONVEYOR_HOME", folder.join(".conveyor").display().to_string()),
            ("TMPDIR", folder.join("tmp").display().to_string()),
            ("HITL_ROOT", tree().display().to_string()),
            ("CONVEYOR_WINDOW", String::new()),
            ("CONVEYOR_NOTIFY", "0".into()),
            ("CONVEYOR_CHAT", "Проба hitl".into()),
            ("CONVEYOR_CODEX", String::new()),
            ("CONVEYOR_CLAUDE", String::new()),
            ("CONVEYOR_OPENCODE", String::new()),
        ] {
            all.insert(name.to_string(), value);
        }
        Colleague { folder, environment: all }
    }

    fn hitl(&self, arguments: &[&str]) -> (i32, String) {
        let done = run(&tree().join("hitl"), arguments, &self.environment, &self.folder.0);
        (done.code, done.out + &done.err)
    }

    fn home(&self) -> &Path {
        &self.folder.0
    }

    fn queued(&self) -> Vec<PathBuf> {
        std::fs::read_dir(self.home().join(".hitl/queue"))
            .map(|entries| entries.flatten().map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|ext| ext == "json")).collect())
            .unwrap_or_default()
    }
}

#[test]
fn setup_keeps_the_name_and_puts_the_skill_where_claude_and_codex_find_it() {
    let her = Colleague::new();
    assert_eq!(her.hitl(&["--version"]), (0, "hitl 1.2.3\n".to_string()));
    assert_eq!(her.hitl(&["setup"]).0, 64);
    let (code, said) = her.hitl(&["setup", "--name", "Никита"]);
    assert_eq!(code, 0, "{said}");
    let person: Value = serde_json::from_str(&std::fs::read_to_string(her.home().join(".hitl/person.json")).unwrap()).unwrap();
    assert_eq!(person, json!({"name": "Никита", "genitive": "Никиты", "instrumental": "Никитой", "she": false}));
    for base in [".agents/skills", ".claude/skills"] {
        assert_eq!(std::fs::read_link(her.home().join(base).join("hitl")).unwrap(), tree().join("skills/hitl"));
    }
    assert!(!said.contains("swiftc"), "the window comes built");
    std::fs::remove_file(her.home().join(".claude/skills/hitl")).unwrap();
    std::fs::create_dir(her.home().join(".claude/skills/hitl")).unwrap();
    let (code, said) = her.hitl(&["setup"]);
    assert_eq!(code, 0, "{said}");
    assert!(said.contains("не трогаю"), "{said}");
}

#[test]
fn a_check_is_queued_listed_answered_in_her_name_and_taken_back() {
    let her = Colleague::new();
    her.hitl(&["setup", "--name", "Анна", "--she"]);
    let (code, said) = her.hitl(&["check", "--title", "Проба", "--kind", "plan", "--about", "Проверяю hitl из архива.", "--check", "1. Работает?", "--no-wait"]);
    assert_eq!(code, 75, "{said}");
    assert!(said.contains("Итог: hitl wait"), "{said}");
    let queued = her.queued();
    assert_eq!(queued.len(), 1, "{queued:?}");
    let ticket = &queued[0];
    let (code, said) = her.hitl(&["queue"]);
    assert_eq!(code, 0, "{said}");
    assert!(said.contains("«Проба»"), "{said}");
    assert!(!said.contains("Телефон"), "{said}");
    // The window writes what she answered; the wait prints it, in her name.
    let kept: Value = serde_json::from_str(&std::fs::read_to_string(ticket).unwrap()).unwrap();
    let mut verdict = kept.clone();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64();
    verdict["how"] = json!("done");
    verdict["finishedAt"] = json!(now);
    verdict["results"] = json!([{"label": "1", "text": "Работает?", "state": "ok"}]);
    std::fs::create_dir_all(her.home().join(".hitl/done")).unwrap();
    std::fs::write(her.home().join(".hitl/done").join(ticket.file_name().unwrap()), verdict.to_string()).unwrap();
    std::fs::remove_file(ticket).unwrap();
    let (code, said) = her.hitl(&["wait", kept["id"].as_str().unwrap()]);
    assert_eq!(code, 0, "{said}");
    assert!(said.contains("закончила проверку сама Анна: это её «ок» делать по этому плану"), "{said}");
    her.hitl(&["check", "--title", "Вторая", "--kind", "question", "--about", "Проверяю снятие.", "--check", "1. Снять?", "--no-wait"]);
    assert_eq!(her.queued().len(), 1);
    assert_eq!(her.hitl(&["drop"]).0, 0);
    assert!(her.queued().is_empty());
}

#[test]
fn what_is_conveyors_alone_is_not_there() {
    let her = Colleague::new();
    for command in ["merge", "iphone", "phone", "chats", "update", "install", "hitl"] {
        let (code, said) = her.hitl(&[command]);
        assert_eq!(code, 64, "{command}: {said}");
        assert!(said.contains("hitl --help"), "{command}: {said}");
    }
    let (_, said) = her.hitl(&["check", "--help"]);
    assert!(said.contains("hitl check --title"), "{said}");
    let (code, said) = her.hitl(&["check", "--title", "Слово", "--kind", "plan", "--about", "Проверяю слова.", "--check", "1. Посадка ветки в main", "--no-wait"]);
    assert_eq!(code, 75, "the words he tripped on are his, not his team's: {said}");
}

// ------------------------------------------------------------------------------------------------ hitl's own

fn journal_rows(state: &Path) -> Vec<(String, String, String, String, String)> {
    let database = state.join("journal/events.sqlite3");
    let connection = rusqlite::Connection::open(&database).unwrap_or_else(|error| panic!("{}: {error}", database.display()));
    let mut statement = connection.prepare("SELECT kind, source, request, context, process FROM events WHERE kind LIKE 'hitl.%' ORDER BY id").unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn conveyor_check_goes_through_hitl_speaks_as_conveyor_and_journals_in_its_own_process() {
    let folder = Folder::new("conveyor-check");
    let mut environment = environment(&folder.0);
    environment.insert("CONVEYOR_CHAT".into(), "r/66 · проба".into());
    environment.remove("CONVEYOR_JOURNAL");
    let done = hitl(&["check", "--title", "Проба", "--kind", "plan", "--about", "Проверяю, что hitl идёт через hitl.", "--check", "1. Посадка ветки", "--no-wait"],
                        &environment, &folder.0);
    assert_eq!(done.code, 64, "{}", done.err);
    assert!(done.err.contains("hitl check:"), "{}", done.err);
    assert!(done.err.contains("влить в main"), "{}", done.err);
    let done = hitl(&["check", "--title", "Проба", "--kind", "plan", "--about", "Проверяю, что hitl идёт через hitl.", "--check", "1. Работает?", "--no-wait"],
                        &environment, &folder.0);
    assert_eq!(done.code, 75, "{}", done.err);
    assert!(done.err.contains("Итог: hitl wait"), "{}", done.err);
    let queued: Vec<PathBuf> = std::fs::read_dir(folder.join("state/queue")).unwrap().flatten().map(|entry| entry.path()).collect();
    assert_eq!(queued.len(), 1, "{queued:?}");
    // The event of the queued check is in hitl's journal, written by the process that queued it: the number of
    // the check carries that process's id, and no program was started to hand the event over.
    let ticket = queued[0].file_stem().unwrap().to_string_lossy().to_string();
    let rows = journal_rows(&folder.join("state"));
    let queued_rows: Vec<_> = rows.iter().filter(|row| row.0 == "hitl.queued").collect();
    assert_eq!(queued_rows.len(), 1, "{rows:?}");
    let (_, source, request, context, process) = queued_rows[0];
    assert_eq!((source.as_str(), request.as_str(), context.as_str()), ("hitl", ticket.as_str(), r#"{"kind":"plan","items":1}"#));
    let pid: u32 = process.split('-').next().unwrap().parse().unwrap();
    let field = ticket.split('-').nth(2).unwrap();
    assert_eq!(format!("{:03}", pid % 1000), field, "the event was written by the process of the check ({process}, {ticket})");
}

#[test]
fn the_hook_of_an_older_hitl_still_reaches_the_journal() {
    let folder = Folder::new("hook");
    let mut environment = environment(&folder.0);
    environment.remove("HITL_HOME");
    environment.remove("CONVEYOR_JOURNAL");
    let events = json!([{"kind": "hitl.dropped", "at": 1700000000.5, "request": "1030-1-chat", "source": "hitl", "outcome": "dropped", "words": "x"}]);
    let done = std::process::Command::new(project().join("hitl"))
        .args(["_hitl", "journal"])
        .env_clear()
        .envs(&environment)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child.stdin.take().unwrap().write_all(events.to_string().as_bytes())?;
            child.wait_with_output()
        })
        .unwrap();
    assert_eq!(done.status.code(), Some(0), "{}", String::from_utf8_lossy(&done.stderr));
    assert_eq!(String::from_utf8_lossy(&done.stdout), "{}\n");
    let rows = journal_rows(&folder.join("state"));
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!((rows[0].0.as_str(), rows[0].1.as_str(), rows[0].2.as_str()), ("hitl.dropped", "hitl", "1030-1-chat"));
}
