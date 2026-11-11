//! Checks of `hitl install`, in a made-up home folder: the move of what the tools kept before there was one home,
//! and the settings only he switches. The links and the status line are checked by the crate's own tests
//! (`install::tests`), on the command's own calls. The command is run through the entry with `HOME` set to a made-up
//! folder, so nothing of the real settings, links or `~/.zshenv` is ever touched.

mod common;

use common::{Temp, hitl, environment, read, root, write};
use serde_json::json;
use std::path::{Path, PathBuf};

struct Made {
    _top: Temp,
    home: PathBuf,
    scripts: PathBuf,
}

impl Made {
    fn new() -> Made {
        let top = Temp::new("install");
        let home = top.0.clone();
        let scripts = home.join("Code/scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        Made { _top: top, home, scripts }
    }

    fn install(&self, arguments: &[&str]) -> (i32, String) {
        let mut all = vec!["install"];
        all.extend_from_slice(arguments);
        let mut env = environment();
        env.insert("HOME".into(), self.home.display().to_string());
        // The made-up home is the only one this command may reach: no settings of Claude Code of the real one.
        env.remove("CONVEYOR_CLAUDE_SETTINGS");
        hitl(&all, &env)
    }

    fn builder(&self) {
        write(&self.scripts.join("ios-run.sh"), "#!/bin/bash\nqueue_command=\"$SCRIPT_DIR/hitl\"\n");
    }
}

fn names(folder: &Path) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(folder).unwrap().flatten().map(|entry| entry.file_name().to_string_lossy().to_string()).collect();
    found.sort();
    found
}

/// Codex hands a chat about 40 KB of one call's output, the middle cut out. On 2026-10-12 `hitl` was one file of 74 KB,
/// and it reached 18 of 19 chats of Codex with a hole in it. A file of a skill stays under half of what Codex hands
/// over, so that it comes whole beside what else the chat reads in that call.
#[test]
fn every_file_of_a_skill_is_small_enough_to_reach_a_chat_of_codex_whole() {
    let skills = root().join("skills");
    for folder in std::fs::read_dir(&skills).unwrap().flatten() {
        for file in std::fs::read_dir(folder.path()).unwrap().flatten() {
            let path = file.path();
            if path.extension().is_some_and(|extension| extension == "md") {
                let size = std::fs::metadata(&path).unwrap().len();
                assert!(
                    size <= 20 * 1024,
                    "{} is over 20 KB: move what only one kind of step needs into a file of its own beside it and name it in SKILL.md, as hitl does with window.md and phone.md",
                    path.display()
                );
            }
        }
    }
    let core = read(&skills.join("hitl/SKILL.md")) + &read(&skills.join("hitl/local.md"));
    let mut parts: Vec<String> =
        names(&skills.join("hitl")).into_iter().filter(|name| name.ends_with(".md") && name != "SKILL.md").collect();
    parts.sort();
    assert_eq!(parts, ["explain.md", "local.md", "page.md", "phone.md", "waiting.md", "window.md"]);
    for part in parts {
        // a part neither the core nor the setup's own part (`local.md`) names is a part no chat reads
        assert!(core.contains(&format!("`{part}`")), "{part} is not named in the core");
    }
}

fn old_state(made: &Made) -> (PathBuf, PathBuf) {
    let window = made.home.join(".agent-checks");
    let phones = made.home.join("Library/Developer/Xcode/DerivedData/.ios-run-device");
    std::fs::create_dir_all(window.join("done")).unwrap();
    std::fs::create_dir_all(window.join("queue")).unwrap();
    std::fs::create_dir_all(window.join("panel")).unwrap();
    write(&window.join("panel/CheckPanel-old"), "the old window's program\n");
    write(&window.join("done/a.json"), "{\"id\": \"a\"}");
    write(&window.join("place.json"), "{\"corner\": [900, 700]}");
    write(
        &window.join("history.jsonl"),
        &(json!({"id": "a", "tag": "r/1", "results": [{"state": "ok"}], "how": "done", "finishedAt": 30}).to_string()
            + "\n"
            + &json!({"id": "c", "tag": "r/3", "results": [], "how": "dropped", "finishedAt": 10}).to_string()
            + "\nnot json\n"),
    );
    let phone = phones.join("UDID-1");
    std::fs::create_dir_all(phone.join("tickets/b")).unwrap();
    write(&phone.join("state.json"), &json!({"active": null, "queue": [], "recent": [{"id": "b"}]}).to_string());
    write(&phone.join("history.jsonl"), &(json!({"id": "b", "tag": "r/2", "how": "phone", "marks": {"ok": 2}, "finishedAt": 20}).to_string() + "\n"));
    write(&phones.join("mac-tests/state.json"), "{\"running\": null, \"waiting\": []}");
    write(&phones.join("landing-owner-app/history.jsonl"), "{\"what\": \"#1\", \"landed\": true, \"at\": 5}\n");
    (window, phones)
}

#[test]
fn nothing_is_moved_from_under_a_check_that_waits() {
    let made = Made::new();
    made.builder();
    let (window, phones) = old_state(&made);
    write(&window.join("queue/waiting.json"), "{\"id\": \"waiting\"}");
    let (code, said) = made.install(&[]);
    assert_eq!(code, 1, "{said}");
    assert!(said.contains("состояние не перенесено"), "{said}");
    assert!(window.join("history.jsonl").is_file() && phones.join("UDID-1/state.json").is_file());
    std::fs::remove_file(window.join("queue/waiting.json")).unwrap();
    write(&phones.join("UDID-1/state.json"), &json!({"active": {"id": "x"}, "queue": []}).to_string());
    assert_eq!(made.install(&[]).0, 1);
    assert!(!made.home.join(".conveyor/iphone").exists());
}

/// A chat does not change the settings of its own protection: called with no terminal, as every test is, the
/// switch changes nothing and hands the chat the file he opens.
#[test]
fn a_chat_cannot_switch_the_window_ok_and_is_handed_the_file() {
    let made = Made::new();
    for option in ["--window-ok", "--window-ok-off"] {
        let (code, said) = made.install(&[option]);
        assert_eq!(code, 64, "{said}");
        assert!(said.contains("this is his to run, not a chat's") && said.contains("Nothing was changed."), "{said}");
    }
    assert!(!made.home.join(".claude/settings.json").exists(), "his settings are untouched");
    let (_, said) = made.install(&["--window-ok"]);
    assert!(said.contains("window-ok.command"), "the file he opens is named: {said}");
    assert!(made.home.join(".hitl/window-ok.command").is_file());
}
