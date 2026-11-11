//! `Probes`, `TheTerminal` and `TheRealDispatcher` of tests/queues-tests.py.

use super::support::*;
use crate::bridge;
use crate::device::Devicectl;
use crate::dispatcher::{Dispatcher, command_dispatch, probe_grace_seconds, use_device};
use crate::state::Queue;
use hitl_host_core::EXIT_BUSY;
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn text(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or("").to_string()
}

fn tags(state: &Value) -> Vec<String> {
    state["queue"].as_array().unwrap().iter().map(|ticket| text(ticket, "tag")).collect()
}

fn ids(state: &Value) -> Vec<String> {
    state["queue"].as_array().unwrap().iter().map(|ticket| text(ticket, "id")).collect()
}

fn pid() -> String {
    std::process::id().to_string()
}

/// A binary without the checklist argument the app declares.
fn without_overlay(case: &Case) {
    let mut binary = vec![0xcf, 0xfa, 0xed, 0xfe];
    binary.extend(vec![0u8; 64]);
    std::fs::write(case.app.join("Demo"), binary).unwrap();
}

// ---- Probes

fn probe(case: &Case, extra: &[&str]) -> (i32, String, String) {
    let pid = pid();
    let mut arguments = vec!["probe-begin", "--udid", UDID, "--name", "Test iPhone", "--tag", "r/41", "--pid", pid.as_str()];
    arguments.extend(extra);
    case.run_cli(&arguments)
}

#[test]
fn a_probe_runs_on_a_free_phone_and_a_check_waits_for_it() {
    let mut case = Case::new("probes-free");
    let (code, _, _) = probe(&case, &[]);
    assert_eq!(code, 0);
    assert_eq!(case.state()["probe"]["tag"], "r/41");
    case.submit("r/38", &[]);
    case.step();
    assert!(case.device.installs().is_empty());
    assert_eq!(case.state()["queue"][0]["waitingFor"], "ждёт конца пробы чата r/41");
    case.run_cli(&["probe-end", "--udid", UDID, "--pid", &pid()]);
    case.step();
    assert_eq!(case.device.installs().len(), 1);
}

#[test]
fn a_probe_gives_way_while_checks_are_on_the_phone_or_waiting() {
    let mut case = Case::new("probes-way");
    case.submit("r/38", &[]);
    let (code, _, err) = probe(&case, &["--no-wait"]);
    assert_eq!(code, EXIT_BUSY);
    assert!(err.contains("В очереди: r/38"), "{err}");
    case.step();
    let (code, _, err) = probe(&case, &["--no-wait"]);
    assert_eq!(code, EXIT_BUSY);
    assert!(err.contains("занят проверкой r/38"), "{err}");
    assert!(case.state()["probe"].is_null());
}

#[test]
fn a_dead_probe_holds_nothing() {
    let mut case = Case::new("probes-dead");
    case.queue.locked(|state| state["probe"] = json!({"pid": 4194304 + 12345, "tag": "r/41", "since": 0}));
    case.submit("r/38", &[]);
    case.step();
    assert_eq!(case.device.installs().len(), 1);
    assert!(case.state()["probe"].is_null());
}

#[test]
fn a_probe_that_overstays_is_replaced_and_told_so() {
    let mut case = Case::new("probes-overstay");
    probe(&case, &[]);
    case.submit("r/38", &[]);
    case.step();
    let overdue = crate::base::now() - probe_grace_seconds() - 1.0;
    case.queue.locked(|state| state["queue"][0]["probeSeenAt"] = json!(overdue));
    case.step();
    assert_eq!(case.device.installs().len(), 1);
    let (_, _, err) = case.run_cli(&["probe-end", "--udid", UDID, "--pid", &pid()]);
    assert!(err.contains("Пробу вытеснила проверка r/38"), "{err}");
}

// ---- TheTerminal

#[test]
fn status_names_what_is_on_the_phone_and_who_waits() {
    let mut case = Case::new("terminal-status");
    case.submit_as("r/38", &["1. Первый", "2. Второй"], "Бары внизу", "r/38", &[]);
    case.step();
    case.submit_as("r/37", &[], "Лента", "r/37", &[]);
    case.as_artem();
    let (code, out, _) = case.run_cli(&["status"]);
    assert_eq!(code, 0);
    assert!(out.contains("На телефоне: r/38 «Бары внизу»"), "{out}");
    assert!(out.contains("отмечено 0 из 2"), "{out}");
    assert!(out.contains("1. r/37 «Лента» · 1 пункт"), "{out}");
    assert!(out.contains("hitl iphone --queue-release"), "{out}");
}

