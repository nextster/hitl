//! A check on the phone told to the chat that asked, through hitl's own program: what `hitl iphone --check`
//! leaves (`_queues check`) for a chat of the Codex app and for a chat of Claude, the check ended on the phone as its
//! queue ends it (`verdict::finish`), the messenger the check started (`hitl _tell ID`, handed the phone by the
//! program) giving the phone's verdict to a made-up `codex`, and the answer a chat of Claude has not read said first,
//! as `queue` says it.
//!
//! Nothing here reaches a real chat, app or phone: a made-up home, `codex`, database of Codex's chats and app bundle;
//! no window, no dispatcher, no notification, no `claude`, no OpenCode.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

const UDID: &str = "UDID-E2E";
const THREAD: &str = "019a2b3c-t1";

/// The made-up world, gone with the test.
struct World {
    folder: PathBuf,
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

fn program(path: &Path, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

impl World {
    fn new() -> World {
        let folder = std::env::temp_dir().join(format!("hitl-phone-tell-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        for part in ["home", "hitl", "hitl-home", "tmp", "root", "claude-live", "claude-app", "claude-projects", "Build/Demo.app"] {
            std::fs::create_dir_all(folder.join(part)).unwrap();
        }
        let world = World { folder };
        // The made-up Codex: it writes down every message it is handed.
        program(&world.path("codex"), &format!("#!/bin/sh\nif [ \"$1\" = queue ] && [ \"$2\" = --help ]; then exit 0; fi\n\
                                                printf '%s\\0' \"$@\" >> '{}'\n", world.path("sent").display()));
        // Codex's record of its chats: the one that asks is a chat of its app.
        let made = Command::new("/usr/bin/sqlite3").arg(world.path("state.sqlite"))
            .arg(format!("create table threads (id text, name text, title text, source text); \
                          insert into threads values ('{THREAD}', 'финансы', '', 'vscode');")).status().unwrap();
        assert!(made.success());
        // A built app with the checklist's panel, as the queue's own tests make one.
        let mut binary = vec![0xcf, 0xfa, 0xed, 0xfe];
        binary.extend(vec![0u8; 64]);
        binary.extend(b"--ios-run-checklist\0");
        std::fs::write(world.path("Build/Demo.app/Demo"), binary).unwrap();
        std::fs::write(world.path("Build/Demo.app/Info.plist"), "<plist/>").unwrap();
        // This process reads the same home as the program it runs.
        for (key, value) in world.settings() {
            unsafe { std::env::set_var(key, value) };
        }
        world
    }

    fn path(&self, part: &str) -> PathBuf {
        self.folder.join(part)
    }

    fn settings(&self) -> Vec<(&'static str, String)> {
        let path = |part: &str| self.path(part).display().to_string();
        vec![
            ("PATH", "/usr/bin:/bin".into()), ("HOME", path("home")), ("TMPDIR", path("tmp")), ("HITL_TOOL", "hitl-host".into()),
            ("HITL_COMMAND", "hitl".into()), ("HITL_ROOT", path("root")), ("HITL_HOME", path("hitl-home")),
            // A messenger a check starts is this program, as the file `hitl` would start it.
            ("HITL_ENTRY", env!("CARGO_BIN_EXE_hitl-host").into()),
            ("CONVEYOR_HOME", path("hitl")), ("CONVEYOR_WINDOW", String::new()), ("CONVEYOR_NOTIFY", "0".into()),
            ("CONVEYOR_JOURNAL", "0".into()), ("CONVEYOR_NO_DISPATCHER", "1".into()), ("CONVEYOR_MAC_TEST_SCAN", "0".into()),
            ("CONVEYOR_CODEX", path("codex")), ("CONVEYOR_CODEX_STATE", path("state.sqlite")), ("CONVEYOR_CLAUDE", String::new()),
            ("CONVEYOR_OPENCODE", String::new()), ("CONVEYOR_CLAUDE_LIVE_DIR", path("claude-live")),
            ("CONVEYOR_CLAUDE_SESSIONS_DIR", path("claude-app")), ("CONVEYOR_CLAUDE_PROJECTS_DIR", path("claude-projects")),
            ("CONVEYOR_CLAUDE_APP_RUNNING", "0".into()), ("CONVEYOR_TELL_POLL", "0.01".into()), ("CONVEYOR_TELL_GRACE", "0.05".into()),
            ("CONVEYOR_TELL_RECEIPT_WAIT", "0.3".into()),
        ]
    }

    /// One call of the program, by a chat these settings name.
    fn hitl(&self, chat: &[(&str, &str)], words: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_hitl-host")).args(words).env_clear().envs(self.settings()).envs(chat.iter().copied())
            .output().unwrap()
    }

    /// A check for the phone, queued as `hitl iphone --check` queues it once the build is made; its number.
    fn check(&self, chat: &[(&str, &str)], tag: &str, title: &str) -> String {
        let app = self.path("Build/Demo.app").display().to_string();
        let done = self.hitl(chat, &["_queues", "check", "--udid", UDID, "--name", "Test iPhone", "--app", &app, "--bundle-id",
                                         "dev.example.demo", "--tag", tag, "--branch", "nextster/demo", "--title", title,
                                         "--item", "Открой экран → он открылся", "--item", "Нажми «Бары» → они внизу", "--no-wait"]);
        let said = String::from_utf8_lossy(&done.stderr).to_string();
        assert!(done.status.success(), "{said}");
        let id = String::from_utf8_lossy(&done.stdout).trim().to_string();
        assert!(!id.is_empty() && !id.contains('\n'), "{id:?} {said}");
        id
    }

    fn record(&self, id: &str) -> PathBuf {
        self.path("hitl/tell").join(format!("{id}.json"))
    }

    /// He ends the check on the phone, as the queue ends it with the app's report: one item works, one does not.
    fn end_on_the_phone(&self, id: &str) {
        let report = self.path(&format!("report-{id}.json"));
        std::fs::write(&report, format!("{{\"id\": \"{id}\", \"updatedAt\": {}, \"comment\": \"Общий комментарий\", \"results\": \
                                          [{{\"label\": \"1\", \"state\": \"ok\", \"note\": \"Сработало\"}}, \
                                          {{\"label\": \"2\", \"state\": \"problem\", \"note\": \"Нет кнопки\"}}]}}", hitl_host_core::now())).unwrap();
        let report = hitl_host_core::read_json(&report).unwrap();
        let queue = hitl_queues::state::Queue::named(UDID, "Test iPhone", "");
        queue.locked(|state| {
            let waiting = state["queue"].as_array_mut().unwrap();
            let index = waiting.iter().position(|ticket| ticket["id"] == id).expect("the check waits in the phone's queue");
            let ticket = waiting.remove(index);
            state["active"] = ticket.clone();
            hitl_queues::verdict::finish(&queue, state, ticket, "phone", Some(&report), None, "done");
        });
    }
}

#[test]
fn a_check_on_the_phone_is_told_to_the_chat_that_asked() {
    let world = World::new();
    let codex = [("CODEX_THREAD_ID", THREAD)];

    // A chat of the Codex app queues a check for the phone: it does not wait, and a messenger is started for it.
    let id = world.check(&codex, "бары", "Бары внизу");
    let record = hitl_host_core::read_json(&world.record(&id)).expect("whom to tell is written down");
    assert_eq!((record["phone"].as_str(), record["thread"].as_str(), record["agent"].as_str()), (Some(UDID), Some(THREAD), Some("Codex")));
    assert!(!world.path("sent").exists(), "nothing is told before he answers");

    // He ends it on the phone; the messenger hands the phone's verdict to the chat, once, and is gone.
    world.end_on_the_phone(&id);
    let deadline = Instant::now() + Duration::from_secs(30);
    while (world.record(&id).exists() || !world.path("sent").exists()) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let log = std::fs::read_to_string(world.path("hitl/tell/messenger.log")).unwrap_or_default();
    let sent = std::fs::read_to_string(world.path("sent")).unwrap_or_else(|_| panic!("the messenger told nobody:\n{log}"));
    let words: Vec<&str> = sent.split('\0').filter(|word| !word.is_empty()).collect();
    assert_eq!(words[..3], ["queue", "--thread", THREAD], "{log}");
    assert_eq!(words.len(), 4, "one message: {words:?}");
    let message = words[3];
    assert!(message.starts_with("--message=Ответ с телефона по проверке hitl. «Бары внизу»\n"), "{message}");
    for part in ["1 работает. «Сработало»", "2 не работает. «Нет кнопки»", "Ко всей проверке: Общий комментарий",
                 "Завершено Проверяющим: кнопкой «Готово» на телефоне.",
                 "Служебно: автодоставка hitl, не новая просьба. Уже обработанный итог не исполнять снова; wait и доставку не дублировать."] {
        assert!(message.contains(part), "{part}: {message}");
    }
    assert!(message.ends_with(&format!("\nНомер проверки: {id}.")), "{message}");
    assert!(!world.record(&id).exists());
    assert!(log.contains(&format!("told {THREAD} the verdict of {id}")), "{log}");
    // A messenger started again has nobody left to tell.
    assert!(world.hitl(&codex, &["_tell", &id]).status.success());
    assert_eq!(std::fs::read_to_string(world.path("sent")).unwrap(), sent);

    // A chat of Claude waits for its check itself; who asked is kept, with where the phone's queue keeps the verdict.
    let claude = [("CLAUDECODE", "1"), ("CLAUDE_CODE_HOST_SESSION_ID", "local_1"), ("CLAUDE_CODE_SESSION_ID", "0a1b-session")];
    let asked = world.check(&claude, "бары-claude", "Бары внизу (Claude)");
    let record = hitl_host_core::read_json(&world.record(&asked)).expect("who asked is written down");
    let verdict = world.path("hitl/iphone").join(UDID).join("tickets").join(&asked).join("verdict.json");
    assert_eq!(record["verdict"].as_str(), Some(verdict.display().to_string().as_str()));
    // What `queue` says first to the chat that asked, with the phone handed in as hitl's program hands it.
    hitl::tell::set_phone(hitl::tell::Phone {
        position: |udid, ticket_id| hitl_queues::commands::position_of(&hitl_queues::state::Queue::new(udid).load(), ticket_id),
        verdict_text: hitl_queues::verdict::verdict_text,
    });
    let ids = ["local_1".to_string(), "0a1b-session".to_string()];
    assert!(hitl::tell::reminders(&ids).is_empty(), "nothing to say while it waits on the phone");
    world.end_on_the_phone(&asked);
    let finished = hitl_host_core::read_json(&verdict).unwrap()["finishedAt"].as_f64();
    assert_eq!(hitl::tell::reminders(&ids), [format!("hitl: Проверяющий ответил на проверку «Бары внизу (Claude)» в {}, а этот чат его ответа \
                                                      не читал. Прочти и действуй по нему: hitl wait {asked}", hitl_host_core::clock(finished))]);
    assert!(hitl::tell::reminders(&["local_2".to_string(), "ffff-other".to_string()]).is_empty(), "only the chat that asked");
    // Its messenger, if one runs, has no `claude` to tell it by: the answer stays for the chat to read.
    assert!(world.hitl(&claude, &["_tell", &asked]).status.success());
    assert!(world.record(&asked).exists());
    assert_eq!(std::fs::read_to_string(world.path("sent")).unwrap(), sent);
}
