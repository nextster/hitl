//! What a chat of Codex or of Claude sees of a check and of its messenger, through the commands: the port of the
//! command-level parts of `ToldTests` and of `CompactDeliveryTests` of `tests/check-tests.py`. The messenger's own
//! scenarios (steering into a turn, retries, a Claude inbox, a stopped chat woken by another) are held by the unit
//! tests of `src/tell.rs`; here `check`, `wait`, `queue`, `drop` and `_tell` are run as programs.
//!
//! Codex is made up: a program that writes down what it was handed, and its database of chats. No real chat is told.

mod common;

use common::{Answers, World, contains, git, lacks};
use serde_json::json;
use std::path::{Path, PathBuf};

/// The made-up Codex beside the world: it takes messages for chats, writes down every call into `sent`, and refuses
/// while `sent.refuse` is there; its chats: t1 of the app, its helper t3 and the helper's helper t7, a run without the
/// app (t5) and its helper (t6), a chat in a terminal (t8).
fn codex_world(world: &World) -> PathBuf {
    let sent = world.root.join("sent");
    let codex = world.program("codex", &format!(
        "#!/bin/sh\nif [ \"$1\" = queue ] && [ \"$2\" = --help ]; then exit 0; fi\nprintf '%s\\0' \"$@\" >> '{s}'\n\
         if [ -f '{s}.refuse' ]; then cat '{s}.refuse' >&2; exit 1; fi\n", s = sent.display()));
    let connection = rusqlite::Connection::open(world.root.join("state.sqlite")).unwrap();
    connection.execute("create table threads (id text, name text, title text, source text)", []).unwrap();
    let helper = |parent: &str| json!({"subagent": {"thread_spawn": {"parent_thread_id": parent}}}).to_string();
    for (id, name, title, source) in [("t1", Some("финансы"), "первое сообщение", "vscode".to_string()), ("t3", None, "", helper("t1")),
                                      ("t7", None, "", helper("t3")), ("t5", Some("без приложения"), "", "exec".into()),
                                      ("t6", None, "", helper("t5")), ("t8", Some("в терминале"), "", "cli".into())] {
        connection.execute("insert into threads values (?, ?, ?, ?)", rusqlite::params![id, name, title, source]).unwrap();
    }
    codex
}

/// The command is run by that chat of Codex from here on, not by a chat named by hand.
fn as_codex(world: &mut World, thread: &str) {
    world.env.retain(|key, _| key != "CONVEYOR_CHAT" && !key.starts_with("CLAUDE") && !key.starts_with("CODEX_"));
    world.set("CODEX_THREAD_ID", thread);
    world.set("CONVEYOR_CODEX_STATE", world.root.join("state.sqlite").display().to_string());
    world.set("CONVEYOR_CODEX", world.root.join("codex").display().to_string());
}

/// The command is run by that chat of Claude Code from here on.
fn as_claude(world: &mut World, host: &str, session: &str) {
    world.env.retain(|key, _| !key.starts_with("CLAUDE") && !key.starts_with("CODEX_") && !key.starts_with("OPENCODE"));
    world.set("CLAUDECODE", "1");
    world.set("CLAUDE_CODE_HOST_SESSION_ID", host);
    world.set("CLAUDE_CODE_SESSION_ID", session);
}

/// Every message Codex was handed: the words of each call.
fn handed(world: &World) -> Vec<Vec<String>> {
    let Ok(text) = std::fs::read_to_string(world.root.join("sent")) else { return Vec::new() };
    let words: Vec<&str> = text.split('\0').collect();
    let mut calls: Vec<Vec<String>> = Vec::new();
    for word in &words[..words.len() - 1] {
        if *word == "queue" || calls.is_empty() {
            calls.push(Vec::new());
        }
        calls.last_mut().unwrap().push(word.to_string());
    }
    calls
}

fn starts(text: &str, head: &str) {
    assert!(text.starts_with(head), "does not start with «{head}»:\n{text}");
}

const WINDOW: &str = "Окно проверок на Маке";

