//! `TheQueue` and `Checklists` (what reaches the app) of tests/queues-tests.py.

use super::support::*;
use crate::base::testing::set_clock;
use crate::bridge;
use crate::dispatcher::INSTALL_ATTEMPTS;
use serde_json::{Value, json};
use std::path::Path;

fn text(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or("").to_string()
}

#[test]
fn one_check_at_a_time_and_the_next_installs_itself() {
    let mut case = Case::new("the-queue-one");
    let first = case.submit_as("r/38", &["1. Первый", "2. Второй"], "Бары", "r/38", &[]);
    let second = case.submit("r/37", &["1. Лента"]);
    assert!(case.step());
    let state = case.state();
    assert_eq!(state["active"]["id"], first.as_str());
    let waiting: Vec<&str> = state["queue"].as_array().unwrap().iter().map(|ticket| ticket["id"].as_str().unwrap()).collect();
    assert_eq!(waiting, [second.as_str()]);
    assert_eq!(case.device.installs().len(), 1);
    assert!(case.notes().is_empty(), "a build on the phone is not announced on the Mac: he asked for these to go");
    assert!(Path::new(&case.device.installs()[0]).is_dir(), "the queued build is a copy of its own");

    // Marks travel while the check goes on; nothing else is installed.
    case.report(&first, &[("1", Some("ok"), None), ("2", None, None)], false, None);
    case.step();
    assert_eq!(case.state()["active"]["progress"]["marked"], 1);
    assert_eq!(case.device.installs().len(), 1);

    // «Готово» on the phone ends it; the next pass installs the next one.
    case.report(&first, &[("1", Some("ok"), None), ("2", Some("problem"), Some("шевроны уползают"))], true, Some("в целом лучше"));
    case.step();
    let state = case.state();
    assert!(state["active"].is_null());
    let done = state["recent"].as_array().unwrap().last().unwrap().clone();
    assert_eq!((text(&done, "id"), text(&done, "state"), text(&done["result"], "how")), (first.clone(), "done".to_string(), "phone".to_string()));
    assert!(!case.queue.ticket_folder(&first).join("app").exists(), "a finished check keeps no build");
    case.step();
    assert_eq!(case.state()["active"]["id"], second.as_str());
    assert_eq!(case.device.installs().len(), 2);

    let verdict = std::fs::read_to_string(case.queue.ticket_folder(&first).join("verdict.md")).unwrap();
    assert!(verdict.contains("Проверка r/38 «Бары» закончена"), "{verdict}");
    assert!(verdict.contains("отмечены все пункты"));
    assert!(verdict.contains("Работает: 1\nНе работает: 2\n"));
    assert!(verdict.contains("Комментарии:\n2. шевроны уползают\nКо всей проверке: в целом лучше"));
    assert!(verdict.contains("кнопкой «Готово» на телефоне"));
    assert!(!verdict.contains("Без отметки"));
    let history = hitl_host_core::history();
    let seen: Vec<(String, String, String)> = history.iter().map(|entry| (text(entry, "tag"), text(entry, "how"), text(entry, "summary"))).collect();
    assert_eq!(seen, [("r/38".to_string(), "phone".to_string(), "работает 1, не работает 1".to_string())]);
    assert!(history[0]["installedAt"].as_f64() <= history[0]["finishedAt"].as_f64());
}

#[test]
fn sending_a_check_back_frees_the_phone_and_returns_the_numbers() {
    let mut case = Case::new("the-queue-back");
    let ticket = case.submit_as("r/38", &["1. Первый", "2. Второй", "4a. Третий", "5. Четвёртый"], "Бары", "r/38", &[]);
    case.submit("r/37", &[]);
    case.step();
    // «Доработать»: finished with items left unmarked.
    case.report(&ticket, &[("1", Some("ok"), Some("работает, но с задержкой")), ("2", Some("problem"), None),
                           ("4a", Some("problem"), Some("уползает")), ("5", None, None)], true, None);
    case.step();
    assert!(case.state()["active"].is_null());
    let (code, out, _) = case.run_cli(&["wait", &ticket]);
    assert_eq!(code, 0);
    assert!(out.contains("Проверка r/38 «Бары» возвращена на доработку"), "{out}");
    assert!(out.contains("отмечено 3 из 4"));
    assert!(out.contains("Работает: 1\nНе работает: 2, 4a\nБез отметки: 5\n"));
    assert!(out.contains("Комментарии:\n1. работает, но с задержкой\n4a. уползает\n"));
    assert!(out.contains("кнопкой «Доработать» на телефоне"));
    assert!(out.contains("Пункты без отметки не проверены."));
    assert!(!out.contains("Первый"), "the chat wrote the items; their numbers are enough");
    case.step();
    assert_eq!(case.state()["active"]["tag"], "r/37", "the next check installs after a send-back too");
}

