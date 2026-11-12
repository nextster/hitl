//! The window itself, started with CONVEYOR_WINDOW_SCRIPT: it reads a made-up queue, takes non-keyboard actions from
//! a script and writes the verdicts it would write for the person. It is never put on the screen, never takes the
//! keyboard and opens nothing: a shown window, started from the app in front, took his typing. Every dump a test
//! reads says so (`Case::seen`).
//!
//! The port of `WindowTests` of tests/check-tests.py: the checks are queued by this crate's `hitl check`, as
//! hitl runs it (`HITL_TOOL=hitl-host`), and the window is the one `hitl::window::built_panel` builds from
//! window/*.swift of this tree, once per version, kept in the temporary folder.

mod window_support;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use window_support::{Case, Listener, one_colour, read, written};

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn strings(value: &Value) -> Vec<String> {
    value.as_array().unwrap_or_else(|| panic!("not a list: {value}")).iter()
        .map(|item| item.as_str().unwrap_or_else(|| panic!("not a text: {item}")).to_string()).collect()
}

fn dump(case: &Case, name: &str) -> String {
    format!("dump {}", case.at(name).display())
}

fn snap(case: &Case, name: &str) -> String {
    format!("snap {}", case.at(name).display())
}

// --------------------------------------------------------------------------
// The chat's name in the head

#[test]
fn a_direct_window_uses_the_same_default_lock_as_the_cli() {
    use std::os::fd::AsRawFd;
    for legacy in [false, true] {
        let case = Case::new(if legacy { "legacy-lock" } else { "modern-lock" });
        std::fs::create_dir_all(case.user.join(".hitl")).unwrap();
        let chosen = case.user.join(if legacy { ".conveyor" } else { ".hitl" });
        std::fs::create_dir_all(chosen.join("window")).unwrap();
        let lock = std::fs::File::create(chosen.join("window/panel.lock")).unwrap();
        assert_eq!(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) }, 0);
        let script = case.at("steps");
        std::fs::write(&script, dump(&case, "duplicate.json")).unwrap();
        let done = std::process::Command::new(window_support::panel()).env_clear()
            .env("HOME", &case.user).env("PATH", "/usr/bin:/bin").env("CONVEYOR_JOURNAL", "0")
            .env("CONVEYOR_WINDOW_SCRIPT", script).env("CONVEYOR_COMMAND", window_support::HITL)
            .output().unwrap();
        assert!(done.status.success(), "{}", String::from_utf8_lossy(&done.stderr));
        assert!(!case.at("duplicate.json").exists(), "a second window ran despite the canonical lock");
        for name in [".hitl", ".conveyor"] {
            assert!(!case.user.join(name).join("window/panel.pid").exists(), "a second window acquired {name}");
        }
    }
}

/// The port of `test_shared_chat_name_on_signed_private_window_without_input_events`, without its signed private
/// bundle: the name hitl keeps for a chat of Claude stands in the head (`check --presentation`, called back by
/// the window), the chat's exact link is offered, the presses of the phone end the check, and the verdict keeps
/// what the check was asked with.
#[test]
fn a_shared_chat_name_stands_in_the_head_without_input_events() {
    let mut case = Case::new("names");
    let folder = case.root.join("named");
    std::fs::create_dir_all(&folder).unwrap();
    let place = |name: &str| case.root.join(name).display().to_string();
    case.more = vec![
        ("CONVEYOR_CLAUDE_SESSIONS_DIR".into(), place("desktop")),
        ("CONVEYOR_CLAUDE_PROJECTS_DIR".into(), place("projects")),
        ("CONVEYOR_CODEX_STATE".into(), place("absent.sqlite")),
        ("CONVEYOR_OPENCODE_STATE".into(), place("absent-oc.sqlite")),
    ];
    // The chat as the Claude app keeps it, and the name hitl keeps for it.
    let record = case.root.join("desktop/a/b/owner.json");
    std::fs::create_dir_all(record.parent().unwrap()).unwrap();
    std::fs::write(&record, json!({"sessionId": "owner-chat", "cliSessionId": "current-alias", "title": "native",
                                   "cwd": folder.display().to_string()}).to_string()).unwrap();
    let key = format!("[{}, {}]", json!("Claude"), json!("owner-chat"));
    let names = case.home.join("chats/names").join(format!("{}.json", hex(&Sha256::digest(key.as_bytes()))));
    std::fs::create_dir_all(names.parent().unwrap()).unwrap();
    std::fs::write(&names, json!({"version": 1, "app": "Claude", "id": "owner-chat", "mode": "manual", "revision": 1,
                                  "generation": 0, "title": "roadmap", "decisions": {}}).to_string()).unwrap();
    let ticket = json!({"id": "fixture-check", "agent": "Claude", "chat": "", "chatIds": ["owner-chat"],
                        "cwd": folder.display().to_string(), "project": folder.display().to_string(), "createdAt": 1,
                        "tag": "", "title": "Fixture", "screen": "mac", "kind": "result", "answers": ["ок", "не ок", "пропустить"],
                        "projectName": "fixture", "commit": "frozen-commit", "about": "What the task is",
                        "chatLink": "claude://code/session_owner",
                        "items": [{"label": "1", "text": "Private naming fixture", "details": "What an answer leads to",
                                   "options": ["One", "Two"], "facts": ["A fact"], "links": ["/tmp/a", "/tmp/b"]}]});
    std::fs::create_dir_all(case.home.join("queue")).unwrap();
    std::fs::write(case.queued("fixture-check"), ticket.to_string()).unwrap();
    for (number, word) in [json!({"do": "note", "ticket": "fixture-check", "label": "1", "text": "preserved"}),
                           json!({"do": "answer", "ticket": "fixture-check", "label": "1", "number": 0}),
                           json!({"do": "finish", "ticket": "fixture-check", "how": "done"})].iter().enumerate() {
        case.press(&number.to_string(), word);
    }
    case.play(&[
        "names".into(), "remote 1".into(), dump(&case, "shown.json"), snap(&case, "header.png"),
        dump(&case, "after-snapshot.json"), "remote 1".into(), "remote 1".into(),
    ]);
    let shown = case.seen("shown.json");
    assert_eq!((shown["origin"].as_str(), shown["note"].as_str()), (Some("roadmap"), Some("preserved")));
    let after = case.seen("after-snapshot.json");
    let (before, then) = (shown["height"].as_f64().unwrap(), after["height"].as_f64().unwrap());
    assert!((before - then).abs() <= 1.0, "the head changed the window's height: {before} then {then}");
    // The "Чат" button opens the chat in the Claude app by the address the check carries, never `conveyor://…`: the
    // app that took that scheme is gone.
    let opens = strings(&shown["openAddresses"]);
    assert!(opens.contains(&"claude://code/session_owner".to_string()), "{opens:?}");
    assert!(opens.iter().all(|address| !address.starts_with("hitl:")), "{opens:?}");
    let verdict = case.verdict("fixture-check");
    assert_eq!((verdict["chat"].as_str(), verdict["commit"].as_str(), verdict["results"][0]["note"].as_str()),
               (Some(""), Some("frozen-commit"), Some("preserved")));
    // What the item offered goes into the verdict and so into the history, to see later what made an item good or bad.
    let result = &verdict["results"][0];
    assert_eq!((verdict["about"].as_str(), result["details"].as_str(), result["options"].clone(), result["facts"].clone(), result["links"].as_i64()),
               (Some("What the task is"), Some("What an answer leads to"), json!(["One", "Two"]), json!(["A fact"]), Some(2)));
    assert!(result.get("copies").is_none() && result.get("page").is_none(), "{result}");
    assert!(std::fs::metadata(case.at("header.png")).unwrap().len() > 1000);
}