#[test]
fn a_chat_codex_cannot_be_asked_about_is_not_left_waiting_without_a_word() {
    let mut world = World::new("codex-broken");
    codex_world(&world);
    as_codex(&mut world, "t1");
    let broken = world.file("broken.sqlite", "this is not a database at all, whatever its name says. ".repeat(40));
    let broken = broken.display().to_string();
    let done = world.check_with(&["--title", "Вход для плагина", "--kind", "question", "--check", "1. Влить без входа с телефона?", "--no-wait"],
                                &[("CONVEYOR_CODEX_STATE", Some(&broken))]);
    let ticket = done.id();
    assert_eq!(done.code, 75, "{}", done.err);
    // No messenger will tell this chat, so the chat is told that, and how to take the verdict itself.
    contains(&done.err, "hitl не смог узнать у Codex, что это за чат (file is not a database)");
    contains(&done.err, &format!("hitl wait {ticket}"));
    lacks(&done.err, "Ждать не нужно");
    let record = world.record(&ticket);
    assert!(record.get("thread").is_none(), "no messenger has this check: {record}");
    let log = std::fs::read_to_string(world.home.join("tell/messenger.log")).unwrap();
    contains(&log, "could not ask Codex whose chat t1 is: file is not a database");
    // A chat that is no chat of Codex is asked nothing of Codex, and nothing is said of it.
    let done = world.check_with(&["--title", "Вход", "--check", "1. Пункт", "--no-wait"],
                                &[("CONVEYOR_CODEX_STATE", Some(&broken)), ("CLAUDE_CODE_HOST_SESSION_ID", Some("local_1"))]);
    assert_eq!(done.code, 75, "{}", done.err);
    lacks(&done.err, "не смог узнать у Codex");
}

#[test]
fn a_check_of_a_codex_chat_does_not_wait_and_its_verdict_comes_as_a_message() {
    let mut world = World::new("codex-told");
    let codex = codex_world(&world);
    as_codex(&mut world, "t1");
    let done = world.check(&["--title", "Вход для плагина", "--kind", "question", "--check", "1. Влить без входа с телефона?"]);
    let ticket = done.id();
    assert_eq!(done.code, 75, "{}", done.err); // without --no-wait too: «Ответил в окне, а чат Codex молчит»
    contains(&done.err, "Ждать не нужно");
    contains(&done.err, "в этот чат сообщением");
    assert_eq!(world.ticket(&ticket)["chat"], "финансы");
    let record = world.record(&ticket);
    assert_eq!((record["thread"].clone(), record["helper"].clone(), record["program"].clone()),
               (json!("t1"), json!(false), json!(codex.display().to_string())));
    // Asking again does not wait either: there is nothing to poll.
    let done = world.run(&["wait", &ticket]);
    assert_eq!((done.code, done.out.as_str()), (75, ""));
    contains(&done.err, "Проверяющий её ещё не закончил");
    contains(&done.err, "Ждать не нужно");
    assert!(handed(&world).is_empty());
    world.finish_as(&ticket, &[Some("problem")], Answers { notes: &[("1", "кстати у меня райф не подрубается поищи в чём дело")], ..Default::default() });
    assert_eq!(world.messenger(&ticket), 0);
    let calls = handed(&world);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0][..3], ["queue", "--thread", "t1"]);
    let message = &calls[0][3];
    assert!(message.starts_with("--message=Ответ из окна проверок hitl."), "{message}");
    for part in ["«Вход для плагина»", ticket.as_str(), "не новая просьба", "1 нет", "кстати у меня райф не подрубается поищи в чём дело"] {
        contains(message, part);
    }
    assert!(!world.record_file(&ticket).exists());
    // Told once: a messenger brought up again has nobody left to tell.
    assert_eq!(world.messenger(&ticket), 0);
    assert_eq!(handed(&world).len(), 1);
}

#[test]
fn a_verdict_the_chat_read_itself_is_not_told_again() {
    let mut world = World::new("codex-read");
    codex_world(&world);
    as_codex(&mut world, "t1");
    let ticket = world.check(&["--title", "Вход", "--kind", "plan", "--check", "1. Шаг", "--no-wait"]).id();
    world.finish(&ticket, &[Some("ok")]);
    // Somebody else reads it, at a terminal: the chat still has to be told.
    assert_eq!(world.run_with(&["wait", &ticket], &[("CONVEYOR_CHAT", Some("")), ("CODEX_THREAD_ID", None)]).code, 0);
    assert!(world.record_file(&ticket).exists());
    let done = world.run(&["wait", &ticket]);
    assert_eq!(done.code, 0);
    contains(&done.out, "закончена");
    assert!(!world.record_file(&ticket).exists());
    assert_eq!(world.messenger(&ticket), 0);
    assert!(handed(&world).is_empty());
}