#[test]
fn the_report_file_the_app_writes_is_read_as_is() {
    // Byte for byte what Realize's DeviceCheckReport.fileData() produces; DeviceCheckTests in the app's Domain package
    // pins the same text.
    let mut case = Case::new("the-queue-file");
    let ticket = case.submit("r/38", &["1. Первый", "4a. Второй", "5. Третий"]);
    case.step();
    let written = format!(r#"{{"finishedAt":"2026-10-07T13:35:00Z","id":"{ticket}","results":[{{"label":"1","state":"ok"}},{{"label":"4a","note":"уползает вправо","state":"problem"}},{{"label":"5","state":"skipped"}}],"updatedAt":"2026-10-07T13:33:20Z"}}"#);
    case.device.with(|state| state.report = Some(serde_json::from_str(&written).unwrap()));
    case.step();
    let state = case.state();
    let done = state["recent"].as_array().unwrap().last().unwrap();
    let seen: Vec<(String, String, Value)> = done["result"]["items"].as_array().unwrap().iter()
        .map(|item| (text(item, "label"), text(item, "state"), item["note"].clone())).collect();
    assert_eq!(seen, [("1".to_string(), "ok".to_string(), Value::Null), ("4a".to_string(), "problem".to_string(), json!("уползает вправо")),
                      ("5".to_string(), "skipped".to_string(), Value::Null)]);
}

#[test]
fn a_report_of_another_check_ends_nothing() {
    let mut case = Case::new("the-queue-other");
    let first = case.submit("r/38", &[]);
    case.step();
    case.report("someone-else", &[("1", Some("ok"), None)], true, None);
    case.step();
    assert_eq!(case.state()["active"]["id"], first.as_str());
}

#[test]
fn a_locked_phone_holds_install_and_has_one_mac_action_instead_of_repeated_notifications() {
    let mut case = Case::new("the-queue-locked");
    let start = hitl_host_core::now();
    set_clock(Some(start));
    case.submit("r/38", &[]);
    case.device.with(|state| state.lock = "locked".into());
    case.step();
    case.step();
    assert!(case.device.installs().is_empty());
    assert!(case.notes().is_empty());
    assert_eq!(case.state()["queue"][0]["waitingFor"], "ждёт, пока телефон разблокируют");
    set_clock(Some(start + 61.0));
    case.step();
    case.step();
    let actions = |_: &Case| bridge::window_queued().into_iter().filter(|ticket| ticket.get("automaticAction").is_some()).count();
    assert_eq!(actions(&case), 1);
    set_clock(Some(start + 61.0 + 125.0));
    case.step();
    assert!(case.notes().is_empty());
    case.device.with(|state| state.lock = "unlocked".into());
    case.step();
    assert_eq!(case.device.installs().len(), 1);
    assert!(case.state()["active"].get("waitingFor").is_none());
    assert_eq!(actions(&case), 0);
}

#[test]
fn a_newer_build_of_a_waiting_chat_takes_its_place() {
    let mut case = Case::new("the-queue-newer");
    case.submit("r/38", &[]);
    case.step();
    let old = case.submit("r/37", &["1. Старая"]);
    case.submit("r/41", &[]);
    let new = case.submit("r/37", &["1. Новая"]);
    let state = case.state();
    let tags: Vec<&str> = state["queue"].as_array().unwrap().iter().map(|ticket| ticket["tag"].as_str().unwrap()).collect();
    assert_eq!(tags, ["r/37", "r/41"]);
    assert_eq!(state["queue"][0]["id"], new.as_str());
    let superseded = state["recent"].as_array().unwrap().iter().find(|ticket| ticket["id"] == old.as_str()).unwrap();
    assert_eq!(superseded["state"], "superseded");
}

#[test]
fn a_recheck_of_the_chat_on_the_phone_goes_to_the_end() {
    let mut case = Case::new("the-queue-recheck");
    let on_phone = case.submit("r/38", &[]);
    case.step();
    case.submit("r/37", &[]);
    let again = case.submit("r/38", &["1. После правок"]);
    let state = case.state();
    assert_eq!(state["active"]["id"], on_phone.as_str());
    let tags: Vec<&str> = state["queue"].as_array().unwrap().iter().map(|ticket| ticket["tag"].as_str().unwrap()).collect();
    assert_eq!(tags, ["r/37", "r/38"]);
    assert_eq!(state["queue"][1]["id"], again.as_str());
    assert_eq!(case.device.installs().len(), 1, "the build under his fingers is not replaced");
}

#[test]
fn two_chats_with_one_name_do_not_replace_each_other() {
    let mut case = Case::new("the-queue-names");
    case.submit("r/38", &[]);
    case.step();
    case.submit_as("main", &[], "", "chat one", &[]);
    case.submit_as("main", &[], "", "chat two", &[]);
    assert_eq!(case.state()["queue"].as_array().unwrap().len(), 2);
}

#[test]
fn install_failures_are_retried_and_then_reported() {
    let mut case = Case::new("the-queue-failures");
    let failing = case.submit("r/38", &[]);
    case.submit("r/37", &[]);
    case.device.with(|state| state.install_result = (false, "The device is not connected".into()));
    for _ in 0..INSTALL_ATTEMPTS {
        case.step();
    }
    let state = case.state();
    let tags: Vec<&str> = state["queue"].as_array().unwrap().iter().map(|ticket| ticket["tag"].as_str().unwrap()).collect();
    assert_eq!(tags, ["r/37"]);
    let failed = state["recent"].as_array().unwrap().iter().find(|ticket| ticket["id"] == failing.as_str()).unwrap();
    assert_eq!((text(failed, "state"), text(failed, "error")), ("failed".to_string(), "The device is not connected".to_string()));
    assert!(case.notes().contains(&("r/38: сборка не встала".to_string(), "The device is not connected".to_string())));
    case.device.with(|state| state.install_result = (true, String::new()));
    case.step();
    assert_eq!(case.state()["active"]["tag"], "r/37");
}

#[test]
fn the_dispatcher_has_nothing_to_do_on_an_empty_queue() {
    let mut case = Case::new("the-queue-empty");
    assert!(!case.step());
}

// ---- Checklists: what reaches the app

fn decode(payload: &str) -> Value {
    // base64url without padding, decoded by hand.
    let value = |c: char| match c {
        'A'..='Z' => c as u32 - 'A' as u32,
        'a'..='z' => c as u32 - 'a' as u32 + 26,
        '0'..='9' => c as u32 - '0' as u32 + 52,
        '-' => 62,
        _ => 63,
    };
    let chars: Vec<u32> = payload.chars().map(value).collect();
    let mut bytes = Vec::new();
    for chunk in chars.chunks(4) {
        let n = chunk.iter().enumerate().fold(0u32, |acc, (i, v)| acc | v << (18 - 6 * i));
        bytes.push((n >> 16) as u8);
        if chunk.len() > 2 {
            bytes.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            bytes.push(n as u8);
        }
    }
    serde_json::from_slice(&bytes).unwrap()
}

#[test]
fn an_items_actions_travel_to_the_app_inside_the_checklist_argument() {
    let mut case = Case::new("checklists-actions");
    let s = crate::items::FIELD_SEPARATOR;
    let item = format!("1. Открой настройки{s}setup=open-settings{s}link=realize://screen/settings");
    let ticket = case.submit("r/38", &[item.as_str()]);
    case.step();
    let launch = &case.device.launches()[0];
    let decoded = decode(&launch.1[3]);
    assert_eq!(decoded["items"], json!([{"label": "1", "text": "Открой настройки", "setup": ["open-settings"], "link": "realize://screen/settings"}]));
    assert_eq!(decoded["id"], ticket.as_str());
}

#[test]
fn the_app_gets_the_checklist_as_one_safe_argument() {
    let mut case = Case::new("checklists-argument");
    let ticket = case.submit_as("r/38", &["1. Первый \"пункт\" → ок", "2. Второй"], "Бары внизу", "r/38", &[]);
    case.step();
    let (bundle, arguments, _url) = case.device.launches()[0].clone();
    assert_eq!(bundle, "dev.example.demo");
    assert_eq!(arguments[..3], ["--ios-run-tag", "r/38", "--ios-run-checklist"]);
    let payload = &arguments[3];
    assert!(payload.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    assert_eq!(decode(payload), json!({"id": ticket, "tag": "r/38", "title": "Бары внизу",
                                       "items": [{"label": "1", "text": "Первый \"пункт\" → ок"}, {"label": "2", "text": "Второй"}]}));
}

#[test]
fn a_build_without_the_overlay_gets_no_checklist_argument() {
    let mut case = Case::new("checklists-no-overlay");
    let mut binary = vec![0xcf, 0xfa, 0xed, 0xfe];
    binary.extend(vec![0u8; 64]);
    binary.extend(b"--ios-run-tag\0");
    std::fs::write(case.app.join("Demo"), binary).unwrap();
    case.submit_as("r/38", &[], "", "r/38", &["--", "--app-flag"]);
    case.step();
    assert_eq!(case.device.launches()[0].1, ["--ios-run-tag", "r/38", "--app-flag"]);
    assert_eq!(case.state()["active"]["overlay"], false);
}