#[test]
fn release_ends_the_check_with_the_marks_made_so_far() {
    let mut case = Case::new("terminal-release");
    let ticket = case.submit("r/38", &["1. Первый", "2. Второй"]);
    case.step();
    without_overlay(&case);
    case.submit("r/37", &[]);
    assert_eq!(case.state()["queue"][0]["overlay"], false);
    // Another chat cannot free the phone; the chat that owns the check can.
    let (code, out, _) = case.run_cli(&["release"]);
    assert_eq!(code, 1);
    assert!(out.contains("проверка другого чата"), "{out}");
    assert_eq!(case.state()["active"]["id"], ticket.as_str());
    case.report(&ticket, &[("1", Some("ok"), None), ("2", None, None)], false, None);
    case.as_chat("r/38");
    let (code, out, _) = case.run_cli(&["release", "--note", "первый ок, второй потом"]);
    assert_eq!(code, 0);
    assert!(out.contains("Проверка r/38 снята. Следующей встанет r/37."), "{out}");
    let state = case.state();
    let released = state["recent"].as_array().unwrap().iter().find(|entry| entry["id"] == ticket.as_str()).unwrap();
    assert_eq!((text(released, "state"), text(&released["result"], "how")), ("released".to_string(), "command".to_string()));
    let (code, out, _) = case.run_cli(&["wait", &ticket]);
    assert_eq!(code, 0);
    assert!(out.contains("Работает: 1\nБез отметки: 2"), "{out}");
    assert!(out.contains("командой --queue-release с Мака"), "{out}");
    assert!(out.contains("Сказано при снятии: первый ок, второй потом"), "{out}");
}

#[test]
fn release_of_a_check_without_the_overlay_says_marks_are_missing() {
    let mut case = Case::new("terminal-release-bare");
    without_overlay(&case);
    case.submit("r/38", &[]);
    case.step();
    case.as_artem();
    assert_eq!(case.run_cli(&["release"]).0, 0);
    case.as_chat("r/38");
    let (code, out, _) = case.run_cli(&["wait"]);
    assert_eq!(code, 0, "a chat finds its own check without naming it");
    assert!(out.contains("Проверка r/38 снята с Мака"), "{out}");
    assert!(out.contains("В этой сборке нет панели чек-листа"), "{out}");
    case.as_chat("r/99");
    assert_eq!(case.run_cli(&["wait"]).0, 1, "and never somebody else's");
}

#[test]
fn first_and_drop_reorder_the_queue() {
    let mut case = Case::new("terminal-reorder");
    case.submit("r/38", &[]);
    case.step();
    case.submit("r/37", &[]);
    case.submit("r/41", &[]);
    case.submit("r/40", &[]);
    // The order is the reviewer's: a chat does not move itself forward or others out.
    let (code, out, _) = case.run_cli(&["first", "r/40"]);
    assert_eq!(code, 1);
    assert!(out.contains("Порядок очереди меняет Проверяющий"), "{out}");
    assert_eq!(case.run_cli(&["drop", "r/37"]).0, 1);
    assert_eq!(tags(&case.state()), ["r/37", "r/41", "r/40"]);
    assert_eq!(case.run_cli(&["first", "--force", "r/40"]).0, 0, "unless he told it to");
    assert_eq!(tags(&case.state()), ["r/40", "r/37", "r/41"]);
    case.as_artem();
    assert_eq!(case.run_cli(&["first", "r/41"]).0, 0);
    assert_eq!(tags(&case.state()), ["r/41", "r/40", "r/37"]);
    // A chat takes its own check out without naming it.
    case.as_chat("r/37");
    let (code, _, _) = case.run_cli(&["drop"]);
    assert_eq!(code, 0);
    assert_eq!(tags(&case.state()), ["r/41", "r/40"]);
    case.as_artem();
    assert_eq!(case.run_cli(&["first", "r/99"]).0, 1);
    assert_eq!(case.run_cli(&["first", "r/40"]).0, 0);
    // Dropping the check that is on the phone frees the phone.
    assert_eq!(case.run_cli(&["drop", "r/38"]).0, 0);
    assert!(case.state()["active"].is_null());
    case.step();
    assert_eq!(case.state()["active"]["tag"], "r/40");
}

