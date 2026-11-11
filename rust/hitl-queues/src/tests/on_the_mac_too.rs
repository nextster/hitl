//! `ChecksAsTheGoAhead` and `OnTheMacToo` of tests/queues-tests.py: a finished check as his go-ahead, and the check on
//! the phone shown in the window on the Mac as well (the window's ticket, `mac.json` and `phone.json` in the ticket's
//! folder, marks from both screens, hurrying).

use super::support::*;
use crate::base::now;
use crate::device::{Device, Heard};
use crate::dispatcher::Dispatcher;
use crate::state::{self, is_green, last_check, window_ticket};
use crate::{bridge, verdict};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

type Marks<'a> = &'a [(&'a str, Option<&'a str>, Option<&'a str>)];

fn text(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or("").to_string()
}

fn read_json(path: &std::path::Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn verdict_of(case: &Case, ticket: &str) -> String {
    std::fs::read_to_string(case.queue.ticket_folder(ticket).join("verdict.md")).unwrap()
}

fn set_report(case: &Case, report: Value) {
    case.device.with(|state| state.report = Some(report));
}

// ---------------------------------------------------------------------------
// ChecksAsTheGoAhead

fn finished_check(case: &mut Case, marks: Marks, commit: &str) -> String {
    let extra: Vec<&str> = if commit.is_empty() { Vec::new() } else { vec!["--commit", commit] };
    let ticket = case.submit_as("r/38", &["1. Первый", "2. Второй"], "", "r/38", &extra);
    case.step();
    case.report(&ticket, marks, true, None);
    case.step();
    ticket
}

/// `last-check --branch BRANCH`: (exit code, what it printed as JSON, if anything).
fn last(case: &Case, branch: &str) -> (i32, Option<Value>) {
    let (code, out, _) = case.run_cli(&["last-check", "--branch", branch]);
    (code, if out.trim().is_empty() { None } else { Some(serde_json::from_str(&out).unwrap()) })
}

#[test]
fn the_last_check_of_a_branch_is_a_go_ahead_only_when_all_of_it_works() {
    let mut case = Case::new("go-ahead-last");
    assert_eq!(last(&case, "nextster/demo"), (1, None), "nothing has been checked yet");
    let first = finished_check(&mut case, &[("1", Some("ok"), None), ("2", Some("ok"), None)], "a1b2c3");
    let (code, check) = last(&case, "nextster/demo");
    let check = check.unwrap();
    assert_eq!((code, text(&check, "id"), text(&check, "commit"), check["green"].clone(), text(&check, "summary")),
               (0, first.clone(), "a1b2c3".to_string(), json!(true), "работает 2".to_string()));
    let history: Vec<Value> = std::fs::read_to_string(hitl_host_core::history_file()).unwrap().lines()
        .map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(history[0]["marks"], json!({"ok": 2, "problem": 0, "skipped": 0, "none": 0}));
    // What each item asked is kept beside its mark, as the window keeps it: the count alone said nothing of his comments.
    let results: Vec<(String, String, String, bool)> = history[0]["results"].as_array().unwrap().iter()
        .map(|entry| (text(entry, "label"), text(entry, "state"), text(entry, "note"), !text(entry, "text").is_empty())).collect();
    assert_eq!(results, [("1".to_string(), "ok".to_string(), String::new(), true), ("2".to_string(), "ok".to_string(), String::new(), true)]);

    // A later check with a problem takes the go-ahead back.
    finished_check(&mut case, &[("1", Some("ok"), None), ("2", Some("problem"), Some("мигает"))], "d4e5f6");
    let check = last(&case, "nextster/demo").1.unwrap();
    assert_eq!((text(&check, "commit"), check["green"].clone()), ("d4e5f6".to_string(), json!(false)));
    // So does one sent back with an item unmarked, and one with an item skipped.
    finished_check(&mut case, &[("1", Some("ok"), None), ("2", None, None)], "");
    assert_eq!(last(&case, "nextster/demo").1.unwrap()["green"], json!(false));
    finished_check(&mut case, &[("1", Some("ok"), None), ("2", Some("skipped"), None)], "");
    assert_eq!(last(&case, "nextster/demo").1.unwrap()["green"], json!(false));
    // A check taken off the phone from the Mac was not ended by the owner's button.
    case.submit_as("r/38", &["1. Первый"], "", "r/38", &["--branch", "nextster/demo"]);
    case.step();
    case.as_artem();
    assert_eq!(case.run_cli(&["release"]).0, 0);
    let check = last(&case, "nextster/demo").1.unwrap();
    assert_eq!((text(&check, "how"), check["green"].clone()), ("command".to_string(), json!(false)));
    // And a green one again gives it back; another branch has checks of its own.
    finished_check(&mut case, &[("1", Some("ok"), None), ("2", Some("ok"), None)], "0a0b0c");
    let check = last(&case, "nextster/demo").1.unwrap();
    assert_eq!((text(&check, "commit"), check["green"].clone()), ("0a0b0c".to_string(), json!(true)));
    assert_eq!(last(&case, "nextster/other"), (1, None));
}

#[test]
fn a_green_verdict_says_what_it_allows() {
    let mut case = Case::new("go-ahead-verdict");
    let git = |arguments: &[&str]| {
        let status = std::process::Command::new("git").args(arguments).status().unwrap();
        assert!(status.success(), "git {arguments:?}");
    };
    let repo = case.sandbox.folder.join("repo");
    std::fs::create_dir_all(repo.join("apps").join("ios")).unwrap();
    git(&["init", "--quiet", repo.to_str().unwrap()]);
    let project = repo.join("apps").join("ios").display().to_string();

    let verdict_for = |case: &mut Case, marks: Marks| -> String {
        let ticket = case.submit_as("r/38", &["1. Первый", "2. Второй"], "", "r/38", &["--project", &project]);
        case.step();
        case.report(&ticket, marks, true, None);
        case.step();
        verdict_of(case, &ticket)
    };

    let green: Marks = &[("1", Some("ok"), None), ("2", Some("ok"), None)];
    let text = verdict_for(&mut case, green);
    assert!(text.contains("Проверка принята в её объёме."), "{text}");
    git(&["-C", repo.to_str().unwrap(), "remote", "add", "origin", "git@github.com:example/app.git"]);
    let text = verdict_for(&mut case, green);
    assert!(text.contains("Проверка принята в её объёме."), "{text}");
    assert!(!text.contains("merge"), "{text}");
    assert_eq!(state::repository_of(&project), "example/app");
    git(&["-C", repo.to_str().unwrap(), "remote", "set-url", "origin", "https://github.com/example/app"]);
    assert_eq!(state::repository_of(&project), "example/app");
    // Anything short of green allows nothing.
    let problems: [Marks; 3] = [&[("1", Some("ok"), None), ("2", Some("problem"), Some("мигает"))],
                                &[("1", Some("ok"), None), ("2", None, None)],
                                &[("1", Some("ok"), None), ("2", Some("skipped"), None)]];
    for marks in problems {
        let text = verdict_for(&mut case, marks);
        assert!(!text.contains("Проверка принята"), "{text}");
        assert!(!text.contains("hitl merge"), "{text}");
    }
}

#[test]
fn checks_logged_before_the_marks_were_counted_are_read_by_their_summary() {
    let case = Case::new("go-ahead-summary");
    std::fs::create_dir_all(&case.queue.folder).unwrap();
    let entries = [
        json!({"id": "a", "branch": "nextster/old-green", "how": "phone", "summary": "работает 3", "finishedAt": 10}),
        json!({"id": "b", "branch": "nextster/old-partial", "how": "phone", "summary": "работает 2, без отметки 1", "finishedAt": 11}),
        json!({"id": "c", "branch": "nextster/old-problem", "how": "phone", "summary": "работает 2, не работает 1", "finishedAt": 12}),
        json!({"id": "d", "branch": "nextster/old-released", "how": "command", "summary": "работает 3", "finishedAt": 13}),
    ];
    let mut written: String = entries.iter().map(|entry| format!("{}\n", serde_json::to_string(entry).unwrap())).collect();
    written.push_str("not json\n");
    std::fs::write(hitl_host_core::history_file(), written).unwrap();
    let greens: Vec<(String, bool)> = entries.iter()
        .map(|entry| (text(entry, "branch"), is_green(&last_check(&text(entry, "branch"), None).unwrap()))).collect();
    assert_eq!(greens, [("nextster/old-green".to_string(), true), ("nextster/old-partial".to_string(), false),
                        ("nextster/old-problem".to_string(), false), ("nextster/old-released".to_string(), false)]);
    assert_eq!(text(&last_check("nextster/old-green", None).unwrap(), "id"), "a");
    assert_eq!(text(&last(&case, "nextster/old-green").1.unwrap(), "commit"), "");
}

// ---------------------------------------------------------------------------
// OnTheMacToo: «Было бы круто если бы проверка на телефоне дублировалась плашкой на маке»: the check that is on
// the phone is in the window on the Mac as well, and what he does on one screen reaches the other.

fn window_ticket_path(ticket: &str) -> PathBuf {
    window_ticket(ticket)
}

/// One item as the window writes it when he marks, comments and ends there: (label, state, note, when).
type Mark<'a> = (&'a str, Option<&'a str>, Option<&'a str>, Option<f64>);

/// What the window writes when he marks, comments and ends there.
fn on_the_mac(case: &Case, ticket: &str, marks: &[Mark], finish: Option<&str>, shots: &[(&str, &[&str])]) {
    let folder = case.queue.ticket_folder(ticket);
    std::fs::create_dir_all(&folder).unwrap();
    let results: Vec<Value> = marks.iter().map(|(label, state, note, at)| {
        let shot = shots.iter().find(|(name, _)| name == label).map(|(_, files)| json!(files)).unwrap_or(Value::Null);
        json!({"label": label, "state": state, "note": note, "at": at, "shots": shot})
    }).collect();
    std::fs::write(folder.join("mac.json"), serde_json::to_string(&json!({"id": ticket, "results": results, "finish": finish})).unwrap()).unwrap();
}

/// The app's lines on its output, as the launch hands them to the queue.
fn heard(case: &Case, report: Value) {
    let callback: Heard = case.device.with(|state| state.heard.clone()).expect("the launch is attached to the app's output");
    callback(report);
}

fn hears(case: &Case) -> bool {
    case.device.hears()
}

/// The phone, with every read of the app's file counted. The dispatcher of the case is made again on it, before any
/// pass.
struct Counting {
    inner: Arc<FakeDevice>,
    reads: Arc<AtomicUsize>,
}

impl Device for Counting {
    fn lock_state(&self) -> String {
        self.inner.lock_state()
    }
    fn install(&self, app: &str) -> (bool, String) {
        self.inner.install(app)
    }
    fn launch(&self, bundle_id: &str, arguments: &[String], url: Option<&str>, heard: Option<Heard>) -> (bool, String) {
        self.inner.launch(bundle_id, arguments, url, heard)
    }
    fn hears(&self) -> bool {
        self.inner.hears()
    }
    fn detach(&self) {
        self.inner.detach()
    }
    fn read_report(&self, bundle_id: &str) -> Option<Value> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.inner.read_report(bundle_id)
    }
    fn write_host(&self, bundle_id: &str, said: &Value) -> bool {
        self.inner.write_host(bundle_id, said)
    }
}

fn counting(case: &mut Case) -> Arc<AtomicUsize> {
    let reads = Arc::new(AtomicUsize::new(0));
    let device = Arc::new(Counting { inner: case.device.clone(), reads: reads.clone() });
    let kept = case.notes.clone();
    case.dispatcher = Dispatcher::new(case.queue.clone(), device)
        .with_notifier(move |title, body| kept.lock().unwrap().push((title.to_string(), body.to_string())))
        .with_sleep(|_| {});
    reads
}

fn phone_json(case: &Case, ticket: &str) -> Value {
    read_json(&case.queue.ticket_folder(ticket).join("phone.json"))
}

fn told(case: &Case) -> Vec<Value> {
    case.device.with(|state| state.told.clone())
}

fn states(phone: &Value) -> Vec<Value> {
    phone["results"].as_array().unwrap().iter().map(|entry| entry["state"].clone()).collect()
}

#[test]
fn the_check_on_the_phone_is_a_check_of_the_window_while_it_is_there() {
    let mut case = Case::new("mac-too-window");
    // The agent a call comes from is read from the environment: this test runs as OpenCode, whatever runs the tests.
    let others: Vec<String> = std::env::vars().map(|(name, _)| name)
        .filter(|name| name.starts_with("CODEX_") || name.starts_with("CLAUDE")).collect();
    for name in &others {
        case.sandbox.unset(name);
    }
    case.sandbox.set("OPENCODE", "1");
    let first = case.submit_as("r/38", &["1. Первый", "4a. Второй"], "Бары", "r/38 · Бары внизу", &[]);
    case.sandbox.unset("OPENCODE");
    case.submit("r/14", &["1. Другой"]);
    assert!(!window_ticket_path(&first).exists(), "a check that waits for the phone is not in the window yet");
    case.step();
    case.step();
    let shown = read_json(&window_ticket_path(&first));
    let seen: Vec<String> = ["screen", "device", "udid", "tag", "title"].iter().map(|key| text(&shown, key)).collect();
    assert_eq!(seen, ["iphone", "Test iPhone", UDID, "r/38", "Бары"]);
    // The app the chat runs in goes with it, as with a check of the window: the row of a phone's own check stood
    // without its icon on both screens, seen on the phone during his hand check of the icons.
    assert_eq!(shown["agent"], "OpenCode");
    assert_eq!(shown["items"], json!([{"label": "1", "text": "Первый"}, {"label": "4a", "text": "Второй"}]));
    assert_eq!(text(&shown, "folder"), case.queue.ticket_folder(&first).display().to_string());
    // It is the phone's check: the window's own queue does not count it, and its listing does not show it.
    assert!(bridge::window_queued().is_empty());
    // Paused for another chat, it steps back from the phone and from the window.
    case.as_artem();
    case.run_cli(&["pause", "--only", "r/14"]);
    assert!(!window_ticket_path(&first).exists());
}

#[test]
fn a_check_put_on_the_phone_again_starts_from_the_new_install() {
    // The audit of 2026-10-16, H07: after a pause the same check went on the phone again, and the window kept
    // the old install's mark, the app's empty report was dropped as older, and the Mac's marks were never told.
    let mut case = Case::new("mac-too-again");
    let first = case.submit("r/14", &["1. Первый", "2. Второй"]);
    case.step();
    set_report(&case, json!({"id": first, "results": [{"label": "1", "state": "ok", "note": null, "at": 1000.0}],
                             "currentAt": 1000.0, "finishedAt": null}));
    case.step();
    on_the_mac(&case, &first, &[("1", None, None, None), ("2", Some("ok"), None, Some(2000.0))], None, &[]);
    case.step();
    let before = told(&case).len();
    case.as_artem();
    case.run_cli(&["pause", "--only", "r/99"]);
    case.step();
    case.run_cli(&["resume"]);
    set_report(&case, json!({"id": first, "results": [], "finishedAt": null})); // the app installed again starts empty
    case.step();
    case.step();
    assert_eq!(case.state()["active"]["id"], first.as_str());
    let phone = phone_json(&case, &first);
    let marked: Vec<&Value> = phone["results"].as_array().unwrap().iter().filter(|entry| entry["state"].as_str().is_some_and(|s| !s.is_empty())).collect();
    assert!(marked.is_empty(), "the window shows the old install's mark");
    let again: Vec<Value> = told(&case)[before..].iter().filter(|said| said["id"] == first.as_str()).cloned().collect();
    let labels: Vec<Value> = again.last().unwrap()["results"].as_array().unwrap().iter().map(|entry| entry["label"].clone()).collect();
    assert_eq!(labels, [json!("2")], "the Mac's marks were not told to the new install");
    assert_eq!(case.state()["active"]["progress"]["marked"], 1);
}

#[test]
fn marks_and_comments_go_both_ways_and_the_later_one_stays() {
    let mut case = Case::new("mac-too-both-ways");
    let ticket = case.submit("r/38", &["1. Первый", "2. Второй", "3. Третий"]);
    case.step();
    set_report(&case, json!({"id": ticket, "current": "2", "updatedAt": "2026-10-07T12:00:00Z", "finishedAt": null, "results": [
        {"label": "1", "state": "ok", "note": null, "at": 100.0}, {"label": "2", "state": "problem", "note": "с телефона", "at": 300.0},
        {"label": "3", "state": null, "note": null}]}));
    on_the_mac(&case, &ticket, &[("1", Some("problem"), Some("мигает"), Some(200.0)), ("2", Some("ok"), None, Some(250.0)), ("3", None, None, None)],
               None, &[("1", &["/tmp/shot-1.png"])]);
    case.step();
    // The window reads what the phone says, with the item he is at there.
    let phone = phone_json(&case, &ticket);
    assert_eq!((phone["current"].clone(), states(&phone)), (json!("2"), vec![json!("ok"), json!("problem"), Value::Null]));
    // The app is told what was done on the Mac, once: only the items touched there.
    assert_eq!(told(&case), [json!({"id": ticket, "finished": false, "results": [
        {"label": "1", "state": "problem", "note": "мигает", "at": 200.0}, {"label": "2", "state": "ok", "note": null, "at": 250.0}]})]);
    case.step();
    assert_eq!(told(&case).len(), 1);
    assert_eq!(case.state()["active"]["progress"]["marked"], 2);
    // A phone that did not take the file is told again.
    case.device.with(|state| state.takes_files = false);
    on_the_mac(&case, &ticket, &[("1", Some("problem"), Some("мигает"), Some(200.0)), ("2", Some("ok"), None, Some(250.0)),
                                 ("3", Some("skipped"), None, Some(400.0))], None, &[]);
    case.step();
    case.device.with(|state| state.takes_files = true);
    case.step();
    let all = told(&case);
    assert_eq!((all.len(), all.last().unwrap()["results"].as_array().unwrap().last().unwrap()["label"].clone()), (3, json!("3")));
    // He ends it on the phone: of two marks of an item the later one is the verdict's.
    case.device.with(|state| state.report.as_mut().unwrap()["finishedAt"] = json!("2026-10-07T12:05:00Z"));
    case.step();
    let verdict = verdict_of(&case, &ticket);
    assert!(verdict.contains("Не работает: 1, 2\nПропущено: 3"), "{verdict}");
    assert!(verdict.contains("Комментарии:\n1. мигает\n2. с телефона"), "{verdict}");
    assert!(verdict.contains("кнопкой «Готово» на телефоне"), "{verdict}");
    assert!(!window_ticket_path(&ticket).exists(), "a check that left the phone leaves the window");
}

#[test]
fn the_item_he_is_at_is_one_for_both_screens() {
    let mut case = Case::new("mac-too-current");
    let ticket = case.submit("r/38", &["1. Первый", "2. Второй", "3. Третий"]);
    case.step();
    let blank: Vec<Value> = ["1", "2", "3"].iter().map(|label| json!({"label": label, "state": null, "note": null})).collect();
    set_report(&case, json!({"id": ticket, "current": "2", "currentAt": 300.0, "updatedAt": "2026-10-07T12:00:00Z", "finishedAt": null, "results": blank}));
    case.step();
    let phone = phone_json(&case, &ticket);
    assert_eq!((phone["current"].clone(), phone["currentAt"].clone()), (json!("2"), json!(300.0)));
    assert!(told(&case).is_empty(), "the Mac has said nothing yet");
    // He moves in the window, without marking anything: that alone is told to the phone.
    let folder = case.queue.ticket_folder(&ticket);
    let untouched: Vec<Value> = ["1", "2", "3"].iter().map(|label| json!({"label": label, "state": null, "note": "", "at": null})).collect();
    std::fs::write(folder.join("mac.json"), json!({"id": ticket, "current": "3", "currentAt": 400.0, "finish": null, "results": untouched}).to_string()).unwrap();
    case.step();
    assert_eq!(told(&case), [json!({"id": ticket, "results": [], "finished": false, "current": "3", "currentAt": 400.0})]);
}

#[test]
fn a_button_of_an_item_pressed_in_the_window_is_run_on_the_phone() {
    // «Оно должно дублироваться как экшн на маке»: the buttons the panel offers with an item are the window's too,
    // under the same names, and a press there is passed to the app.
    let mut case = Case::new("mac-too-run");
    let ticket = case.submit("r/38", &["1. Первый", "2. Второй"]);
    case.step();
    let offers = json!([{"label": "1", "actions": [{"id": ">setup", "title": "Подготовить"}, {"id": "dock-fold", "title": "dock-fold"}]}]);
    let blank: Vec<Value> = ["1", "2"].iter().map(|label| json!({"label": label, "state": null, "note": null})).collect();
    let before = json!({"id": ticket, "current": "1", "currentAt": 100.0, "finishedAt": null, "offers": offers, "results": blank});
    set_report(&case, before.clone());
    case.step();
    let folder = case.queue.ticket_folder(&ticket);
    let phone = phone_json(&case, &ticket);
    assert_eq!((phone["offers"].clone(), phone.get("ran").cloned()), (offers, None));
    // A press alone, with nothing marked and no move, is told to the phone, once.
    let untouched: Vec<Value> = ["1", "2"].iter().map(|label| json!({"label": label, "state": null, "note": "", "at": null})).collect();
    std::fs::write(folder.join("mac.json"), json!({"id": ticket, "finish": null, "run": {"label": "1", "action": ">setup", "at": 500.0},
                                                   "results": untouched}).to_string()).unwrap();
    case.step();
    case.step();
    assert_eq!(told(&case), [json!({"id": ticket, "results": [], "finished": false, "run": {"label": "1", "action": ">setup", "at": 500.0}})]);
    // What the app says of it is the window's to show; a read that was on its way before says less and is dropped.
    let mut said = before.clone();
    said["ran"] = json!({"at": 500.0, "failure": "не вышло"});
    let shared = case.dispatcher.shared().clone();
    shared.hear(said);
    let on_the_phone = shared.on_the_phone().expect("the check is on the phone");
    shared.take(&on_the_phone, Some(&before));
    let phone = phone_json(&case, &ticket);
    assert_eq!(phone["ran"], json!({"at": 500.0, "failure": "не вышло"}));
}

#[test]
fn while_he_is_at_the_check_both_screens_are_looked_at_all_the_time() {
    let mut case = Case::new("mac-too-hurry");
    let reads = counting(&mut case);
    let ticket = case.submit("r/38", &["1. Первый"]);
    case.step();
    case.report(&ticket, &[("1", Some("ok"), None)], false, None);
    // The loop's own passes: just installed and just marked, the phone is read on every one, with next to no pause between.
    for _ in 0..3 {
        case.dispatcher.step(true);
    }
    assert_eq!((reads.load(Ordering::SeqCst), case.dispatcher.pause_seconds()), (3, 0.02));
    // A minute after his last move it is read once a second, and looked at twice as often for what the Mac says.
    case.dispatcher.shared().set_stirred(now() - 120.0);
    case.queue.locked(|state| state["active"]["installedAt"] = json!(now() - 120.0));
    reads.store(0, Ordering::SeqCst);
    case.dispatcher.set_waited(0.0);
    for _ in 0..6 {
        case.dispatcher.step(true);
        let pause = case.dispatcher.pause_seconds();
        case.dispatcher.add_waited(pause);
    }
    assert_eq!((reads.load(Ordering::SeqCst), case.dispatcher.pause_seconds()), (2, 0.5));
}

#[test]
fn what_the_app_says_on_its_output_is_on_the_mac_at_once() {
    // «Хочется ещё мгновеннее если возможно»: the launch stays attached to the app's output, and a mark the app says
    // there is in the window's hands the moment it is said, not at the next read of the app's file.
    let mut case = Case::new("mac-too-output");
    let reads = counting(&mut case);
    let ticket = case.submit("r/38", &["1. Первый", "2. Второй"]);
    case.step();
    case.step();
    assert!(hears(&case), "an app with the panel is launched attached to its output");
    reads.store(0, Ordering::SeqCst);

    let said = |case: &Case| -> (Value, Vec<Value>) {
        let phone = phone_json(case, &ticket);
        (phone.get("current").cloned().unwrap_or(Value::Null), states(&phone))
    };
    let line = |current: &str, at: f64, first: Option<&str>, second: Option<&str>| -> Value {
        json!({"id": ticket, "current": current, "currentAt": at, "finishedAt": null, "results": [
            {"label": "1", "state": first, "note": null, "at": if first.is_some() { json!(at) } else { Value::Null }},
            {"label": "2", "state": second, "note": null, "at": if second.is_some() { json!(at) } else { Value::Null }}]})
    };

    // No pass of the loop between the app's line and the file the window reads.
    heard(&case, line("2", 300.0, Some("ok"), None));
    assert_eq!(said(&case), (json!("2"), vec![json!("ok"), Value::Null]));
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    // A line about another check is not this check's.
    let mut other = line("1", 900.0, Some("problem"), Some("problem"));
    other["id"] = json!("0000-other");
    heard(&case, other);
    assert_eq!(said(&case), (json!("2"), vec![json!("ok"), Value::Null]));
    // While the app speaks, its file is read only to be sure: not on every pass.
    for _ in 0..5 {
        case.dispatcher.step(true);
        let pause = case.dispatcher.pause_seconds();
        case.dispatcher.add_waited(pause);
    }
    assert!(reads.load(Ordering::SeqCst) <= 1);
    assert_eq!(case.dispatcher.pause_seconds(), 0.02, "what he does on the Mac is still looked at all the time");
    // A read of the file that was on its way while he marked again says less than the line: it does not take the mark back.
    heard(&case, line("2", 400.0, Some("ok"), Some("problem")));
    set_report(&case, line("2", 300.0, Some("ok"), None));
    case.dispatcher.set_waited(60.0);
    case.dispatcher.step(true);
    assert!(reads.load(Ordering::SeqCst) >= 1);
    assert_eq!(said(&case), (json!("2"), vec![json!("ok"), json!("problem")]));
    // The app reopened by hand says nothing more: its file is read on every pass again.
    case.device.with(|state| state.attached = false);
    set_report(&case, line("1", 500.0, Some("ok"), Some("ok")));
    reads.store(0, Ordering::SeqCst);
    for _ in 0..3 {
        case.dispatcher.step(true);
    }
    assert_eq!((reads.load(Ordering::SeqCst), said(&case)), (3, (json!("1"), vec![json!("ok"), json!("ok")])));
    // The end he says on the output ends the check, and the launch is let go of.
    case.device.with(|state| state.attached = true);
    let mut ended = line("1", 600.0, Some("ok"), Some("ok"));
    ended["finishedAt"] = json!("2026-10-10T12:00:00Z");
    heard(&case, ended);
    case.step();
    assert!(case.state()["active"].is_null());
    assert!(verdict_of(&case, &ticket).contains("Работает: 1, 2"));
    case.step();
    assert!(!hears(&case));
}

fn without_panel(case: &Case) {
    let mut binary = vec![0xcf, 0xfa, 0xed, 0xfe];
    binary.extend(vec![0u8; 64]);
    std::fs::write(case.app.join("Demo"), binary).unwrap();
}

#[test]
fn an_app_without_the_panel_is_launched_as_before() {
    let mut case = Case::new("mac-too-nopanel");
    without_panel(&case);
    case.submit("r/38", &["1. Первый"]);
    case.step();
    case.step();
    assert_eq!(case.device.launches().len(), 1);
    assert!(!hears(&case), "nothing to hear from an app that has no panel");
}

#[test]
fn done_in_the_window_on_the_mac_ends_the_check_on_the_phone_and_is_his_word() {
    let mut case = Case::new("mac-too-done");
    let ticket = case.submit("r/38", &["1. Первый", "2. Второй"]);
    case.step();
    case.report(&ticket, &[("1", Some("ok"), None), ("2", None, None)], false, None);
    on_the_mac(&case, &ticket, &[("1", None, None, None), ("2", Some("ok"), Some("с Мака"), Some(500.0))], Some("done"), &[("2", &["/tmp/a.png"])]);
    case.step();
    assert_eq!(told(&case).last().unwrap()["finished"], json!(true));
    assert!(case.state()["active"].is_null());
    let verdict = verdict_of(&case, &ticket);
    assert!(verdict.contains("закончена"), "{verdict}");
    assert!(verdict.contains("Работает: 1, 2"), "{verdict}");
    assert!(verdict.contains("Снимки экрана к комментариям, открой файлы:\n2. /tmp/a.png"), "{verdict}");
    assert!(verdict.contains("Как закончена: кнопкой «Готово» в окне на Маке."), "{verdict}");
    assert!(verdict.contains("Проверка принята в её объёме."), "{verdict}");
    let last = last_check("nextster/demo", None).unwrap();
    assert_eq!((text(&last, "how"), text(&last, "ended"), is_green(&last)), ("mac".to_string(), "done".to_string(), true));
}

#[test]
fn a_check_ended_on_the_mac_is_the_macs_though_the_app_says_the_end_first() {
    // 2026-10-10: «Готово» pressed in the window came back as «кнопкой «Готово» на телефоне». The app, told of the
    // end, ends its own and says so on its output, and that is heard before the loop looks at the Mac.
    let mut case = Case::new("mac-too-first");
    for (finish, how) in [("done", "кнопкой «Готово» в окне на Маке"), ("rework", "кнопкой «Доработать» в окне на Маке")] {
        let ticket = case.submit("r/38", &["1. Первый", "2. Второй"]);
        case.step();
        case.step();
        on_the_mac(&case, &ticket, &[("1", Some("ok"), None, Some(10.0)), ("2", Some("ok"), None, Some(11.0))], Some(finish), &[]);
        heard(&case, json!({"id": ticket, "finishedAt": "2026-10-10T12:00:00Z", "current": "2", "currentAt": 11.0, "results": [
            {"label": "1", "state": "ok", "note": null, "at": 10.0}, {"label": "2", "state": "ok", "note": null, "at": 11.0}]}));
        case.step();
        assert!(case.state()["active"].is_null());
        let verdict = verdict_of(&case, &ticket);
        assert!(verdict.contains(how), "{verdict}");
        let last = last_check("nextster/demo", None).unwrap();
        assert_eq!((text(&last, "how"), text(&last, "ended"), is_green(&last)), ("mac".to_string(), finish.to_string(), finish == "done"));
        case.step();
    }
}

#[test]
fn a_build_without_the_panel_is_marked_and_ended_in_the_window() {
    let mut case = Case::new("mac-too-nopanel-end");
    without_panel(&case);
    let ticket = case.submit("r/38", &["1. Первый"]);
    case.step();
    assert!(window_ticket_path(&ticket).exists());
    on_the_mac(&case, &ticket, &[("1", Some("problem"), Some("не открывается"), Some(10.0))], Some("done"), &[]);
    case.step();
    assert!(told(&case).is_empty(), "an app without the panel reads nothing");
    let verdict = verdict_of(&case, &ticket);
    assert!(verdict.contains("Не работает: 1"), "{verdict}");
    assert!(verdict.contains("Комментарии:\n1. не открывается"), "{verdict}");
    assert!(!verdict.contains("итог Проверяющий даёт словами в чате"), "{verdict}");
}

#[test]
fn an_app_that_does_not_say_when_a_mark_was_made_is_taken_at_the_time_of_its_report() {
    let _case = Case::new("mac-too-merge");
    let ticket = json!({"id": "t", "items": [{"label": "1", "text": "Первый"}, {"label": "2", "text": "Второй"}]});
    let report = json!({"id": "t", "updatedAt": "2026-10-07T12:00:00Z", "results": [{"label": "1", "state": "ok"}, {"label": "2", "state": null}]});
    let noon = verdict::seconds(&json!("2026-10-07T12:00:00Z"));
    let earlier = json!({"id": "t", "results": [{"label": "1", "state": "problem", "at": noon - 5.0}, {"label": "2", "state": "ok", "at": noon - 5.0}]});
    let later = json!({"id": "t", "results": [{"label": "1", "state": "problem", "at": noon + 5.0}]});
    let marks = |mine: &Value| -> Vec<Value> {
        verdict::merge_report(&ticket, Some(&report), Some(mine)).iter().map(|entry| entry["state"].clone()).collect()
    };
    assert_eq!(marks(&earlier), [json!("ok"), json!("ok")]);
    assert_eq!(marks(&later), [json!("problem"), Value::Null]);
    assert_eq!(verdict::seconds(&Value::Null), 0.0);
}