// --------------------------------------------------------------------------
// A check taken back while the window is up

/// The id of the window's card that stands for one blocked phone check (check.automatic_action_id).
fn automatic_action_id(owner: &Value) -> String {
    let mut sorted = serde_json::Map::new();
    let map = owner.as_object().unwrap();
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    for key in keys {
        sorted.insert(key.clone(), map[key].clone());
    }
    let line = hitl::common::dumps_line(&Value::Object(sorted));
    format!("iphone-action-{}", &hex(&Sha256::digest(line.as_bytes()))[..24])
}

/// The card of a blocked phone check, as the phone's queue puts it into the window (check.submit_automatic_action):
/// an action queued by `check`, under the id of its owner and with the owner beside it; its id.
fn automatic_action(case: &Case, owner: &Value) -> String {
    let done = case.hitl(&["check", "--title", "Phone", "--about", "Phone waits", "--kind", "action", "--check", "1. Unlock", "--no-wait"]);
    assert_eq!(done.status.code(), Some(75), "{}", String::from_utf8_lossy(&done.stderr));
    let source = String::from_utf8_lossy(&done.stdout).trim().to_string();
    let mut card = read(&case.queued(&source));
    std::fs::remove_file(case.queued(&source)).unwrap();
    let ticket = automatic_action_id(owner);
    card["id"] = json!(ticket);
    card["automaticAction"] = owner.clone();
    std::fs::write(case.queued(&ticket), card.to_string()).unwrap();
    ticket
}

#[test]
fn automatic_action_withdrawal_keeps_delivered_edits_and_foreign_card() {
    let case = Case::new("withdrawal");
    let owner = json!({"type": "iphone-blocked", "udid": "fake-phone", "phoneCheck": "fake-check"});
    let ticket = automatic_action(&case, &owner);
    let foreign = case.queue(&["--title", "Other", "--check", "1. Other"]);
    case.press("001", &json!({"do": "answer", "ticket": ticket, "label": "1", "number": 0}));
    let token = "c".repeat(32);
    let verdict = json!({"id": ticket, "how": "automatic-resolved", "automaticAction": owner, "kind": "action", "screen": "mac",
                         "technicalReason": "installed-and-launched", "results": []});
    let reply = case.drop_request(&token, &json!({"id": ticket, "token": token, "deadline": now() + 30.0, "verdict": verdict}));
    case.play(&["reload".into(), dump(&case, "after.json")]);
    assert!(!case.queued(&ticket).exists());
    assert!(case.queued(&foreign).exists());
    let receipt = case.verdict(&ticket);
    assert_eq!(receipt["how"], "automatic-resolved");
    assert_eq!(receipt["results"][0]["state"], "ok");
    assert!(!hitl::verdict::is_green(&receipt));
    assert_eq!(read(&reply)["outcome"], "dropped");
    case.seen("after.json");
}

