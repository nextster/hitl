//! `BlockedPhoneActions`, `AnItemsActions` and `ACodexChatIsToldWhatHeSaidOnThePhone` of tests/queues-tests.py.
//!
//! The Codex class tested tell.py's messenger together with the queue. Here the queue's own part is tried: what
//! `hitl _queues check` and `wait` do when somebody is to be told (`bridge::standins`), what they print, their
//! exit codes and what is passed to `start_phone`. The messenger itself (the message that is handed to Codex, the
//! records in `~/.conveyor/tell`) is tell.py's, and is the `hitl` part's to test.

use super::support::*;
use crate::base::testing::set_clock;
use crate::bridge;
use crate::dispatcher::Dispatcher;
use serde_json::{Value, json};
use std::path::Path;

/// The made-up clock the Python's `self.clock` was, starting at 1000.
struct Clock(f64);

impl Clock {
    fn start() -> Clock {
        set_clock(Some(1000.0));
        Clock(1000.0)
    }

    fn advance(&mut self, seconds: f64) {
        self.0 += seconds;
        set_clock(Some(self.0));
    }
}

fn actions() -> Vec<Value> {
    bridge::window_queued().into_iter().filter(|ticket| ticket.get("automaticAction").is_some_and(|owner| !owner.is_null())).collect()
}

/// A dispatcher that has started again: nothing of the old one is remembered but what is on disk.
fn restarted(case: &Case) -> Dispatcher {
    Dispatcher::new(case.queue.clone(), case.device.clone()).with_notifier(|_, _| {}).with_sleep(|_| {})
}

fn blocked(case: &mut Case, clock: &mut Clock) -> String {
    let ticket = case.submit("blocked", &[]);
    case.device.with(|state| state.lock = "locked".into());
    case.step();
    clock.advance(60.0);
    case.step();
    ticket
}

fn done_receipt(card: &Value) -> Value {
    hitl_host_core::read_json(&bridge::window_done_folder().join(format!("{}.json", card["id"].as_str().unwrap()))).expect("a receipt")
}

#[test]
fn blocked_threshold_restart_dedup_and_actual_readiness() {
    let mut case = Case::new("blocked-threshold");
    let mut clock = Clock::start();
    let ticket = case.submit("blocked", &[]);
    case.device.with(|state| state.lock = "unreachable".into());
    case.step();
    clock.advance(59.0);
    case.step();
    assert!(actions().is_empty());
    case.dispatcher = restarted(&case);
    clock.advance(1.0);
    case.step();
    assert_eq!(actions().len(), 1);
    let action = actions()[0].clone();
    assert_eq!(action["automaticAction"]["phoneCheck"], ticket.as_str());
    assert_eq!(action["kind"], "action");
    case.dispatcher = restarted(&case);
    case.step();
    assert_eq!(actions()[0]["id"], action["id"]);
    // A successful unlock is not successful installation/launch.
    case.device.with(|state| {
        state.lock = "unlocked".into();
        state.install_result = (false, "install failed".into());
    });
    case.step();
    assert_eq!(actions().len(), 1);
    case.device.with(|state| state.install_result = (true, String::new()));
    case.step();
    assert_eq!(case.state()["active"]["id"], ticket.as_str());
    assert!(actions().is_empty());
    let receipt = done_receipt(&action);
    assert_eq!(receipt["how"], "automatic-resolved");
    assert!(!hitl::verdict::is_green(&receipt));
    assert_eq!(receipt["technicalReason"], "installed-and-launched");
}

#[test]
fn blocked_transition_and_unknown_reset_timer() {
    let mut case = Case::new("blocked-transition");
    let mut clock = Clock::start();
    case.submit("blocked", &[]);
    case.device.with(|state| state.lock = "locked".into());
    case.step();
    clock.advance(40.0);
    case.device.with(|state| state.lock = "unknown".into());
    case.step();
    clock.advance(40.0);
    case.device.with(|state| state.lock = "unreachable".into());
    case.step();
    assert!(actions().is_empty());
    clock.advance(60.0);
    case.device.with(|state| state.lock = "locked".into());
    case.step();
    assert_eq!(actions().len(), 1);
}