#[test]
fn a_pause_for_one_chat_sets_the_others_aside_until_resume() {
    let mut case = Case::new("terminal-pause-one");
    let on_phone = case.submit("r/14", &["1. Пуши"]);
    case.step();
    case.submit("r/37", &[]);
    let mine = case.submit("канбан", &["1. Панель видна"]);
    // A chat cannot pause the queue for itself; on the reviewer's word it can.
    let (code, out, _) = case.run_cli(&["pause", "--only", "канбан"]);
    assert_eq!(code, 1);
    assert!(out.contains("Паузу ставит и снимает Проверяющий"), "{out}");
    let (code, out, _) = case.run_cli(&["pause", "--only", "канбан", "--force"]);
    assert_eq!(code, 0);
    assert!(out.contains("на телефон идёт только канбан"), "{out}");
    assert!(out.contains("r/14 снята с телефона и стоит в очереди первой"), "{out}");
    let state = case.state();
    assert!(state["active"].is_null());
    assert_eq!(tags(&state), ["r/14", "r/37", "канбан"]);
    assert!(Path::new(state["queue"][0]["app"].as_str().unwrap()).is_dir(), "a check set aside keeps its build");

    assert!(case.step());
    assert_eq!(case.state()["active"]["id"], mine.as_str(), "only the named chat gets the phone");
    case.as_chat("r/37");
    let (code, _, err) = case.run_cli(&["probe-begin", "--udid", UDID, "--tag", "r/37", "--pid", &pid(), "--no-wait"]);
    assert_eq!(code, EXIT_BUSY);
    assert!(err.contains("Очередь на паузе"), "{err}");
    case.as_artem();
    let (_, out, _) = case.run_cli(&["status"]);
    assert!(out.contains("на телефон идёт только канбан. Снять: hitl iphone --queue-resume"), "{out}");
    assert!(out.contains("1. r/14 · 1 пункт · ждёт с"), "{out}");
    assert!(out.contains("· на паузе"), "{out}");

    // The check ends; the others still wait, and the dispatcher stays for them.
    case.report(&mine, &[("1", Some("ok"), None)], true, None);
    case.step();
    let installs = case.device.installs().len();
    assert!(case.step(), "a paused queue is not an empty one");
    assert!(case.state()["active"].is_null());
    assert_eq!(case.device.installs().len(), installs);

    // A newer build of the named chat still goes through.
    let again = case.submit("канбан", &["1. После правок"]);
    case.step();
    assert_eq!(case.state()["active"]["id"], again.as_str());
    case.report(&again, &[("1", Some("ok"), None)], true, None);
    case.step();

    case.as_artem();
    let (code, out, _) = case.run_cli(&["resume"]);
    assert_eq!(code, 0);
    assert!(out.contains("Пауза снята. Следующей встанет r/14."), "{out}");
    case.step();
    assert_eq!(case.state()["active"]["id"], on_phone.as_str(), "the check set aside comes back first");
}

#[test]
fn an_urgent_check_takes_the_phone_now_and_gives_it_back() {
    let mut case = Case::new("terminal-urgent");
    let on_phone = case.submit("r/14", &["1. Пуши"]);
    case.step();
    case.report(&on_phone, &[("1", Some("ok"), None)], false, None);
    case.step();
    case.submit("r/37", &[]);
    case.as_artem();
    case.run_cli(&["pause"]);
    let urgent = case.submit_as("r/50", &["1. Хотфикс"], "", "r/50", &["--force"]);
    let state = case.state();
    assert!(state["active"].is_null());
    assert_eq!(tags(&state), ["r/50", "r/14", "r/37"]);
    assert_eq!(state["queue"][0]["urgent"], true);
    assert_eq!(state["queue"][1]["setAsideBy"], "срочная проверка r/50");
    assert_eq!(state["queue"][1]["lostMarks"], 1);

    // Through the pause, ahead of everyone.
    case.step();
    assert_eq!(case.state()["active"]["id"], urgent.as_str());
    case.as_artem();
    let (_, out, _) = case.run_cli(&["status"]);
    assert!(out.contains("1. r/14 · 1 пункт · ждёт с"), "{out}");
    assert!(out.contains("снята с телефона: срочная проверка r/50"), "{out}");

    case.report(&urgent, &[("1", Some("ok"), None)], true, None);
    case.step();
    case.step();
    assert!(case.state()["active"].is_null(), "the pause still holds the others");
    case.run_cli(&["resume"]);
    case.step();
    let back = case.state()["active"].clone();
    assert_eq!(back["id"], on_phone.as_str(), "the displaced check comes back first");
    assert!(back.get("setAsideBy").is_none());
    let history: Vec<(String, Value)> = hitl_host_core::history().iter().map(|entry| (text(entry, "tag"), entry["urgent"].clone())).collect();
    assert_eq!(history, [("r/50".to_string(), json!(true))]);
}