#[test]
fn a_helper_s_check_is_told_to_the_chat_that_started_it() {
    let mut world = World::new("codex-helper");
    codex_world(&world);
    as_codex(&mut world, "t3");
    let done = world.check(&["--title", "Вход в Райф", "--kind", "action", "--check", "1. Войди в Райф → соберу операции"]);
    let ticket = done.id();
    assert_eq!(done.code, 75, "{}", done.err);
    contains(&done.err, "в чат, который тебя запустил");
    contains(&done.err, &format!("проверка {ticket} поставлена"));
    assert_eq!(world.ticket(&ticket)["chat"], "финансы · помощник");
    assert_eq!(world.run(&["wait", &ticket]).code, 75); // the helper has nothing to wait for either
    world.finish_as(&ticket, &[Some("problem")], Answers { notes: &[("1", "скипаем пока")], ..Default::default() });
    assert_eq!(world.messenger(&ticket), 0);
    let calls = handed(&world);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0][..3], ["queue", "--thread", "t1"]);
    contains(&calls[0][3], "проверка помощника этого чата");
    contains(&calls[0][3], "1 не сделаю. «скипаем пока»");
}

#[test]
fn a_chat_its_app_wakes_is_left_as_it_was() {
    let mut world = World::new("claude-in-codex");
    codex_world(&world);
    as_codex(&mut world, "t1");
    world.set("CLAUDE_CODE_HOST_SESSION_ID", "local_1"); // a chat of Claude Code, whatever else is around it
    let done = world.check(&["--title", "Вход", "--check", "1. Пункт", "--no-wait"]);
    let ticket = done.id();
    assert_eq!(done.code, 75);
    lacks(&done.err, "Ждать не нужно");
    // It has no messenger: only who asked is written down, for a wake that is lost.
    let record = world.record(&ticket);
    assert_eq!(record.get("thread"), None);
    assert!(record["asked"].as_array().unwrap().contains(&json!("local_1")), "{record}");
    world.finish(&ticket, &[Some("ok")]);
    assert_eq!(world.run(&["wait", &ticket]).code, 0);
    assert!(handed(&world).is_empty());
    assert!(!world.record_file(&ticket).exists());
}

#[test]
fn a_chat_that_lost_its_wake_finds_the_answer_first_in_the_queue_until_it_reads_it() {
    let mut world = World::new("lost-wake");
    as_claude(&mut world, "local_1", "0a1b-session");
    let ticket = world.check(&["--title", "Сайт с играми: перенос макета", "--kind", "plan", "--check", "1. Шаг", "--no-wait"]).id();
    let record = world.record(&ticket);
    assert_eq!((record["asked"].clone(), record["agent"].clone(), record.get("thread").is_some()),
               (json!(["local_1", "0a1b-session"]), json!("Claude"), false));
    // While he has not answered there is nothing to say.
    starts(&world.run(&["queue"]).out, WINDOW);
    // The command that waited was stopped an hour in; he answers after that.
    world.finish(&ticket, &[Some("ok")]);
    let queue = world.run(&["queue"]).out;
    starts(&queue, "hitl: Проверяющий ответил на проверку «Сайт с играми: перенос макета»");
    contains(&queue, "его ответа не читал");
    contains(&queue, &format!("hitl wait {ticket}\n\n{WINDOW}"));
    assert_eq!(world.run(&["queue"]).out, queue); // and says so until the chat has read it
    let done = world.run(&["wait", &ticket]);
    assert_eq!(done.code, 0);
    contains(&done.out, "закончена");
    assert!(!world.record_file(&ticket).exists());
    starts(&world.run(&["queue"]).out, WINDOW);
}

#[test]
fn explicit_wait_redirected_to_a_file_keeps_the_answer_unread() {
    // A verdict printed into a log is not one the chat has read: its messenger keeps it.
    let mut world = World::new("wait-to-file");
    as_claude(&mut world, "local_1", "0a1b-session");
    let ticket = world.check(&["--title", "Own fixture", "--check", "1. Шаг", "--no-wait"]).id();
    world.finish(&ticket, &[Some("ok")]);
    let done = world.run_into_file(&["wait", &ticket], &world.root.join("wait.log"));
    assert_eq!(done.code, 0);
    contains(&done.out, "закончена");
    assert!(world.record_file(&ticket).exists());
    assert!(world.record(&ticket).get("outputAt").is_some());
    contains(&world.run(&["queue"]).out, &format!("hitl wait {ticket}"));
}