#[test]
fn blocked_cancellation_and_failure_remove_only_owned_action() {
    let mut case = Case::new("blocked-cancellation");
    let mut clock = Clock::start();
    // A check of the window that is none of the phone's business.
    let foreign = bridge::window_queue_folder().join("foreign-check.json");
    hitl_host_core::write_json(&foreign, &json!({"id": "foreign-check", "screen": "mac", "kind": "result", "title": "Other", "createdAt": 1.0,
                                                "items": [{"label": "1", "text": "Other"}]})).unwrap();
    let ticket = blocked(&mut case, &mut clock);
    assert_eq!(actions().len(), 1);
    case.as_artem();
    case.run_cli(&["drop", "--udid", UDID, &ticket]);
    case.step();
    assert!(actions().is_empty());
    assert!(foreign.exists());
    blocked(&mut case, &mut clock);
    let action = actions()[0].clone();
    case.device.with(|state| {
        state.lock = "unlocked".into();
        state.install_result = (false, "failure".into());
    });
    for _ in 0..3 {
        case.step();
    }
    assert!(actions().is_empty());
    assert_eq!(done_receipt(&action)["technicalReason"], "failed");
}

#[test]
fn blocked_ordinary_queue_pause_and_probe_do_not_notify() {
    let mut case = Case::new("blocked-ordinary");
    let mut clock = Clock::start();
    case.submit("first", &[]);
    case.step();
    case.submit("next", &[]);
    case.device.with(|state| state.lock = "locked".into());
    clock.advance(1000.0);
    case.step();
    assert!(actions().is_empty());
    case.queue.locked(|state| {
        state["active"] = Value::Null;
        state["paused"] = json!({"only": []});
    });
    case.step();
    clock.advance(1000.0);
    case.step();
    assert!(actions().is_empty());
}

#[test]
fn blocked_saved_answers_are_retained_without_fake_done() {
    let mut case = Case::new("blocked-saved");
    let mut clock = Clock::start();
    blocked(&mut case, &mut clock);
    let action = actions()[0].clone();
    let marks = json!([{"label": "1", "state": "no", "note": "later", "shots": ["kept.png"]}]);
    hitl_host_core::write_json(&hitl::window::window_folder().join("answers.json"), &json!({"checks": {action["id"].as_str().unwrap(): {"marks": marks}}})).unwrap();
    assert_eq!(hitl::queue::drop(action["id"].as_str().unwrap()), 1); // PR133 protection still applies to public drop
    case.device.with(|state| state.lock = "unlocked".into());
    case.step();
    assert!(actions().is_empty());
    let receipt = done_receipt(&action);
    assert_eq!(receipt["results"], marks);
    assert_eq!(receipt["how"], "automatic-resolved");
}

#[test]
fn blocked_provenance_link_survives_dispatcher_owner() {
    let mut case = Case::new("blocked-provenance");
    let mut clock = Clock::start();
    case.submit("blocked", &[]);
    case.queue.locked(|state| {
        let head = &mut state["queue"][0];
        head["agent"] = json!("Codex");
        head["chatIds"] = json!(["origin-chat-1234"]);
        head["chat"] = json!("original owner");
    });
    case.device.with(|state| state.lock = "locked".into());
    case.step();
    clock.advance(60.0);
    case.step();
    let action = actions()[0].clone();
    assert_eq!(action["chat"], "original owner");
    assert_eq!(action["items"][0]["links"], json!(["conveyor://chat/Codex/origin-chat-1234"]));
    // Answered actions must not reappear even when short-lived done files are cleaned.
    let head = case.state()["queue"][0]["id"].as_str().unwrap().to_string();
    assert!(bridge::resolve_automatic_action(&case.dispatcher.action_owner(&head), "observed"));
    std::fs::remove_file(bridge::window_done_folder().join(format!("{}.json", action["id"].as_str().unwrap()))).unwrap();
    case.dispatcher = restarted(&case);
    clock.advance(1000.0);
    case.step();
    assert!(actions().is_empty());
}

#[test]
fn blocked_probe_then_new_head_restarts_continuous_wait() {
    let mut case = Case::new("blocked-probe");
    let mut clock = Clock::start();
    case.submit("first", &[]);
    case.submit("second", &[]);
    case.device.with(|state| state.lock = "locked".into());
    case.queue.locked(|state| state["probe"] = json!({"pid": std::process::id(), "tag": "probe"}));
    case.step();
    clock.advance(60.0);
    case.step();
    assert!(actions().is_empty());
    case.queue.locked(|state| state["probe"] = Value::Null);
    case.step();
    clock.advance(40.0);
    case.queue.locked(|state| state["queue"].as_array_mut().unwrap().reverse());
    case.step();
    clock.advance(30.0);
    case.step();
    assert!(actions().is_empty());
    clock.advance(30.0);
    case.step();
    assert_eq!(actions().len(), 1);
}