#[test]
fn an_urgent_recheck_replaces_the_chats_own_build_on_the_phone() {
    let mut case = Case::new("terminal-urgent-recheck");
    let first = case.submit("r/50", &["1. Раз"]);
    case.step();
    let second = case.submit_as("r/50", &["1. Два"], "", "r/50", &["--force"]);
    let state = case.state();
    assert_eq!(ids(&state), [second.clone()]);
    let old = state["recent"].as_array().unwrap().iter().find(|ticket| ticket["id"] == first.as_str()).unwrap();
    assert_eq!(old["state"], "superseded");
    case.step();
    assert_eq!(case.state()["active"]["id"], second.as_str());
}

#[test]
fn an_urgent_probe_starts_now_and_the_check_returns_after_it() {
    let mut case = Case::new("terminal-urgent-probe");
    let on_phone = case.submit("r/14", &[]);
    case.step();
    let (code, _, err) = case.run_cli(&["probe-begin", "--udid", UDID, "--tag", "r/41", "--pid", &pid(), "--force"]);
    assert_eq!(code, 0);
    assert!(err.contains("Срочная проба"), "{err}");
    assert!(err.contains("r/14 снята с телефона"), "{err}");
    let state = case.state();
    assert!(state["active"].is_null());
    assert_eq!(state["probe"]["urgent"], true);
    case.step();
    assert_eq!(case.device.installs().len(), 1, "the phone is the probe's while it runs");
    case.run_cli(&["probe-end", "--udid", UDID, "--pid", &pid()]);
    case.step();
    assert_eq!(case.state()["active"]["id"], on_phone.as_str());
    assert_eq!(case.device.installs().len(), 2);
}

#[test]
fn a_bare_pause_holds_everything_and_keeps_what_is_on_the_phone() {
    let mut case = Case::new("terminal-bare-pause");
    let on_phone = case.submit("r/14", &[]);
    case.step();
    case.submit("r/37", &[]);
    case.as_artem();
    assert_eq!(case.run_cli(&["pause"]).0, 0);
    assert_eq!(case.state()["active"]["id"], on_phone.as_str());
    case.report(&on_phone, &[("1", Some("ok"), None)], true, None);
    case.step();
    case.step();
    assert!(case.state()["active"].is_null());
    assert_eq!(case.device.installs().len(), 1);
    case.run_cli(&["resume"]);
    case.step();
    assert_eq!(case.state()["active"]["tag"], "r/37");
}

#[test]
fn waiting_ends_with_the_verdict_on_stdout() {
    let mut case = Case::new("terminal-wait");
    let ticket = case.submit("r/38", &["1. Первый"]);
    case.step();
    case.report(&ticket, &[("1", Some("skipped"), None)], true, None);
    case.step();
    let (code, out, err) = case.run_cli(&["wait", "r/38"]);
    assert_eq!(code, 0);
    assert!(out.contains("Пропущено: 1"), "{out}");
    assert!(err.contains("Test iPhone свободен, очередь пуста."), "{err}");
}