#[test]
fn automatic_action_behind_an_active_foreign_card_does_not_interrupt_it() {
    let case = Case::new("behind");
    let foreign = case.queue(&["--title", "Other active check", "--check", "1. Other"]);
    let owner = json!({"type": "iphone-blocked", "udid": "fake-phone", "phoneCheck": "fake-check"});
    let ticket = automatic_action(&case, &owner);
    case.press("001", &json!({"do": "answer", "ticket": foreign, "label": "1", "number": 0}));
    let token = "d".repeat(32);
    case.drop_request(&token, &json!({"id": ticket, "token": token, "deadline": now() + 30.0, "verdict": {
        "id": ticket, "how": "automatic-resolved", "automaticAction": owner, "kind": "action", "results": [],
        "technicalReason": "cancelled"}}));
    case.play(&["reload".into(), dump(&case, "after.json")]);
    let state = case.seen("after.json");
    assert_eq!((state["id"].as_str(), state["marks"][0]["state"].as_str()), (Some(foreign.as_str()), Some("ok")));
    assert!(!case.queued(&ticket).exists());
    assert!(case.queued(&foreign).exists());
}

#[test]
fn live_drop_is_serialized_after_an_answer_without_keyboard_events() {
    let case = Case::new("serialized");
    let ticket = case.queue(&["--title", "Protected", "--check", "1. Item"]);
    case.press("001", &json!({"do": "answer", "ticket": ticket, "label": "1", "number": 0}));
    let mut script = vec!["remote".to_string(), dump(&case, "marked.json")];
    script.extend(std::iter::repeat_n("reload".to_string(), 60));
    let mut window = case.start(&script, "");
    written(&case.at("marked.json"), Duration::from_secs(8));
    let state = case.seen("marked.json");
    assert_eq!(state["marks"][0]["state"], "ok");
    // The chat takes its check back while the window holds his answer: the window says no.
    let dropped = case.hitl(&["drop", &ticket]);
    let said = String::from_utf8_lossy(&dropped.stderr).to_string();
    assert_eq!(dropped.status.code(), Some(1), "{said}");
    assert!(said.contains("уже содержит ответы"), "refused by the window, for his answer: {said}");
    assert!(case.queued(&ticket).exists());
    assert!(!case.home.join("done").join(format!("{ticket}.json")).exists());
    let status = window.wait(Duration::from_secs(12));
    assert!(status.success(), "the window ended with {status}: {}", window.said());
    let kept = read(&case.home.join("window/answers.json"));
    assert_eq!(kept["checks"][&ticket]["marks"][0]["state"], "ok");
}

#[test]
fn delivered_phone_answer_precedes_a_pending_drop_on_first_reload() {
    let case = Case::new("precedes");
    let ticket = case.queue(&["--title", "Protected", "--check", "1. Item"]);
    case.press("001", &json!({"do": "answer", "ticket": ticket, "label": "1", "number": 0}));
    let token = "b".repeat(32);
    let reply = case.drop_request(&token, &json!({"id": ticket, "token": token, "deadline": now() + 8.0,
                                                  "verdict": {"id": ticket, "how": "dropped", "results": []}}));
    case.play(&["reload".into(), dump(&case, "after.json")]);
    assert_eq!(case.seen("after.json")["marks"][0]["state"], "ok");
    assert_eq!(read(&reply)["outcome"], "protected");
    assert!(case.queued(&ticket).exists());
    assert!(!case.home.join("done").join(format!("{ticket}.json")).exists());
}

#[test]
fn live_drop_of_untouched_check_keeps_one_terminal_receipt() {
    let case = Case::new("untouched");
    let ticket = case.queue(&["--title", "Untouched", "--check", "1. Item"]);
    let token = "a".repeat(32);
    let reply = case.drop_request(&token, &json!({"id": ticket, "token": token, "deadline": now() + 8.0,
                                                  "verdict": {"id": ticket, "how": "dropped", "screen": "mac", "results": []}}));
    case.play(&["reload".into(), dump(&case, "after.json")]);
    assert!(!case.queued(&ticket).exists());
    assert_eq!(case.verdict(&ticket)["how"], "dropped");
    assert_eq!(read(&reply)["outcome"], "dropped");
    let history = std::fs::read_to_string(case.home.join("history.jsonl")).unwrap();
    let lines = history.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|entry| entry["id"].as_str() == Some(ticket.as_str())).count();
    assert_eq!(lines, 1, "{history}");
    case.seen("after.json");
}

// --------------------------------------------------------------------------
// A live page

const PAGE: &str = r#"<!doctype html><meta charset="utf-8"><body style="margin:0">
<label><input type="checkbox" id="on" checked> включено</label>
<input type="range" id="many" min="1" max="10" value="3"><div id="tall" style="height:80px"></div>__MORE__
<script>
  var hitl = window.conveyor || {set: function () {}};
  function draw() { hitl.set({'включено': document.getElementById('on').checked, 'сколько': +document.getElementById('many').value,
                                  'подпись': '  два   слова  '}); }
  document.addEventListener('input', draw);
  draw();
</script>"#;

