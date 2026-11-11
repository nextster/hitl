//! How the window names the chat a check came from, through the command: `check --presentation` and the ticket that
//! `check` queues. The port of the command-level parts of `HeaderNames` of `tests/check-tests.py`; how a chat is
//! looked up (aliases, foreign folders, ambiguity, Codex and OpenCode, corrupt names) is held by the unit tests of
//! `src/names.rs`.

mod common;

use common::{World, contains};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The world of `HeaderNames.setUp`: a chat of the Claude app, owner-chat, run by the command under the alias
/// current-alias in a folder of its own, and the name hitl keeps for it, «roadmap». Codex and OpenCode have no
/// database here, and every program that could be asked about a chat only writes down that it was.
struct Names {
    world: World,
    folder: PathBuf,
    record: PathBuf,
    native: Value,
}

impl Names {
    fn new(name: &str) -> Names {
        let mut world = World::new(name);
        let folder = world.root.join("project");
        std::fs::create_dir_all(&folder).unwrap();
        let root = world.root.clone();
        for (key, value) in [("CONVEYOR_CLAUDE_SESSIONS_DIR", "desktop"), ("CONVEYOR_CLAUDE_PROJECTS_DIR", "projects"),
                             ("CONVEYOR_CODEX_STATE", "absent.sqlite"), ("CONVEYOR_OPENCODE_STATE", "absent-oc.sqlite")] {
            world.set(key, root.join(value).display().to_string());
        }
        // A lookup asks no program: one that is asked says so.
        let called = root.join("called");
        for program in ["claude", "codex", "opencode", "osascript"] {
            let path = world.program(&format!("bin/{program}"), &format!("#!/bin/sh\necho {program} \"$@\" >> '{}'\n", called.display()));
            if program != "osascript" {
                world.set(&format!("CONVEYOR_{}", program.to_uppercase()), path.display().to_string());
            }
        }
        world.set("PATH", format!("{}:/usr/bin:/bin", root.join("bin").display()));
        let record = root.join("desktop/a/b/owner.json");
        let native = json!({"sessionId": "owner-chat", "cliSessionId": "current-alias", "title": "native", "cwd": folder.display().to_string()});
        let names = Names { world, folder, record, native };
        names.save_native();
        let key = format!("[{}, {}]", json!("Claude"), json!("owner-chat"));
        let digest: String = Sha256::digest(key.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect();
        let kept = json!({"version": 1, "app": "Claude", "id": "owner-chat", "mode": "manual", "revision": 1, "generation": 0,
                          "title": "roadmap", "decisions": {}});
        names.world.file(&format!("hitl/chats/names/{digest}.json"), kept.to_string());
        names
    }

    fn save_native(&self) {
        std::fs::create_dir_all(self.record.parent().unwrap()).unwrap();
        std::fs::write(&self.record, self.native.to_string()).unwrap();
    }

    fn ticket(&self, change: Value) -> Value {
        let folder = self.folder.display().to_string();
        let mut ticket = json!({"id": "fixture-check", "agent": "Claude", "chat": "old label", "chatIds": ["owner-chat"], "cwd": folder,
                                "project": folder, "createdAt": 1, "tag": "", "title": "Fixture", "items": []});
        for (key, value) in change.as_object().unwrap() {
            ticket[key] = value.clone();
        }
        ticket
    }

    /// What `check --presentation` says of these checks.
    fn labels(&self, tickets: &[Value]) -> (String, Vec<Value>) {
        let done = self.world.run_fed(&["check", "--presentation"], Value::Array(tickets.to_vec()).to_string().as_bytes());
        assert_eq!(done.code, 0, "{}", done.err);
        let labels: Value = serde_json::from_str(&done.out).unwrap_or_else(|error| panic!("{error}: {}", done.out));
        (done.out.clone(), labels.as_array().unwrap().clone())
    }

    /// Every file of the world and what it holds.
    fn files(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        fn walk(folder: &std::path::Path, found: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in std::fs::read_dir(folder).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, found);
                } else {
                    found.insert(path.clone(), std::fs::read(&path).unwrap());
                }
            }
        }
        let mut found = BTreeMap::new();
        walk(&self.world.root, &mut found);
        found
    }
}

#[test]
fn shared_name_and_exact_link_without_mutation() {
    // test_shared_name_and_exact_link_without_mutation, test_display_lookup_never_renames_or_invokes_a_model and
    // test_presentation_command_has_no_queue_or_panel_lifecycle: the window asks, and nothing is written, renamed,
    // queued or started for it.
    let names = Names::new("names-shared");
    let before = names.files();
    let (said, labels) = names.labels(&[names.ticket(json!({}))]);
    assert_eq!(said, "[{\"chat\": \"roadmap\", \"reference\": \"conveyor://chat/Claude/owner-chat\", \"status\": \"active\"}]\n");
    assert_eq!(labels[0], json!({"chat": "roadmap", "reference": "conveyor://chat/Claude/owner-chat", "status": "active"}));
    assert_eq!(names.files(), before);
    assert!(!names.world.home.join("queue").exists() && !names.world.home.join("window").exists());
    assert!(!names.world.root.join("called").exists(), "a program was asked: {:?}", std::fs::read_to_string(names.world.root.join("called")));
}

#[test]
fn pinned_canonical_survives_alias_rotation_but_never_retargets() {
    let mut names = Names::new("names-pinned");
    let pinned = names.ticket(json!({"chatRef": "conveyor://chat/Claude/owner-chat", "chatIds": ["current-alias"]}));
    names.native["cliSessionId"] = json!("new-alias");
    names.save_native();
    assert_eq!(names.labels(&[pinned.clone()]).1[0]["chat"], "roadmap");
    names.native["isArchived"] = json!(true);
    names.save_native();
    assert_eq!(names.labels(&[pinned]).1[0]["status"], "archived");
}

#[test]
fn submission_freezes_canonical_author_and_current_label() {
    let mut names = Names::new("names-submit");
    names.world.unset("CONVEYOR_CHAT");
    names.world.set("CLAUDECODE", "1");
    names.world.set("CLAUDE_CODE_SESSION_ID", "current-alias");
    let done = names.world.run_in(&["check", "--title", "Fixture", "--about", "Private fixture", "--check", "1. Item", "--no-wait"],
                                  &names.world.env, &names.folder);
    assert_eq!(done.code, 75, "{}", done.err);
    contains(&done.err, "принята");
    let ticket = names.world.ticket(&done.id());
    assert_eq!((ticket["chat"].clone(), ticket["chatRef"].clone()), (json!("roadmap"), json!("conveyor://chat/Claude/owner-chat")));
    assert_eq!(ticket["chatIds"], json!(["current-alias"]));
    assert_eq!(ticket["agent"], "Claude");
}
