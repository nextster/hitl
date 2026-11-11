//! `hitl queue`, `wait`, `drop` and `last-check`: one check, found on either screen, as Python's `checks.py`
//! was. The window's side is hitl's (`hitl::queue`), a phone's is `hitl-queues`'; this only chooses between them.

use hitl_host_core::{dumps_line, say, text_of};
use hitl_queues::state::Queue;
use serde_json::{Value, json};

fn number(value: Option<&Value>) -> f64 {
    value.and_then(Value::as_f64).unwrap_or(0.0)
}

/// The one queue from where he sits: the answers the calling chat has not read and the window (hitl's), then each
/// phone's queue, then the lines of test runs beside them (checks.show).
pub fn show() -> i32 {
    hitl::queue::prepare();
    hitl::queue::show();
    let phones = hitl_queues::commands::known_queues();
    for phone in &phones {
        let text = phone.locked(|state| {
            if hitl_host_core::truthy(state.get("active")) || !state["queue"].as_array().is_none_or(Vec::is_empty) {
                phone.ensure_dispatcher();
            }
            hitl_queues::commands::status_text(phone, state)
        });
        println!("\n{text}");
    }
    if phones.is_empty() {
        println!("\nТелефон: проверок на него ещё не ставили.");
    }
    let others = hitl_queues::lines::other_lines();
    if !others.is_empty() {
        println!("\n{}", others.join("\n"));
    }
    0
}