#[test]
fn a_chat_that_lost_its_wake_is_told_of_the_phone_s_verdict_until_it_reads_it() {
    // The Python reads the chat's record in `tell` (reminders, sweep), which is the window's part and is not in this
    // crate; here the queue's side of it: the chat that queued a check is remembered at once, and the verdict is marked
    // as read by the chat only when `wait` has printed it.
    let mut case = Case::new("terminal-lost-wake");
    case.sandbox.set("CLAUDE_CODE_HOST_SESSION_ID", "local_1");
    case.sandbox.set("CLAUDE_CODE_SESSION_ID", "0a1b-session");
    // The record is shared by all the tests and ticket numbers repeat from one made-up home to the next: count from here.
    let before = bridge::recorded().len();
    let ticket = case.submit_as("r/38", &["1. Первый"], "Бары внизу", "r/38", &[]);
    let seen = |what: &str| bridge::recorded().iter().skip(before).filter(|(kind, id)| kind == what && *id == ticket).count();
    assert_eq!(seen("remember"), 1, "the chat that queued the check is kept");
    assert_eq!(seen("printed"), 0, "nothing to read while the check is on the phone");
    case.step();
    case.report(&ticket, &[("1", Some("ok"), None)], true, None);
    case.step();
    assert_eq!(seen("printed"), 0, "the verdict is not read until the chat has it");
    let (code, out, _) = case.run_cli(&["wait", "r/38"]);
    assert_eq!(code, 0);
    assert!(out.contains("Работает: 1"), "{out}");
    assert_eq!(seen("printed"), 1, "once printed, the chat has read it");
}

#[test]
fn a_superseded_wait_ends_with_its_own_code() {
    let mut case = Case::new("terminal-superseded");
    case.submit("r/38", &[]);
    case.step();
    let old = case.submit("r/37", &[]);
    case.submit("r/37", &[]);
    let (code, _, err) = case.run_cli(&["wait", &old]);
    assert_eq!(code, 3);
    assert!(err.contains("заменена более новой сборкой"), "{err}");
}

#[test]
fn a_check_needs_items() {
    let case = Case::new("terminal-needs-items");
    let app = case.app.display().to_string();
    let (code, _, err) = case.run_cli(&["check", "--udid", UDID, "--app", &app, "--bundle-id", "dev.example.demo", "--tag", "r/38"]);
    assert_ne!(code, 0);
    assert!(err.contains("needs at least one item"), "{err}");
}

#[test]
fn a_queue_is_found_by_any_name_of_its_phone() {
    let _case = Case::new("terminal-found");
    let named = Queue::named(UDID, "Test iPhone", "CORE-ID");
    named.save(&named.load());
    for key in [UDID, "CORE-ID", "test iphone"] {
        assert_eq!(crate::commands::resolve_queue(Some(key)).unwrap().udid, UDID, "{key}");
    }
}

// ---- TheRealDispatcher

/// A made-up devicectl: always unlocked, installs and launches succeed, and the marks file is whatever the test put
/// beside this script.
const FAKE_XCRUN: &str = r##"#!/bin/bash
out=""; destination=""; previous=""
for argument in "$@"; do
  [[ "$previous" == "--json-output" ]] && out="$argument"
  [[ "$previous" == "--destination" ]] && destination="$argument"
  previous="$argument"
done
printf '%s\n' "$*" >> "$FAKE_PHONE/calls.log"
case "$*" in
  *"info lockState"*) printf '%s' '{"info":{"outcome":"success"},"result":{"passcodeRequired":false}}' > "$out" ;;
  *"install app"*) printf '%s' '{"info":{"outcome":"success"}}' > "$out" ;;
  *"process launch"*"--console"*)
    # Attached to the app's output for as long as the app runs: what the test puts into `line.json` is what
    # the app says there.
    echo "Launched application with dev.example.demo bundle identifier."
    echo "Waiting for the application to terminate…"
    for _ in $(seq 1 600); do
      if [[ -f "$FAKE_PHONE/line.json" ]]; then
        printf '\x1eios-run-check %s\n' "$(cat "$FAKE_PHONE/line.json")"
        rm "$FAKE_PHONE/line.json"
      fi
      sleep 0.1
    done ;;
  *"process launch"*) printf '%s' '{"info":{"outcome":"success"}}' > "$out" ;;
  *"copy from"*)
    if [[ -f "$FAKE_PHONE/check.json" ]]; then
      cp "$FAKE_PHONE/check.json" "$destination"
      printf '%s' '{"info":{"outcome":"success"}}' > "$out"
    else
      printf '%s' '{"info":{"outcome":"failed"},"error":{"code":7000}}' > "$out"
      exit 1
    fi ;;
  *) exit 64 ;;
esac
"##;