#[test]
fn blocked_launch_failure_keeps_action_until_terminal_failure() {
    let mut case = Case::new("blocked-launch");
    let mut clock = Clock::start();
    blocked(&mut case, &mut clock);
    let action = actions()[0].clone();
    case.device.with(|state| {
        state.lock = "unlocked".into();
        state.launch_result = Some((false, "launch failed".into()));
    });
    case.step();
    assert_eq!(actions().len(), 1);
    assert!(case.state()["active"].is_null());
    case.step();
    case.step();
    assert!(actions().is_empty());
    assert_eq!(done_receipt(&action)["technicalReason"], "failed");
}

#[test]
fn blocked_typed_withdrawal_refuses_wrong_owner_and_unreadable_answers() {
    let mut case = Case::new("blocked-typed");
    let mut clock = Clock::start();
    let ticket = blocked(&mut case, &mut clock);
    let action = actions()[0].clone();
    let owner = case.dispatcher.action_owner(&ticket);
    let answers = hitl::window::window_folder().join("answers.json");
    hitl_host_core::write_json(&answers, &json!({"broken": true})).unwrap();
    assert!(!bridge::resolve_automatic_action(&owner, "cancelled"));
    assert_eq!(actions().len(), 1);
    std::fs::remove_file(&answers).unwrap();
    let path = bridge::window_queue_folder().join(format!("{}.json", action["id"].as_str().unwrap()));
    let mut foreign = action.clone();
    foreign["automaticAction"] = json!({"type": "iphone-blocked", "udid": "foreign", "phoneCheck": ticket});
    hitl_host_core::write_json(&path, &foreign).unwrap();
    assert!(!bridge::resolve_automatic_action(&owner, "cancelled"));
    assert!(path.exists());
}

#[test]
fn blocked_cancellation_leaves_another_phones_action() {
    let mut case = Case::new("blocked-other-phone");
    let mut clock = Clock::start();
    let ticket = blocked(&mut case, &mut clock);
    let action = actions()[0].clone();
    let other = json!({"type": "iphone-blocked", "udid": "other-phone", "phoneCheck": ticket});
    bridge::submit_automatic_action(&other, &action);
    case.as_artem();
    case.run_cli(&["drop", "--udid", UDID, &ticket]);
    case.step();
    let left: Vec<Value> = actions().iter().map(|card| card["automaticAction"].clone()).collect();
    assert_eq!(left, [other]);
}

#[test]
fn blocked_receipt_claim_recovery_and_actual_user_verdict_are_distinct() {
    let mut case = Case::new("blocked-receipt");
    let mut clock = Clock::start();
    let ticket = blocked(&mut case, &mut clock);
    let action = actions()[0].clone();
    let owner = case.dispatcher.action_owner(&ticket);
    let mut receipt = action.clone();
    receipt["how"] = json!("automatic-resolved");
    receipt["results"] = json!([]);
    receipt["technicalReason"] = json!("cancelled");
    assert!(hitl::queue::claim_done(action["id"].as_str().unwrap(), &receipt));
    assert!(bridge::resolve_automatic_action(&owner, "cancelled"));
    assert!(actions().is_empty());
    assert_eq!(hitl_host_core::history().iter().filter(|row| row["id"] == action["id"]).count(), 1);
    case.as_artem();
    case.run_cli(&["drop", "--udid", UDID, &ticket]);
    case.submit("different", &[]);
    case.step();
    clock.advance(60.0);
    case.step();
    // A genuine terminal verdict cannot be replaced by a technical receipt.
    let second = actions()[0].clone();
    let mut original = second.clone();
    original["how"] = json!("done");
    original["results"] = json!([{"state": "ok"}]);
    hitl::queue::claim_done(second["id"].as_str().unwrap(), &original);
    assert!(!bridge::resolve_automatic_action(&second["automaticAction"], "cancelled"));
    assert_eq!(done_receipt(&second), original);
}