#[test]
fn a_live_page_keeps_utf8_with_a_late_or_missing_charset() {
    for charset in ["", "<meta charset=\"utf-8\">"] {
        let case = Case::new(if charset.is_empty() { "no-charset" } else { "late-charset" });
        let page = case.root.join("страница.html");
        let html = format!("<!doctype html><head><style>/*{}*/</style></head>{charset}<p>Все темы</p>\n\
                            <script>hitl.set({{'подпись': document.querySelector('p').textContent}});</script>", "x".repeat(8000));
        std::fs::write(&page, html).unwrap();
        case.queue(&["--title", "Кодировка", "--check", "1. Русский текст", "--page", page.to_str().unwrap()]);
        case.play(&[dump(&case, "page.json")]);
        let shown = case.seen("page.json");
        assert_eq!(shown["livePageReady"], true);
        assert_eq!(shown["sent"], "Уйдёт чату с ответом: подпись Все темы");
    }
}

#[test]
fn a_live_page_reaches_no_network_and_goes_nowhere() {
    let case = Case::new("page");
    let listener = Listener::start();
    let there = listener.address();
    // The server hears whoever asks it: a page that reached the network would be heard.
    listener.probe();
    let page = case.root.join("страница.html");
    let more = format!(r#"<img src="{there}/picture.png"><script>fetch("{there}/fetch").catch(function () {{}})</script>"#);
    std::fs::write(&page, PAGE.replace("__MORE__", &more)).unwrap();
    case.queue(&["--title", "Страница", "--kind", "question", "--check", "1. Взять эти?", "--page", &page.display().to_string()]);
    case.play(&[
        dump(&case, "first.json"), format!("page location.href = '{there}/leave'"), dump(&case, "stayed.json"),
        "page document.getElementById('on').click()".into(), dump(&case, "alive.json"),
    ]);
    assert_eq!(case.seen("first.json")["livePageReady"], true);
    case.seen("stayed.json");
    let sent = case.seen("alive.json")["sent"].as_str().unwrap_or("").to_string();
    assert!(sent.contains("включено нет"), "the page is still the page: {sent:?}");
    assert_eq!(listener.heard(), Vec::<String>::new());
}

/// An item too tall for the page is drawn at the largest scale at which it fits, and the search costs few
/// layouts: from the scale the item had a letter before it is two tries and the layout that stays, where the
/// bisection of the whole range took ten and more, at every letter of a comment.
#[test]
fn an_item_too_tall_for_the_page_is_laid_out_few_times_per_letter() {
    let case = Case::new("scale");
    let mut arguments = vec!["--title", "Длинный пункт", "--kind", "question", "--check", "1. Какой из длинных вариантов взять?"];
    let facts: Vec<String> = (1..=5).map(|n| format!("Строка {n}: {}", "очень длинный текст о том, что тут. ".repeat(2))).collect();
    for fact in &facts {
        arguments.extend(["--fact", fact]);
    }
    arguments.extend(["--details", "Подробности, которые тоже занимают место на странице и не помещаются на низкий экран целиком."]);
    case.queue(&arguments);
    let mut script = vec!["click На весь экран".to_string(), "focus".into(), dump(&case, "d0.json")];
    for n in 1..=6 {
        script.push("text ы".to_string());
        script.push(dump(&case, &format!("d{n}.json")));
    }
    case.play_on(&script, "1000x600");
    let first = case.seen("d0.json");
    assert_eq!(first["full"], true);
    let drawn = first["scale"].as_f64().unwrap();
    assert!(drawn > 0.85 && drawn < 1.0, "the item is to be drawn smaller, between the least scale and its own: {first}");
    let mut before = first["layouts"].as_i64().unwrap();
    for n in 1..=6 {
        let now = case.seen(&format!("d{n}.json"));
        let layouts = now["layouts"].as_i64().unwrap();
        // A letter costs the step it had, the next one, and the layout that stays: not a search of the whole range
        // (eleven layouts a letter before).
        assert!(layouts - before <= 5, "letter {n}: {} layouts for one typed letter", layouts - before);
        before = layouts;
        assert!((now["scale"].as_f64().unwrap() - drawn).abs() < 0.02, "a letter moves the scale by a hair: {now}");
    }
}

/// The wheel over a live page taller than it is let to stand scrolls the page while it has room that way, and the
/// item when it has none, so that the item is never out of the wheel's reach under a page.
#[test]
fn the_wheel_over_a_tall_page_scrolls_the_page_and_then_the_item() {
    let case = Case::new("wheel");
    let page = case.root.join("высокая.html");
    std::fs::write(&page, r#"<!doctype html><meta charset="utf-8"><body style="margin:0"><div style="height:900px">длинная страница</div>
<script>var hitl = window.conveyor || {set: function () {}}; hitl.set({'a': 1});</script>"#).unwrap();
    let facts: Vec<String> = (1..=5).map(|n| format!("Строка {n}: {}", "очень длинный текст о том, что тут. ".repeat(2))).collect();
    let mut arguments = vec!["--title", "Высокая страница", "--kind", "question", "--check", "1. Какой из длинных вариантов взять?"];
    for fact in &facts {
        arguments.extend(["--fact", fact]);
    }
    arguments.extend(["--page", page.to_str().unwrap()]);
    case.queue(&arguments);
    let script: Vec<String> = ["dump d0", "wheel 100", "dump d1", "wheel 5000", "dump d2", "wheel 40", "dump d3", "wheel -3000", "dump d4", "wheel -40", "dump d5"]
        .iter().map(|step| match step.strip_prefix("dump ") {
            Some(name) => dump(&case, &format!("{name}.json")),
            None => step.to_string(),
        }).collect();
    case.play_on(&script, "1000x420");
    let page_at = |name: &str| {
        let seen = case.seen(&format!("{name}.json"));
        (seen["pageScroll"].clone(), seen["scrolled"].as_f64().unwrap())
    };
    let d0 = page_at("d0");
    assert!(d0.0[1].as_f64().unwrap() > 300.0, "the page is taller than it stands: {d0:?}");
    let d1 = page_at("d1");
    assert!(d1.0[0].as_f64().unwrap() > 50.0 && d1.1 == 0.0, "the page takes the first turn: {d1:?}");
    let d2 = page_at("d2");
    assert!(d2.0[0].as_f64().unwrap() >= d2.0[1].as_f64().unwrap() - 1.0 && d2.1 == 0.0, "the page goes to its end: {d2:?}");
    let d3 = page_at("d3");
    assert!(d3.1 > 0.0, "at the page's end the turn is the item's: {d3:?}");
    let d4 = page_at("d4");
    assert!(d4.0[0].as_f64().unwrap() < 1.0, "upwards the page goes first again: {d4:?}");
    let d5 = page_at("d5");
    assert!(d5.1 < d4.1.max(d3.1), "at the page's top the turn up is the item's: {d5:?}");
}

// --------------------------------------------------------------------------
// What he does in the window

#[test]
fn a_window_started_again_keeps_every_screenshot_of_an_item() {
    // The audit of 2026-10-16, A08: the next window numbered its shots from one again and wrote over the last one's.
    let case = Case::new("shots");
    let ticket = case.queue(&["--title", "Снимки", "--kind", "plan", "--check", "1. Сначала сервер"]);
    let (red_file, blue_file) = (case.at("red.png"), case.at("blue.png"));
    let (red, blue) = (one_colour(&red_file, [255, 0, 0]), one_colour(&blue_file, [0, 0, 255]));
    case.play(&[format!("paste {}", red_file.display()), dump(&case, "first.json")]);
    case.play(&[format!("paste {}", blue_file.display()), dump(&case, "second.json")]);
    let first = strings(&case.seen("first.json")["marks"][0]["shots"]);
    let second = strings(&case.seen("second.json")["marks"][0]["shots"]);
    assert_eq!(first.len(), 1);
    let distinct: std::collections::BTreeSet<&String> = second.iter().collect();
    assert_eq!((second.len(), distinct.len(), &second[0]), (2, 2, &first[0]));
    let taken: Vec<Vec<u8>> = second.iter().map(|shot| std::fs::read(shot).unwrap()).collect();
    assert_eq!(taken, vec![red, blue]);
    let mut kept: Vec<String> = std::fs::read_dir(case.home.join("shots")).unwrap().flatten()
        .map(|entry| entry.file_name().to_string_lossy().to_string()).collect();
    let mut named: Vec<String> = second.iter().map(|shot| Path::new(shot).file_name().unwrap().to_string_lossy().to_string()).collect();
    kept.sort();
    named.sort();
    assert_eq!(kept, named);
    assert!(named.iter().all(|name| name.starts_with(&format!("{ticket}-1-"))), "{named:?}");
}

#[test]
fn the_mouse_answers_too_and_a_click_beside_the_comment_ends_it() {
    let case = Case::new("mouse");
    let ticket = case.queue(&["--title", "Окно", "--check", "1. Раз", "--check", "2. Два", "--check", "3. Три"]);
    case.play(&[
        "click Работает".into(), "click Пропустить".into(), "click Не работает".into(), dump(&case, "no.json"),
        "click ‹".into(), dump(&case, "back.json"), "click Готово".into(),
    ]);
    let (no, back) = (case.seen("no.json"), case.seen("back.json"));
    assert_eq!((no["index"].as_i64(), no["editing"].as_bool()), (Some(2), Some(true)));
    assert_eq!((back["index"].as_i64(), back["editing"].as_bool()), (Some(1), Some(false)));
    let verdict = case.verdict(&ticket);
    let states: Vec<Value> = verdict["results"].as_array().unwrap().iter().map(|mark| mark["state"].clone()).collect();
    assert_eq!((verdict["how"].as_str(), states), (Some("done"), vec![json!("ok"), json!("skipped"), json!("problem")]));
}

#[test]
fn a_no_is_counted_and_goes_on_without_opening_the_comment() {
    for (kind, no) in [("plan", "Нет"), ("result", "Не ок")] {
        let case = Case::new(&format!("no-{kind}"));
        case.queue(&["--title", "Окно", "--kind", kind, "--check", "1. Раз", "--check", "2. Два"]);
        case.play(&[format!("click {no}"), dump(&case, "after.json")]);
        let after = case.seen("after.json");
        assert_eq!((after["index"].as_i64(), after["editing"].as_bool(), after["marks"][0]["state"].as_str()),
                   (Some(1), Some(false), Some("problem")), "{kind}: {after}");
    }
}

#[test]
fn a_failed_checklist_item_keeps_the_comment_on_the_same_item() {
    let case = Case::new("failed-checklist");
    case.queue(&["--title", "Окно", "--kind", "plan", "--check", "1. Раз", "--as", "checklist", "--check", "2. Два"]);
    case.play(&[
        "click Не работает".into(), dump(&case, "failed.json"), "text Задержка".into(),
        "click ›".into(), dump(&case, "next.json"),
        "click ‹".into(), "click Не работает".into(), dump(&case, "already-filled.json"),
    ]);
    let failed = case.seen("failed.json");
    assert_eq!((failed["index"].as_i64(), failed["editing"].as_bool(), failed["marks"][0]["state"].as_str()),
               (Some(0), Some(true), Some("problem")));
    let next = case.seen("next.json");
    assert_eq!((next["index"].as_i64(), next["editing"].as_bool(), next["marks"][0]["note"].as_str()),
               (Some(1), Some(false), Some("Задержка")));
    let filled = case.seen("already-filled.json");
    assert_eq!((filled["index"].as_i64(), filled["editing"].as_bool(), filled["marks"][0]["note"].as_str()),
               (Some(0), Some(false), Some("Задержка")));
}

#[test]
fn a_free_text_answer_focuses_only_an_empty_comment_and_stays_on_its_item() {
    let case = Case::new("free-text-answer");
    case.queue(&["--title", "Выбор", "--kind", "question",
                 "--check", "1. Какой вариант?", "--option", "Первое", "--option", "Второе",
                 "--check", "2. Продолжить?"]);
    case.play(&[
        "click Другое, напишу сам".into(), dump(&case, "empty.json"), "text Свой вариант".into(),
        "click ›".into(), "click ‹".into(), "click Другое, напишу сам".into(), dump(&case, "filled.json"),
        "click Первое".into(), dump(&case, "next.json"),
    ]);
    let empty = case.seen("empty.json");
    assert_eq!((empty["index"].as_i64(), empty["editing"].as_bool(), empty["marks"][0]["state"].as_str()),
               (Some(0), Some(true), Some("problem")));
    let filled = case.seen("filled.json");
    assert_eq!((filled["index"].as_i64(), filled["editing"].as_bool(), filled["marks"][0]["note"].as_str()),
               (Some(0), Some(false), Some("Свой вариант")));
    let next = case.seen("next.json");
    assert_eq!((next["index"].as_i64(), next["marks"][0]["choice"].as_str()), (Some(1), Some("Первое")));
}

#[test]
fn the_keys_light_up_with_the_keyboard_and_the_window_keeps_its_size() {
    let case = Case::new("keys");
    case.queue(&["--title", "Окно", "--check", "1. Раз", "--check", "2. Два", "--link", "https://example.com/pull/1"]);
    case.play(&[
        dump(&case, "without.json"), snap(&case, "without.png"), "keyboard".into(), dump(&case, "with.json"), snap(&case, "with.png"),
    ]);
    let (without, with_keys) = (case.seen("without.json"), case.seen("with.json"));
    assert_eq!((without["keysLive"].as_bool(), with_keys["keysLive"].as_bool()), (Some(false), Some(true)));
    // Nothing is written about it: a line that came and went with the keyboard resized the window under the click
    // that brought the keyboard, and the click missed its button.
    assert_eq!((without["hint"].as_str(), with_keys["hint"].as_str()), (Some(""), Some("")));
    assert_eq!((&without["height"], &without["window"]), (&with_keys["height"], &with_keys["window"]));
    assert_ne!(std::fs::read(case.at("without.png")).unwrap(), std::fs::read(case.at("with.png")).unwrap(),
               "the keys look the same lit and not");
}

#[test]
fn a_site_opens_in_the_web_browser_and_another_app_s_link_in_that_app() {
    // 2026-10-21: a task's link to realize.day opened in the Realize build that test runs put on this Mac, which
    // claims the site, and not on the site: «почему то в дев сборке приложения а не на сайте».
    let case = Case::new("links");
    let (site, app) = ("https://realize.day/r/team/445", "obsidian://open?vault=x");
    case.queue(&["--title", "Ссылки", "--check", "1. Открой", "--link", site, "--link", app]);
    case.play(&["click realize.day".into(), "click obsidian".into(), dump(&case, "links.json")]);
    let seen = case.seen("links.json");
    assert_eq!((strings(&seen["opened"]), strings(&seen["browsed"])), (vec![site.to_string(), app.to_string()], vec![site.to_string()]));
}

#[test]
fn two_waiting_checks_of_one_chat_are_told_apart() {
    let case = Case::new("apart");
    case.queue(&["--title", "Сейчас", "--tag", "покупки", "--check", "1. Пункт"]);
    case.queue(&["--title", "План: скачивание вложений", "--kind", "plan", "--check", "1. Раз", "--check", "2. Два"]);
    case.queue(&["--title", "Лента изменений: результат", "--kind", "result", "--check", "1. Раз", "--check", "2. Два"]);
    case.queue(&["--title", "Вход плагина: результат", "--kind", "result", "--check", "1. Раз"]);
    case.play(&[dump(&case, "queue.json")]);
    // By the chat and the kind of check; where those are the same, by the check's own name.
    let queue = strings(&case.seen("queue.json")["queue"]);
    assert_eq!(queue[..2], ["r/66 · план".to_string(), "r/66 · Лента изменений: резуль…".to_string()]);
    let distinct: std::collections::BTreeSet<&String> = queue.iter().collect();
    assert_eq!(distinct.len(), queue.len(), "{queue:?}");
    // Sent in one second, none replaced another.
    let queued = std::fs::read_dir(case.home.join("queue")).unwrap().flatten()
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".json")).count();
    assert_eq!(queued, 4);
}

