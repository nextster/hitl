//! `hitl queue`, `wait`, `drop` and `last-check` through the entry, on a made-up home: each is answered, none is
//! refused as a command nobody has. They went unported once (Python's checks.py) and every chat took its verdict
//! with them; this holds them in place.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn entry() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../hitl").canonicalize().expect("entry")
}

fn run(home: &Path, words: &[&str]) -> Output {
    Command::new(entry()).args(words).env("HITL_HOME", home).env("CONVEYOR_HOME", home)
        .env("CONVEYOR_WINDOW", "").env("CONVEYOR_NOTIFY", "0").env("CONVEYOR_NO_MESSENGER", "1")
        .env_remove("T3CODE_THREAD_ID").env("CONVEYOR_CHAT", "Проверка очереди")
        .env("CONVEYOR_JOURNAL", "0").stdin(Stdio::null()).output().expect("hitl")
}

fn said(output: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
}

#[test]
fn queue_wait_drop_and_last_check_are_answered() {
    let folder = std::env::temp_dir().join(format!("conveyor-checks-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let home = folder.canonicalize().unwrap();
    let queued = run(&home, &["check", "--title", "Проба", "--kind", "question", "--about", "Проба очереди на выдуманном доме.",
                              "--check", "1. Работает?", "--no-wait"]);
    let text = said(&queued);
    let id = text.split_whitespace().find(|word| word.len() > 16 && word.as_bytes()[4] == b'-' && word[..4].chars().all(|c| c.is_ascii_digit()))
        .map(|word| word.trim_end_matches(['.', ',', '»']).to_string()).expect(&text);
    for words in [vec!["queue"], vec!["last-check", "--branch", "nope"]] {
        let output = run(&home, &words);
        assert!(!said(&output).contains("нет команды"), "{words:?}: {}", said(&output));
    }
    assert!(said(&run(&home, &["queue"])).contains("Проба"));
    let dropped = run(&home, &["drop", &id]);
    assert!(said(&dropped).contains("снята"), "{}", said(&dropped));
    let waited = run(&home, &["wait", &id]);
    assert_eq!(waited.status.code(), Some(2), "{}", said(&waited));
    let last = run(&home, &["last-check", "--branch", "nope"]);
    assert_eq!((last.status.code(), String::from_utf8_lossy(&last.stdout).trim().to_string()), (Some(1), "null".to_string()));
    std::fs::remove_dir_all(&home).ok();
}

#[test]
fn the_check_button_opens_desktop_and_terminal_chats_in_their_own_app() {
    use std::os::unix::fs::PermissionsExt;

    let folder = std::env::temp_dir().join(format!("conveyor-check-chat-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    let database = folder.join("codex.sqlite");
    let made = Command::new("/usr/bin/sqlite3").arg(&database).arg(
        "create table threads (id text, cwd text, source text, originator text);
         insert into threads values ('desktop', '', 'vscode', 'Codex Desktop'),
                                    ('terminal', '', 'vscode', 'codex-tui');"
    ).status().unwrap();
    assert!(made.success());
    let program = |name: &str, body: &str| {
        let path = folder.join(name);
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    };
    program("open", "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$CONVEYOR_HOME/open.log\"\n");
    let ps = program("fake-ps", "#!/bin/sh\necho '202 ttys999 /opt/homebrew/bin/codex resume terminal'\n");
    let osa = program("fake-osascript", "#!/bin/sh\nprintf '%s\\n' \"$2\" > \"$CONVEYOR_HOME/iterm.log\"\necho found\n");
    let reveal = |id: &str| Command::new(env!("CARGO_BIN_EXE_hitl-host")).args(["chat", "reveal", "Codex", id])
        .env_clear().env("PATH", format!("{}:/usr/bin:/bin", folder.display()))
        .env("HITL_TOOL", "hitl-host").env("CONVEYOR_HOME", &folder).env("CONVEYOR_WINDOW", "")
        .env("CONVEYOR_CODEX_STATE", &database).env("CONVEYOR_PS", &ps).env("CONVEYOR_OSASCRIPT", &osa)
        .output().unwrap();
    let desktop = reveal("desktop");
    assert!(desktop.status.success(), "{}", said(&desktop));
    assert_eq!(std::fs::read_to_string(folder.join("open.log")).unwrap(), "-b\ncom.openai.codex\ncodex://threads/desktop\n");
    assert!(!folder.join("iterm.log").exists());

    let terminal = reveal("terminal");
    assert!(terminal.status.success(), "{}", said(&terminal));
    assert!(std::fs::read_to_string(folder.join("iterm.log")).unwrap().contains("tty of s is \"/dev/ttys999\""));

    std::fs::create_dir_all(folder.join("helpers/legacy")).unwrap();
    std::fs::write(folder.join("helpers/legacy/record.json"), "{}").unwrap();
    std::fs::remove_file(folder.join("iterm.log")).unwrap();
    assert!(!reveal("legacy").status.success());
    assert!(folder.join("helpers/legacy/record.json").exists());
    assert!(!folder.join("iterm.log").exists());
    std::fs::remove_dir_all(&folder).unwrap();
}