/// `hitl queue --watch`: the window, the phones and the lines of tests, drawn again every second on
/// the whole terminal until Ctrl-C (his «да» of 2026-11-04 to a picture of it).
pub fn watch() -> i32 {
    hitl::queue::prepare();
    loop {
        let mut lines = vec![format!("hitl · очереди · {} · Ctrl-C — выйти", hitl_host_core::clock(Some(hitl_host_core::now()))), String::new()];
        lines.extend(hitl::queue::queue_lines());
        for phone in hitl_queues::commands::known_queues() {
            lines.push(String::new());
            lines.extend(phone.locked(|state| hitl_queues::commands::status_text(&phone, state)).lines().map(str::to_string));
        }
        let others = hitl_queues::lines::other_lines();
        if !others.is_empty() {
            lines.push(String::new());
            lines.extend(others);
        }
        // Home, the screen anew, then the lines, each ended by clearing what an older and longer one left there.
        let mut screen = String::from("\x1b[H");
        for line in &lines {
            screen.push_str(line);
            screen.push_str("\x1b[K\n");
        }
        screen.push_str("\x1b[J");
        {
            use std::io::Write;
            let mut out = std::io::stdout();
            let _ = out.write_all(screen.as_bytes());
            let _ = out.flush();
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

/// `hitl queue --brief`: the queues in a few short lines, for test runs with their progress, then the window and the phones when they have something.
pub fn brief() -> i32 {
    let mut lines = hitl_queues::lines::brief_lines();
    let waiting = hitl::window::queued().len();
    if waiting > 0 {
        lines.push(format!("окно  {waiting} {}", hitl_host_core::plural(waiting as i64, "проверка ждёт", "проверки ждут", "проверок ждут")));
    }
    for phone in hitl_queues::commands::known_queues() {
        let (active, queued) = phone.locked(|state| {
            (hitl_host_core::truthy(state.get("active")), state["queue"].as_array().map(Vec::len).unwrap_or(0))
        });
        if active || queued > 0 {
            let more = if queued > 0 { format!(", ещё {queued} в очереди") } else { String::new() };
            lines.push(format!("тел   {}{more}", if active { "проверка на нём" } else { "свободен" }));
        }
    }
    println!("{}", lines.join("\n"));
    0
}

/// The newest such check of any phone, by its number or its chat, or the caller's own (checks.on_a_phone).
fn on_a_phone(target: Option<&str>, waiting: bool) -> Option<(Queue, Value)> {
    let mut found: Option<(Queue, Value)> = None;
    for phone in hitl_queues::commands::known_queues() {
        let ticket = phone.locked(|state| match target {
            Some(target) => hitl_queues::state::find_ticket(state, Some(target)),
            None => hitl_queues::commands::own_ticket(state),
        });
        let Some(ticket) = ticket else { continue };
        if waiting && !matches!(ticket.get("state").and_then(Value::as_str), Some("queued" | "active")) {
            continue;
        }
        if found.as_ref().is_none_or(|(_, newest)| number(ticket.get("submittedAt")) > number(newest.get("submittedAt"))) {
            found = Some((phone, ticket));
        }
    }
    found
}

/// One check for a number or a chat: a number names it exactly, a chat means its newest on either screen
/// (checks.pick).
fn pick(target: Option<&str>, waiting: bool) -> (Option<Value>, Option<(Queue, Value)>) {
    let mac = hitl::queue::in_window(target, waiting);
    let phone = on_a_phone(target, waiting);
    let (Some(window), Some((_, on_phone))) = (&mac, &phone) else { return (mac, phone) };
    let keep_mac = if target == Some(text_of(window, "id")) {
        true
    } else if target == Some(text_of(on_phone, "id")) {
        false
    } else {
        number(window.get("createdAt")) >= number(on_phone.get("submittedAt"))
    };
    if keep_mac { (mac, None) } else { (None, phone) }
}

/// `hitl wait [ID|TAG]` (checks.wait).
pub fn wait(target: Option<&str>) -> i32 {
    hitl::queue::prepare();
    let (mac, phone) = pick(target, false);
    if let Some(mac) = mac {
        return hitl::queue::wait(Some(text_of(&mac, "id")));
    }
    if let Some((queue, ticket)) = phone {
        return hitl_queues::commands::wait_for(&queue, text_of(&ticket, "id"));
    }
    say(&format!("hitl: {} Очередь: hitl queue", if target.is_some() {
        "такой проверки нет ни в окне на Маке, ни на телефоне."
    } else {
        "у этого чата нет проверки; назови её номер или метку чата."
    }));
    hitl::queue::EXIT_NOT_CHECKED
}

/// `hitl drop [ID|TAG] [--note TEXT] [--force]` (cli.drop and checks.drop): a chat takes back only its own
/// check; another chat's is the reviewer's to drop, or his word to.
pub fn drop(arguments: &[String]) -> i32 {
    let (mut target, mut note, mut force): (Option<String>, Option<String>, bool) = (None, None, false);
    let mut position = 0;
    while position < arguments.len() {
        let argument = &arguments[position];
        if argument == "--force" {
            force = true;
        } else if argument == "--note" && position + 1 < arguments.len() {
            position += 1;
            note = Some(arguments[position].clone());
        } else if let Some(given) = argument.strip_prefix("--note=") {
            note = Some(given.to_string());
        } else if !argument.starts_with('-') && target.is_none() {
            target = Some(argument.clone());
        } else {
            say("usage: hitl drop [ID|TAG] [--note TEXT] [--force]");
            return hitl_host_core::EXIT_REFUSED;
        }
        position += 1;
    }
    hitl::queue::prepare();
    let target = target.as_deref().filter(|target| !target.is_empty());
    let (mac, phone) = pick(target, true);
    if let Some(mac) = mac {
        let mut words = vec![text_of(&mac, "id").to_string()];
        if let Some(note) = &note {
            words.extend(["--note".to_string(), note.clone()]);
        }
        if force {
            words.push("--force".into());
        }
        return hitl::queue::drop_command(&words);
    }
    if let Some((queue, ticket)) = phone {
        let mut words = vec!["drop".to_string(), "--udid".into(), queue.udid.clone(), text_of(&ticket, "id").to_string()];
        if let Some(note) = &note {
            words.push(format!("--note={note}"));
        }
        if force {
            words.push("--force".into());
        }
        return hitl_queues::commands::run(&words);
    }
    say(&match target {
        Some(target) => format!("hitl: «{target}» не ждёт ни в окне на Маке, ни на телефоне."),
        None => "hitl: у этого чата нет ждущей проверки; назови её номер или метку чата.".to_string(),
    });
    1
}

/// `hitl last-check --branch BRANCH [--screen mac|iphone]` (cli.last_check and checks.last_check): the check of a
/// branch that ended last, on either screen, as a gate reads it.
pub fn last_check(arguments: &[String]) -> i32 {
    let (mut branch, mut screen): (String, Option<String>) = (String::new(), None);
    let mut position = 0;
    while position < arguments.len() {
        let argument = arguments[position].as_str();
        if (argument == "--branch" || argument == "--screen") && position + 1 < arguments.len() {
            if argument == "--branch" {
                branch = arguments[position + 1].clone();
            } else {
                screen = Some(arguments[position + 1].clone());
            }
            position += 2;
        } else {
            branch.clear();
            break;
        }
    }
    if branch.is_empty() || !matches!(screen.as_deref(), None | Some("mac") | Some("iphone")) {
        say("usage: hitl last-check --branch BRANCH [--screen mac|iphone]");
        return hitl_host_core::EXIT_REFUSED;
    }
    let mut found: Vec<Value> = Vec::new();
    if matches!(screen.as_deref(), None | Some("mac")) {
        if let Some(mut mac) = hitl::queue::last_check(&branch, None, None) {
            let when = mac.get("when").cloned().unwrap_or(Value::Null);
            if let Some(map) = mac.as_object_mut() {
                map.insert("when".into(), json!(hitl_host_core::clock(when.as_f64())));
                map.insert("finishedAt".into(), when);
            }
            found.push(mac);
        }
    }
    if matches!(screen.as_deref(), None | Some("iphone")) {
        if let Some(entry) = hitl_queues::state::last_check(&branch, None) {
            let finished = entry.get("finishedAt").cloned().unwrap_or(Value::Null);
            found.push(json!({
                "id": entry.get("id"), "screen": "iphone", "tag": entry.get("tag"), "title": entry.get("title"),
                "branch": entry.get("branch"), "commit": entry.get("commit").and_then(Value::as_str).unwrap_or(""),
                "finishedAt": finished, "when": hitl_host_core::clock(finished.as_f64()),
                "how": entry.get("how"), "summary": entry.get("summary"),
                "green": hitl_queues::state::is_green(&entry),
            }));
        }
    }
    match found.into_iter().max_by(|a, b| number(a.get("finishedAt")).total_cmp(&number(b.get("finishedAt")))) {
        Some(found) => {
            println!("{}", dumps_line(&found).trim_end());
            0
        }
        None => {
            println!("null");
            1
        }
    }
}