#[test]
fn the_check_that_is_on_the_phone_comes_forward_once_and_what_was_being_answered_waits() {
    let case = Case::new("phone");
    let own = case.queue(&["--title", "Своя", "--check", "1. Раз", "--check", "2. Два"]);
    case.phone_check("1009-120000-001-r38");
    case.play(&[
        dump(&case, "phone.json"), snap(&case, "phone.png"), "click r/66 · чек-лист".into(), dump(&case, "own.json"),
        "reload".into(), dump(&case, "stays.json"),
    ]);
    let (phone, mine) = (case.seen("phone.json"), case.seen("own.json"));
    // A window that comes up with both shows the phone's first, though it was queued later.
    assert_eq!((phone["id"].as_str(), phone["onPhone"].as_bool(), phone["kind"].as_str(), phone["origin"].as_str()),
               (Some("1009-120000-001-r38"), Some(true), Some("на телефоне XYZC"), Some("r/38 · Бары внизу")));
    assert_eq!(strings(&phone["queue"]), ["r/66 · чек-лист"]);
    // The phone's panel has three marks, so «не понял» is not offered for its check; for the window's own it is.
    let (answers, keys) = (strings(&mine["answers"]), strings(&mine["keys"]));
    assert_eq!((strings(&phone["answers"]), answers.last().map(String::as_str), keys.last().map(String::as_str)),
               (vec!["Работает".to_string(), "Не работает".into(), "Пропустить".into()], Some("Не понял"), Some("0")));
    // He goes to another check by his own hand, and is not pulled back to the phone's.
    assert_eq!((mine["id"].as_str(), mine["onPhone"].as_bool(), case.seen("stays.json")["id"].as_str()),
               (Some(own.as_str()), Some(false), Some(own.as_str())));
    assert!(!std::fs::read(case.at("phone.png")).unwrap().is_empty());
}