#[test]
fn it_installs_reads_the_verdict_and_goes_away() {
    let mut case = Case::new("real-dispatcher");
    let root = case.sandbox.folder.clone();
    let phone = root.join("phone");
    let fake_bin = root.join("bin");
    std::fs::create_dir_all(&phone).unwrap();
    std::fs::create_dir_all(&fake_bin).unwrap();
    let program = fake_bin.join("xcrun");
    std::fs::write(&program, FAKE_XCRUN).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    case.sandbox.set("FAKE_PHONE", phone.to_str().unwrap());
    case.sandbox.set("CONVEYOR_POLL_SECONDS", "0.2");
    // The real dispatcher runs against the real way to the phone, whose `xcrun` is the made-up one.
    use_device(Some(Arc::new(Devicectl::with_program(UDID, program.to_str().unwrap()))));

    let ticket = case.submit_as("r/38", &["1. Первый", "2. Второй"], "", "r/38 · Бары", &[]);
    // In place of the detached process the first check starts, the dispatcher of the command in a thread.
    let dispatcher = std::thread::spawn(|| command_dispatch(UDID));

    let wait_for = |what: &str, done: &dyn Fn() -> bool| {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline && !done() {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(done(), "{what}");
    };
    wait_for("the dispatcher did not install the check", &|| case.state()["active"]["id"] == ticket.as_str());
    assert!(case.queue.dispatcher_alive());
    let calls = std::fs::read_to_string(phone.join("calls.log")).unwrap();
    assert!(calls.contains("devicectl device install app --device UDID-TEST"), "{calls}");
    // The app has the panel: the launch stays attached to its output.
    let launch = calls.lines().find(|line| line.contains("process launch")).unwrap_or_else(|| panic!("{calls}"));
    let expected = "process launch --device UDID-TEST --terminate-existing --console dev.example.demo --ios-run-tag r/38 --ios-run-checklist ";
    assert!(launch.contains(expected), "{launch}");
    assert!(launch.split("--ios-run-checklist ").nth(1).is_some_and(|rest| !rest.trim().is_empty() && !rest.contains(' ')), "{launch}");

    let on_the_mac = || -> Vec<Value> {
        let path = case.queue.ticket_folder(&ticket).join("phone.json");
        let Some(said) = std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok()) else { return Vec::new() };
        said["results"].as_array().into_iter().flatten().map(|entry| entry["state"].clone()).collect()
    };

    // A mark is read from the app's file,
    std::fs::write(phone.join("check.json"), json!({"id": ticket, "finishedAt": null, "results": [
        {"label": "1", "state": "ok", "note": null, "at": 100.0}, {"label": "2", "state": null, "note": null}]}).to_string()).unwrap();
    wait_for("the mark in the app's file did not reach the Mac", &|| on_the_mac() == [json!("ok"), Value::Null]);
    // and what the app says on its output is taken as it is said: the file knows nothing of the second mark
    // and of the end, and the verdict has both.
    std::fs::write(phone.join("line.json"), json!({
        "id": ticket, "finishedAt": "2026-10-07T12:00:00Z", "comment": null,
        "results": [{"label": "1", "state": "ok", "note": null, "at": 100.0}, {"label": "2", "state": "problem", "note": "не то", "at": 200.0}],
    }).to_string()).unwrap();
    let (code, out, err) = case.run_cli(&["wait", &ticket]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("Работает: 1\nНе работает: 2"), "{out}");
    assert!(out.contains("Комментарии:\n2. не то"), "{out}");

    wait_for("an idle dispatcher has to exit", &|| !case.queue.dispatcher_alive());
    wait_for("the dispatcher thread did not end", &|| dispatcher.is_finished());
    assert_eq!(dispatcher.join().unwrap(), 0);
}

#[test]
fn a_dispatcher_restarts_itself_when_the_helper_changes() {
    let mut case = Case::new("real-dispatcher-restart");
    let restarted = Arc::new(Mutex::new(Vec::new()));
    let kept = restarted.clone();
    let mut dispatcher = Dispatcher::new(case.queue.clone(), case.device.clone())
        .with_notifier(|_, _| {})
        .with_sleep(|_| {})
        .with_restart(move || kept.lock().unwrap().push(true));
    case.submit("r/38", &[]);
    dispatcher.mark_code_changed();
    dispatcher.run();
    assert_eq!(*restarted.lock().unwrap(), [true]);
    assert!(case.device.installs().is_empty(), "the old code does nothing more once the helper changed");
}