#[test]
fn a_check_taken_back_and_a_check_of_the_codex_app_leave_nothing_to_read() {
    let mut world = World::new("nothing-to-read");
    codex_world(&world);
    as_claude(&mut world, "local_1", "0a1b-session");
    let ticket = world.check(&["--title", "Вход", "--check", "1. Пункт", "--no-wait"]).id();
    assert_eq!(world.run(&["drop", &ticket]).code, 0);
    starts(&world.run(&["queue"]).out, WINDOW);
    assert!(!world.record_file(&ticket).exists()); // any call for the window clears it away
    // A chat of the Codex app is told by its messenger, as before, and nothing is kept beside that.
    as_codex(&mut world, "t1");
    let ticket = world.check(&["--title", "Вход", "--kind", "plan", "--check", "1. Шаг"]).id();
    assert!(world.record(&ticket).get("asked").is_none());
    world.finish(&ticket, &[Some("ok")]);
    starts(&world.run(&["queue"]).out, WINDOW);
}

// ---- What the message says: a plan, a result and an explanation do not gain each other's permissions
// (CompactDeliveryTests.test_plan_result_and_explanation_do_not_gain_each_others_permissions)

/// The message the chat of Codex is handed for a check of this kind, asked from a branch of a repository on GitHub
/// and answered so.
fn told(world: &World, repository: &Path, kind: &str, items: &[&str], states: &[Option<&str>], answers: Answers) -> String {
    let mut arguments = vec!["check", "--about", common::ABOUT, "--title", "Вход", "--kind", kind];
    for item in items {
        arguments.extend(["--check", item]);
    }
    if items.len() == 1 && answers.choices.iter().any(|(label, _)| *label == "1") {
        arguments.extend(["--option", "Влить", "--option", "Не вливать"]);
    }
    let done = world.run_in(&arguments, &world.env, repository);
    assert_eq!(done.code, 75, "{}", done.err);
    let ticket = done.id();
    assert_eq!(world.ticket(&ticket)["branch"], "feature");
    world.finish_as(&ticket, states, answers);
    let before = handed(world).len();
    assert_eq!(world.messenger(&ticket), 0);
    let calls = handed(world);
    assert_eq!(calls.len(), before + 1);
    calls[before][3].clone()
}

#[test]
fn plan_result_and_explanation_do_not_gain_each_others_permissions() {
    let mut world = World::new("permissions");
    codex_world(&world);
    as_codex(&mut world, "t1");
    let repository = world.root.join("app");
    std::fs::create_dir_all(&repository).unwrap();
    let place = repository.display().to_string();
    git(&world, &["-C", &place, "init", "-q"]);
    git(&world, &["-C", &place, "remote", "add", "origin", "https://github.com/example/app"]);
    git(&world, &["-C", &place, "commit", "-q", "--allow-empty", "-m", "start"]);
    git(&world, &["-C", &place, "checkout", "-q", "-b", "feature"]);
    let two = ["1. Раз", "2. Два"];
    let plan = told(&world, &repository, "plan", &two, &[Some("ok"), Some("ok")], Answers::default());
    contains(&plan, "1 да\n2 да");
    contains(&plan, "План принят");
    lacks(&plan, "Разрешён");
    let result = told(&world, &repository, "result", &two, &[Some("ok"), Some("ok")], Answers::default());
    contains(&result, "1 ок\n2 ок");
    lacks(&result, "hitl merge");
    contains(&result, "Результат принят.");
    for (states, answers) in [(vec![Some("ok"), Some("ok")], Answers { how: "rework", ..Default::default() }),
                              (vec![Some("problem")], Answers::default()), (vec![Some("skipped")], Answers::default()),
                              (vec![Some("ok")], Answers { choices: &[("1", "Не вливать")], ..Default::default() })] {
        let items: &[&str] = if states.len() == 2 { &two } else { &["1. Раз"] };
        let denied = told(&world, &repository, "result", items, &states, answers);
        lacks(&denied, "Разрешён");
        lacks(&denied, "Результат принят");
    }
    let explanation = told(&world, &repository, "explain", &two, &[Some("ok"), Some("ok")], Answers::default());
    lacks(&explanation, "Разрешён");
    lacks(&explanation, "План принят");
}