// ---- AnItemsActions: «Где экшны подготовки экрана ты чего»

/// Queues a check with these items, as the chat «r/390 · Карточка после видео»; what the command said.
fn said_to(case: &mut Case, items: &[&str], more: &[&str]) -> String {
    case.as_chat("r/390 · Карточка после видео");
    let app = case.app.display().to_string();
    let mut arguments: Vec<String> = ["check", "--udid", UDID, "--name", "Test iPhone", "--app", app.as_str(), "--bundle-id", "dev.example.demo",
                                      "--tag", "r/390", "--branch", "nextster/demo", "--no-wait"].iter().map(|word| word.to_string()).collect();
    for item in items {
        arguments.extend(["--item".to_string(), item.to_string()]);
    }
    arguments.extend(more.iter().map(|word| word.to_string()));
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let (code, _, said) = case.run_cli(&words);
    assert_eq!(code, 0, "{said}");
    said
}

#[test]
fn a_check_with_items_alone_is_taken_and_the_chat_is_reminded() {
    let mut case = Case::new("actions-reminded");
    let said = said_to(&mut case, &["1. Прикрепи видео к задаче → карточка раскрыта", "2. Закрой карточку → в списке обычная задача"], &[]);
    assert!(said.contains("принята, номер"), "{said}");
    assert_eq!(said.matches("Ни у одного пункта нет экшна").count(), 1);
    for word in ["--check-setup", "--check-link", "--check-button", "упростить ему работу", "Не нужны — оставь как есть"] {
        assert!(said.contains(word), "{word}: {said}");
    }
}

#[test]
fn a_check_that_takes_him_somewhere_is_not_reminded() {
    let mut case = Case::new("actions-not-reminded");
    let separator = crate::items::FIELD_SEPARATOR;
    let link = format!("1. Прикрепи видео{separator}link=demo://task/1");
    let setup = format!("1. Открой настройки{separator}setup=open-settings");
    let buttons = format!("1. Сложи доку{separator}buttons=dock-fold");
    let cases: Vec<(Vec<&str>, Vec<&str>)> = vec![
        (vec![link.as_str(), "2. Закрой карточку"], vec![]),
        (vec![setup.as_str()], vec![]),
        (vec![buttons.as_str()], vec![]),
        (vec!["1. Прикрепи видео"], vec!["--url", "demo://task/1"]),
        (vec!["1. Прикрепи видео"], vec!["--", "--open-task", "1"]),
    ];
    for (items, more) in cases {
        let said = said_to(&mut case, &items, &more);
        assert!(!said.contains("нет экшна"), "{items:?} {more:?}: {said}");
    }
}

#[test]
fn an_app_without_the_panel_has_no_actions_to_be_reminded_of() {
    let mut case = Case::new("actions-no-panel");
    let mut binary = vec![0xcf, 0xfa, 0xed, 0xfe];
    binary.extend(vec![0u8; 64]);
    std::fs::write(case.app.join("Demo"), binary).unwrap();
    let said = said_to(&mut case, &["1. Прикрепи видео"], &[]);
    assert!(said.contains("нет панели чек-листа"), "{said}");
    assert!(!said.contains("нет экшна"));
}

// ---- ACodexChatIsToldWhatHeSaidOnThePhone
//
// A chat nothing wakes (a chat of Codex) is told his verdict as a message. The queue's part: it does not wait for such
// a chat (a messenger of tell.py does the telling, and that is `hitl`'s to test), asks `start_phone` for the phone's
// queue, and says what tell.py's note says. The stand-ins say who the chat is.

fn codex_thread() -> Value {
    json!({"thread": "t1", "program": "codex"})
}

/// A check the way the Codex test asked for it: the title, an item, and (not always) `--no-wait`.
fn check(case: &mut Case, items: &[&str], more: &[&str]) -> (i32, String, String) {
    let app = case.app.display().to_string();
    let mut arguments: Vec<String> = ["check", "--udid", UDID, "--name", "Test iPhone", "--app", app.as_str(), "--bundle-id", "dev.example.demo",
                                      "--tag", "r/390", "--branch", "nextster/demo", "--title", "Карточка после видео"].iter().map(|word| word.to_string()).collect();
    for item in if items.is_empty() { &["1. Прикрепи видео → карточка раскрыта"][..] } else { items } {
        arguments.extend(["--item".to_string(), item.to_string()]);
    }
    arguments.extend(more.iter().map(|word| word.to_string()));
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let (code, out, err) = case.run_cli(&words);
    (code, out.trim().to_string(), err)
}