/// The redesign of 2026-10-20: the folder and the branch under the chat's name in the head, gone from the folded
/// line; «Не понял» the quiet one of the answers; «Готово» the white button once every item is answered.
#[test]
fn head_says_where_from_and_answers_are_buttons() {
    let case = Case::new("head");
    case.queue(&["--title", "Окно", "--kind", "question", "--check", "1. Удалить пустую запись?", "--option", "Да", "--option", "Нет"]);
    case.play(&[
        dump(&case, "open.json"), "click Да".into(), dump(&case, "answered.json"), "click Свернуть".into(), dump(&case, "folded.json"),
    ]);
    let (opened, answered, folded) = (case.seen("open.json"), case.seen("answered.json"), case.seen("folded.json"));
    let from = opened["from"].as_str().unwrap_or("");
    assert!(from.contains(" · ⎇ "), "{from:?}");
    assert_eq!((opened["originShown"].as_bool(), folded["originShown"].as_bool()), (Some(true), Some(false)));
    assert_eq!(strings(&opened["quietAnswers"]), ["Не понял"]);
    assert_eq!((opened["doneReady"].as_bool(), answered["doneReady"].as_bool()), (Some(false), Some(true)));
}

/// The app a check came from stands before the project as its own icon, and the "Чат" button opens the chat by what the
/// check carries and the setting of the button says: the window names no program. A scripted window opens nothing.
#[test]
fn a_chat_has_its_icon_and_a_button_that_runs_what_the_setting_says() {
    let thread = "019a8f2e-7c41-7d2a-b3e5-0123456789ab";
    // The last is a helper hitl runs: no app shows it, so its button shows its own program in a terminal.
    let helper = "my-chat://Codex/5f1c9b2e-0000-4000-8000-0123456789ab".to_string();
    for (agent, link, address, ran) in [("Codex", format!("my-chat://Codex/{thread}"), Some(format!("my-chat://Codex/{thread}")), Some(format!("/bin/echo Codex {thread}"))),
                                        ("OpenCode", format!("my-chat://OpenCode/{thread}"), Some(format!("my-chat://OpenCode/{thread}")), Some(format!("/bin/echo OpenCode {thread}"))),
                                        ("Codex", helper.clone(), Some(helper.clone()), Some("/bin/echo Codex 5f1c9b2e-0000-4000-8000-0123456789ab".to_string())),
                                        ("Claude", "claude://code/session_owner".to_string(), Some("claude://code/session_owner".to_string()), None),
                                        ("Claude", String::new(), None, None)] {
        let mut case = Case::new(&format!("app-{}-{}", agent.to_lowercase(), link.len()));
        let folder = case.root.join("named");
        std::fs::create_dir_all(&folder).unwrap();
        let folder_text = folder.display().to_string();
        let codex = rusqlite::Connection::open(case.root.join("codex.sqlite")).unwrap();
        codex.execute_batch("create table threads (id text,title text,name text,cwd text,source text,archived integer)").unwrap();
        codex.execute("insert into threads values (?,?,?,?,?,?)", (thread, "Работа в Codex", "", &folder_text, "cli", 0)).unwrap();
        let oc = rusqlite::Connection::open(case.root.join("oc.sqlite")).unwrap();
        oc.execute_batch("create table session_v2 (id text,title text,directory text,parent_id text,time_archived integer,metadata text)").unwrap();
        oc.execute("insert into session_v2 values (?,?,?,?,?,?)", (thread, "Работа в OpenCode", &folder_text, "", None::<i64>, "{}")).unwrap();
        drop((codex, oc));
        let setting = case.root.join("chat-button.json");
        std::fs::write(&setting, r#"{"open": {"my-chat": ["/bin/echo", "{host}", "{path}"]}}"#).unwrap();
        let place = |name: &str| case.root.join(name).display().to_string();
        case.more = vec![
            ("CONVEYOR_CLAUDE_SESSIONS_DIR".into(), place("desktop")),
            ("CONVEYOR_CLAUDE_PROJECTS_DIR".into(), place("projects")),
            ("CONVEYOR_CODEX_STATE".into(), place("codex.sqlite")),
            ("CONVEYOR_OPENCODE_STATE".into(), place("oc.sqlite")),
            ("CONVEYOR_CHAT_BUTTON".into(), setting.display().to_string()),
        ];
        let ticket = json!({"id": "fixture-check", "agent": agent, "chat": "", "chatIds": [thread], "cwd": folder_text,
                            "project": folder_text, "createdAt": 1, "tag": "", "title": "Fixture", "screen": "mac", "kind": "result",
                            "answers": ["ок", "не ок", "пропустить"], "projectName": "fixture", "commit": "frozen-commit", "chatLink": link,
                            "items": [{"label": "1", "text": "Значок приложения и кнопка «Чат»"}]});
        std::fs::create_dir_all(case.home.join("queue")).unwrap();
        std::fs::write(case.queued("fixture-check"), ticket.to_string()).unwrap();
        case.play(&["names".into(), dump(&case, "shown.json"), snap(&case, "header.png"), "click Чат".into(), dump(&case, "after.json")]);
        let shown = case.seen("shown.json");
        assert_eq!(shown["app"].as_str(), Some(agent));
        // The icon is the installed app's own; on a Mac without the app the agent's name stands in its place.
        assert!(matches!(shown["appWord"].as_str(), Some("") | Some("Codex") | Some("OpenCode")), "{}", shown["appWord"]);
        if agent != "Claude" {
            assert_eq!(shown["origin"].as_str().map(|origin| origin.contains(&format!("Работа в {agent}"))), Some(true), "{}", shown["origin"]);
        }
        let opens = strings(&shown["openAddresses"]);
        assert_eq!(opens, address.clone().into_iter().collect::<Vec<_>>(), "{agent}");
        // The button opens what it offers; a scripted window runs nothing, and says what the setting would have run.
        let after = case.seen("after.json");
        assert_eq!(strings(&after["opened"]), address.into_iter().collect::<Vec<_>>(), "{agent}");
        assert_eq!(strings(&after["ran"]), ran.into_iter().collect::<Vec<_>>(), "{agent}");
        if let Some(keep) = std::env::var_os("CONVEYOR_KEEP_SNAPS") {
            std::fs::copy(case.at("header.png"), Path::new(&keep).join(format!("header-{agent}-{}.png", link.len()))).unwrap();
        }
    }
}

#[test]
fn a_check_queued_before_the_setting_opens_through_it_and_without_it_as_any_address() {
    // A check on disk carries the address it was queued with; the setting is read when the button is pressed.
    let old = "conveyor-chat://Codex/019a8f2e-7c41";
    for (name, setting, ran) in [("with", Some(r#"{"open": {"conveyor-chat": ["/bin/echo", "reveal", "{host}", "{path}"]}}"#), vec!["/bin/echo reveal Codex 019a8f2e-7c41".to_string()]),
                                 ("without", None, vec![])] {
        let mut case = Case::new(&format!("old-{name}"));
        let file = case.root.join("chat-button.json");
        match setting {
            Some(text) => std::fs::write(&file, text).unwrap(),
            None => {}
        }
        case.more = vec![("CONVEYOR_CHAT_BUTTON".into(), file.display().to_string())];
        let ticket = json!({"id": "fixture-check", "agent": "Codex", "chat": "", "chatIds": [], "cwd": "", "project": "", "createdAt": 1, "tag": "",
                            "title": "Fixture", "screen": "mac", "kind": "result", "answers": ["ок", "не ок", "пропустить"], "projectName": "fixture",
                            "commit": "frozen-commit", "chatLink": old, "items": [{"label": "1", "text": "Кнопка «Чат»"}]});
        std::fs::create_dir_all(case.home.join("queue")).unwrap();
        std::fs::write(case.queued("fixture-check"), ticket.to_string()).unwrap();
        case.play(&["names".into(), "click Чат".into(), dump(&case, "after.json")]);
        let after = case.seen("after.json");
        assert_eq!(strings(&after["opened"]), [old], "{name}");
        assert_eq!(strings(&after["ran"]), ran, "{name}");
    }
}