#[test]
fn the_check_does_not_wait_and_its_verdict_comes_as_a_message() {
    let mut case = Case::new("codex-message");
    {
        let mut chat = bridge::standins();
        chat.addressee = Some(codex_thread());
        chat.starts = true;
    }
    let (code, ticket, said) = check(&mut case, &[], &[]); // without --no-wait too
    assert_eq!(code, 0, "{said}");
    assert!(said.contains(&format!("tell {} {ticket}", codex_thread())), "{said}");
    assert!(!said.contains("Не жду. Итог: hitl wait"), "{said}");
    // The messenger is asked for this phone, for this very check.
    assert_eq!(bridge::standins().started, [(ticket.clone(), codex_thread(), UDID.to_string())]);
    // Asking again does not wait either: there is nothing to poll.
    bridge::standins().by_itself = Some(codex_thread());
    let (code, out, said) = case.run_cli(&["wait", "--udid", UDID, &ticket]);
    assert_eq!((code, out.as_str()), (hitl_host_core::EXIT_BUSY, ""));
    assert_eq!(hitl_host_core::EXIT_BUSY, 75);
    assert!(said.contains(&format!("tell {} {ticket}", codex_thread())), "{said}");
    case.step();
    case.report(&ticket, &[("1", None, Some("где экшны подготовки экрана ты чего"))], true, None);
    case.step();
    // The verdict is on disk for the messenger to hand over; the state has it as the chat would read it.
    assert!(case.state()["recent"].as_array().unwrap().iter().any(|done| done["id"] == ticket.as_str() && done["state"] == "done"));
    assert!(Path::new(&case.queue.ticket_folder(&ticket).join("verdict.md")).exists());
    // Whoever reads the verdict by waiting for it is not told it again: tell.printed is asked.
    let (code, out, _) = case.run_cli(&["wait", "--udid", UDID, &ticket]);
    assert_eq!(code, 0);
    assert!(out.contains("где экшны подготовки экрана ты чего"), "{out}");
    assert!(bridge::recorded().contains(&("printed".to_string(), ticket)));
}

#[test]
fn a_chat_codex_cannot_be_asked_about_is_told_to_take_the_verdict_itself() {
    let mut case = Case::new("codex-cannot-be-asked");
    // tell.addressee found nobody (here: the chat could not be looked up); what it says of that is tell's.
    let reason = "hitl не смог узнать у Codex, что это за чат (file is not a database)";
    bridge::standins().left_waiting = reason.to_string();
    let (code, ticket, said) = check(&mut case, &[], &["--no-wait"]);
    assert_eq!(code, 0);
    assert!(said.contains(reason), "{said}");
    assert!(said.contains(&format!("hitl wait {ticket}")), "{said}");
    assert!(!said.contains("tell "), "{said}");
    assert!(bridge::standins().started.is_empty());
}

#[test]
fn a_chat_nothing_can_be_handed_to_waits_as_before() {
    let mut case = Case::new("codex-waits-as-before");
    // A run of Codex without its app: tell.addressee gives nobody.
    let (code, ticket, said) = check(&mut case, &[], &["--no-wait"]);
    assert_eq!(code, 0);
    assert!(said.contains(&format!("Не жду. Итог: hitl wait {ticket}")), "{said}");
    assert!(bridge::standins().started.is_empty());
}

#[test]
fn a_chat_whose_messenger_does_not_start_waits_as_before() {
    // Not in the Python: the queue's side of «tell.start_phone says no».
    let mut case = Case::new("codex-messenger-fails");
    {
        let mut chat = bridge::standins();
        chat.addressee = Some(codex_thread());
        chat.starts = false;
    }
    let (code, ticket, said) = check(&mut case, &[], &["--no-wait"]);
    assert_eq!(code, 0);
    assert!(said.contains(&format!("Не жду. Итог: hitl wait {ticket}")), "{said}");
    assert!(!said.contains(&format!("tell {}", codex_thread())), "{said}");
    assert_eq!(bridge::standins().started.len(), 1);
}
