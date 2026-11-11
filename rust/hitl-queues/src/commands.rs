//! `hitl _queues`: the commands of the phone's queue and of the turns (queues.py `main`, `build_parser`, and the
//! `command_*` functions).
//!
//! `run` is queues.main(argv): the first word is the command ("check", "wait", "probe-begin", "probe-end", "peek",
//! "status", "release", "pause", "resume", "first", "drop", "mac-begin", "mac-end",
//! "last-check", "dispatch", "chat-tag"), the rest its options. A refusal of the parser is exit 2 with argparse's
//! words on stderr.

use crate::base::{EXIT_USAGE, list_mut, number, text};
use crate::bridge;
use crate::dispatcher;
use crate::items::{self, CHECKLIST_ARGUMENT};
use crate::lines::{self, Interrupts};
use crate::state::*;
use crate::verdict::{finish, prune, verdict_text};
use crate::base::{now, say};
use crate::out;
use hitl_host_core::{EXIT_BUSY, clock, env, items_word, minutes, plural};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;

// --------------------------------------------------------------------------
// The options of a command, read as argparse reads them

/// What one option of a command is.
struct Spec {
    name: &'static str,
    /// Takes a value (else it is a flag).
    value: bool,
    /// May be given more than once.
    many: bool,
    required: bool,
}

const fn flag(name: &'static str) -> Spec {
    Spec { name, value: false, many: false, required: false }
}
const fn option(name: &'static str) -> Spec {
    Spec { name, value: true, many: false, required: false }
}
const fn required(name: &'static str) -> Spec {
    Spec { name, value: true, many: false, required: true }
}
const fn repeated(name: &'static str) -> Spec {
    Spec { name, value: true, many: true, required: false }
}

/// The options given: `--name value` and `--name=value`, a unique beginning of a name as argparse allows, and
/// everything after `--` is a word of the positional kind.
#[derive(Default, Debug)]
struct Given {
    values: HashMap<&'static str, Vec<String>>,
    flags: Vec<&'static str>,
    words: Vec<String>,
}

impl Given {
    fn one(&self, name: &str) -> Option<&str> {
        self.values.get(name).and_then(|values| values.last()).map(String::as_str)
    }
    fn text(&self, name: &str) -> &str {
        self.one(name).unwrap_or("")
    }
    fn all(&self, name: &str) -> Vec<String> {
        self.values.get(name).cloned().unwrap_or_default()
    }
    fn flag(&self, name: &str) -> bool {
        self.flags.contains(&name)
    }
    fn number(&self, name: &str) -> i64 {
        self.one(name).and_then(|value| value.parse().ok()).unwrap_or(0)
    }
}

fn parse(command: &str, arguments: &[String], specs: &[Spec], positional: (usize, Option<usize>)) -> Result<Given, i32> {
    let usage_error = |message: String| -> Result<Given, i32> {
        say(&format!("usage: hitl _queues {command} [-h] ..."));
        say(&format!("hitl _queues {command}: error: {message}"));
        Err(EXIT_USAGE)
    };
    let mut given = Given::default();
    let mut position = 0;
    while position < arguments.len() {
        let argument = arguments[position].as_str();
        position += 1;
        if argument == "--" {
            given.words.extend(arguments[position..].iter().cloned());
            break;
        }
        if !argument.starts_with("--") || argument.len() == 2 {
            given.words.push(argument.to_string());
            continue;
        }
        let (head, attached) = match argument.split_once('=') {
            Some((head, value)) => (head, Some(value.to_string())),
            None => (argument, None),
        };
        let wanted = &head[2..];
        let exact = specs.iter().find(|spec| spec.name == wanted);
        let candidates: Vec<&Spec> = specs.iter().filter(|spec| spec.name.starts_with(wanted)).collect();
        let spec = match (exact, candidates.as_slice()) {
            (Some(spec), _) => spec,
            (None, [only]) => only,
            (None, []) => return usage_error(format!("unrecognized arguments: {argument}")),
            (None, many) => {
                let names: Vec<String> = many.iter().map(|spec| format!("--{}", spec.name)).collect();
                return usage_error(format!("ambiguous option: {head} could match {}", names.join(", ")));
            }
        };
        if !spec.value {
            if attached.is_some() {
                return usage_error(format!("argument --{}: ignored explicit argument", spec.name));
            }
            given.flags.push(spec.name);
            continue;
        }
        let value = match attached {
            Some(value) => value,
            None if position < arguments.len() => {
                position += 1;
                arguments[position - 1].clone()
            }
            None => return usage_error(format!("argument --{}: expected one argument", spec.name)),
        };
        let slot = given.values.entry(spec.name).or_default();
        if !spec.many {
            slot.clear();
        }
        slot.push(value);
    }
    for spec in specs.iter().filter(|spec| spec.required) {
        if !given.values.contains_key(spec.name) {
            return usage_error(format!("the following arguments are required: --{}", spec.name));
        }
    }
    let (least, most) = positional;
    if given.words.len() < least || most.is_some_and(|most| given.words.len() > most) {
        return usage_error(if given.words.len() < least { "the following arguments are required: ticket".to_string() }
                           else { format!("unrecognized arguments: {}", given.words[most.unwrap_or(0)..].join(" ")) });
    }
    Ok(given)
}

// --------------------------------------------------------------------------
// Who is calling

/// The agent chat this command runs in, or "" for a person at a terminal.
pub fn caller_chat() -> String {
    for name in ["CLAUDE_CODE_HOST_SESSION_ID", "CODEX_THREAD_ID", "OPENCODE_SESSION_ID", "CONVEYOR_CHAT"] {
        if let Some(value) = hitl_host_core::var(name).filter(|value| !value.is_empty()) {
            return value;
        }
    }
    String::new()
}

/// What makes two runs the same chat: its session, else its checkout.
pub fn chat_identity(project: &str) -> String {
    let chat = caller_chat();
    if chat.is_empty() { project.to_string() } else { chat }
}

/// The calling chat's latest check: on the phone, waiting, or just ended.
pub fn own_ticket(state: &Value) -> Option<Value> {
    let chat = caller_chat();
    if chat.is_empty() {
        return None;
    }
    let mut all: Vec<&Value> = Vec::new();
    all.extend(state.get("active").filter(|active| active.is_object()));
    all.extend(state["queue"].as_array().into_iter().flatten());
    all.extend(state["recent"].as_array().into_iter().flatten().rev());
    all.into_iter().find(|ticket| text(ticket, "identity") == chat).cloned()
}

/// A chat ends, drops or moves only its own check. The phone is the reviewer's to hand out: he does it on the phone or in a
/// terminal, or tells a chat to (--force).
fn refuses_foreign(ticket: &Value, force: bool, what: &str) -> bool {
    let chat = caller_chat();
    if chat.is_empty() || force || text(ticket, "identity") == chat {
        return false;
    }
    out!("{} — проверка другого чата: {what} её может Проверяющий, на телефоне или командой в терминале. \
              Если он прямо сказал это сделать тебе, повтори с --force.", describe(ticket));
    true
}

fn new_ticket_id(state: &Value, tag: &str) -> String {
    let mut known: Vec<String> = Vec::new();
    for key in ["queue", "recent"] {
        known.extend(state[key].as_array().into_iter().flatten().map(|ticket| text(ticket, "id").to_string()));
    }
    known.push(text(state.get("active").unwrap_or(&Value::Null), "id").to_string());
    hitl_host_core::new_id(tag, |id| known.iter().any(|one| one == id))
}

/// A copy of the build, so the checkout can go on building while the ticket waits. On APFS the copy shares the blocks
/// and costs nothing.
fn clone_app(source: &Path, destination: &Path) {
    if let Some(parent) = destination.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let done = std::process::Command::new("cp").arg("-cR").arg(source).arg(destination)
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
    if !done.is_ok_and(|status| status.success()) {
        let _ = std::fs::remove_dir_all(destination);
        copy_tree(source, destination);
    }
}

fn copy_tree(source: &Path, destination: &Path) {
    let _ = std::fs::create_dir_all(destination);
    for entry in std::fs::read_dir(source).into_iter().flatten().flatten() {
        let target = destination.join(entry.file_name());
        match entry.file_type() {
            Ok(kind) if kind.is_symlink() => {
                if let Ok(link) = std::fs::read_link(entry.path()) {
                    let _ = std::os::unix::fs::symlink(link, &target);
                }
            }
            Ok(kind) if kind.is_dir() => copy_tree(&entry.path(), &target),
            _ => {
                let _ = std::fs::copy(entry.path(), &target);
            }
        }
    }
}

// --------------------------------------------------------------------------
// Commands: a check

/// What `check` was given, besides the queue's name.
struct Submission {
    title: String,
    items: Vec<String>,
    checklist: String,
    tag: String,
    launch_tag: Option<String>,
    branch: String,
    commit: String,
    project: String,
    bundle_id: String,
    app: String,
    url: String,
    app_arguments: Vec<String>,
    force: bool,
}

fn rewrite(replaced: &mut Value, state: &mut Value, queue: &Queue, ticket_id: &str, error: String) {
    replaced["state"] = json!("superseded");
    replaced["finishedAt"] = json!(now());
    replaced["error"] = json!(error.replace("{id}", ticket_id));
    let _ = std::fs::remove_dir_all(queue.ticket_folder(text(replaced, "id")).join("app"));
    archive(state, replaced.clone());
}

/// The ticket put into the queue; Err is the refusal ("Error: …") a chat reads.
fn submit(queue: &Queue, given: &Submission) -> Result<Value, String> {
    let mut title = given.title.clone();
    let mut raw_items = given.items.clone();
    if !given.checklist.is_empty() {
        let (file_title, file_items) = items::read_checklist_file(&given.checklist).map_err(|error| format!("{error}"))?;
        if title.is_empty() {
            title = file_title;
        }
        raw_items.extend(file_items);
    }
    let items = items::parse_items(&raw_items);
    if items.is_empty() {
        return Err("Error: a check needs at least one item: --check \"что сделать → что должно быть\".".into());
    }
    let tag = {
        let tag = given.tag.trim();
        let branch = given.branch.trim();
        if !tag.is_empty() { tag } else if !branch.is_empty() { branch } else { "без имени" }.to_string()
    };
    let identity = chat_identity(&given.project);
    let source = Path::new(&given.app);
    if !source.is_dir() {
        return Err(format!("Error: built app bundle was not found at '{}'.", source.display()));
    }
    let overlay = items::app_supports_checklist(source);
    let environment = env();
    let ticket = queue.locked(|state| {
        let ticket_id = new_ticket_id(state, &tag);
        let mut ticket = json!({
            "id": ticket_id, "tag": tag, "chat": hitl_host_core::chat_title(&environment), "chatLink": hitl_host_core::chat_link(&environment),
            "agent": hitl_host_core::agent(&environment), "branch": given.branch, "commit": given.commit.trim(),
            "project": given.project, "title": title.split_whitespace().collect::<Vec<_>>().join(" "), "items": items,
            "bundleId": given.bundle_id, "app": "", "appArguments": [],
            "payloadUrl": if given.url.is_empty() { Value::Null } else { json!(given.url) },
            "overlay": overlay, "submittedAt": now(), "state": "queued", "installedAt": null, "finishedAt": null,
            "progress": {"marked": 0, "updatedAt": null}, "result": null, "error": null, "attempts": 0,
            "identity": identity, "chatIds": bridge::chat_ids(),
        });
        let mut launch_arguments: Vec<String> = given.app_arguments.clone();
        if overlay {
            launch_arguments.splice(0..0, [CHECKLIST_ARGUMENT.to_string(), items::checklist_payload(&ticket)]);
        }
        let launch_tag = given.launch_tag.clone().unwrap_or_else(|| tag.clone());
        if !launch_tag.is_empty() {
            launch_arguments.splice(0..0, ["--ios-run-tag".to_string(), launch_tag]);
        }
        ticket["appArguments"] = json!(launch_arguments);
        let destination = queue.ticket_folder(text(&ticket, "id")).join("app").join(source.file_name().unwrap_or_default());
        clone_app(source, &destination);
        ticket["app"] = json!(destination.display().to_string());
        let id = text(&ticket, "id").to_string();

        // A chat has one build waiting: a newer one takes the older one's place in the queue. The build that is on
        // the phone is never touched, unless the newer one is urgent.
        let replaced = state["queue"].as_array().into_iter().flatten()
            .position(|waiting| text(waiting, "tag") == tag && text(waiting, "identity") == identity);
        if given.force {
            ticket["urgent"] = json!(true);
            if let Some(index) = replaced {
                let mut old = list_mut(state, "queue").remove(index);
                rewrite(&mut old, state, queue, &id, "заменена более новой сборкой того же чата ({id})".into());
            }
            let active = state.get("active").filter(|active| active.is_object()).cloned();
            match active {
                Some(mut active) if text(&active, "tag") == tag && text(&active, "identity") == identity => {
                    // Its own earlier build: nothing to come back to.
                    state["active"] = Value::Null;
                    rewrite(&mut active, state, queue, &id, "заменена срочной сборкой того же чата ({id})".into());
                }
                _ => {
                    if let Some(aside) = set_aside(state, &format!("срочная проверка {tag}")) {
                        ticket["note"] = json!(format!("срочная: сняла с телефона {}, та встанет заново следующей", describe(&aside)));
                    }
                }
            }
            if ticket.get("note").is_none() {
                ticket["note"] = json!("срочная: встаёт на телефон первой");
            }
            list_mut(state, "queue").insert(0, ticket.clone());
        } else if let Some(index) = replaced {
            let mut old = state["queue"][index].clone();
            old["state"] = json!("superseded");
            old["finishedAt"] = json!(now());
            old["error"] = json!(format!("заменена более новой сборкой того же чата ({id})"));
            let _ = std::fs::remove_dir_all(queue.ticket_folder(text(&old, "id")).join("app"));
            archive(state, old);
            ticket["note"] = json!("заняла место прежней сборки этого чата в очереди");
            state["queue"][index] = ticket.clone();
        } else {
            list_mut(state, "queue").push(ticket.clone());
            if state.get("active").is_some_and(|active| text(active, "tag") == tag) {
                ticket["note"] = json!("прежняя сборка этого чата ещё проверяется; эта встала в конец очереди");
                let last = list_mut(state, "queue").len() - 1;
                state["queue"][last] = ticket.clone();
            }
        }
        prune(queue, state);
        queue.save(state);
        queue.ensure_dispatcher();
        ticket
    });
    // The wake of the command that waits can be lost: who asked is kept until that chat has printed the verdict.
    remember_phone_check(queue, &ticket);
    bridge::emit("hitl.queued", json!({"request": ticket["id"], "source": "hitl.iphone", "outcome": "queued",
                                       "context": {"items": ticket["items"].as_array().map_or(0, Vec::len)}}));
    Ok(ticket)
}

/// tell.remember(ticket, verdict=…): a check on the phone says where its verdict will lie, the phone's queue keeps it.
fn remember_phone_check(queue: &Queue, ticket: &Value) {
    let verdict = queue.ticket_folder(text(ticket, "id")).join("verdict.json");
    bridge::tell_remember(ticket, &verdict);
}

fn ticket_position(state: &Value, ticket_id: &str) -> (String, Option<Value>) {
    if let Some(active) = state.get("active").filter(|active| text(active, "id") == ticket_id) {
        return ("active".into(), Some(active.clone()));
    }
    for (index, ticket) in state["queue"].as_array().into_iter().flatten().enumerate() {
        if text(ticket, "id") == ticket_id {
            return (format!("queued:{}", index + 1), Some(ticket.clone()));
        }
    }
    for ticket in state["recent"].as_array().into_iter().flatten() {
        if text(ticket, "id") == ticket_id {
            return (text(ticket, "state").to_string(), Some(ticket.clone()));
        }
    }
    ("missing".into(), None)
}

/// Where a ticket stands in a state: "active", "queued:N", its end state, or "missing" (queues.ticket_position).
pub fn position_of(state: &Value, ticket_id: &str) -> (String, Option<Value>) {
    ticket_position(state, ticket_id)
}

/// Follows a ticket to its end and prints the verdict. The chat may stop this at any time; the ticket stays, and
/// `hitl wait` picks it up again.
pub fn wait_for(queue: &Queue, ticket_id: &str) -> i32 {
    let interrupts = Interrupts::catch();
    let mut last = String::new();
    loop {
        let (position, ticket, lines, name) = queue.locked(|state| {
            let (position, ticket) = ticket_position(state, ticket_id);
            let lines = if position.starts_with("queued") { occupancy_lines(state, Some(ticket_id)) } else { Vec::new() };
            let name = Some(text(state, "name")).filter(|name| !name.is_empty()).unwrap_or("телефон").to_string();
            (position, ticket, lines, name)
        });
        let Some(ticket) = ticket else {
            say(&format!("Error: проверки {ticket_id} нет в очереди и среди недавних."));
            return 1;
        };
        let status;
        if position == "active" {
            status = format!("==> {} на {name} с {}: {}. Жду, пока Проверяющий закончит её на телефоне.", describe(&ticket),
                             clock(ticket.get("installedAt").and_then(Value::as_f64)), progress_text(&ticket));
        } else if position.starts_with("queued") {
            let detail = lines.join(" ");
            let waiting = text(&ticket, "waitingFor");
            status = format!("==> {} в очереди на {name}, место {}.{}{}", describe(&ticket), position.split(':').nth(1).unwrap_or(""),
                             if detail.is_empty() { String::new() } else { format!(" {detail}") },
                             if waiting.is_empty() { String::new() } else { format!(" Сейчас: {waiting}.") });
        } else if position == "done" || position == "released" {
            use std::io::Write;
            out!("{}", verdict_text(&ticket, false));
            let _ = std::io::stdout().flush();
            bridge::tell_printed(ticket_id); // the chat has it now
            let following = queue.locked(|state| occupancy_lines(state, None));
            say("");
            say(&format!("==> {}", if following.is_empty() { format!("{name} свободен, очередь пуста.") } else { following.join(" ") }));
            return 0;
        } else {
            let reason = Some(text(&ticket, "error")).filter(|error| !error.is_empty()).unwrap_or(&position).to_string();
            say(&format!("==> {}: проверка не состоялась ({reason}).", describe(&ticket)));
            return if position == "superseded" { 3 } else { 2 };
        }
        if let Some(who) = bridge::tell_comes_by_itself(ticket_id) {
            // A chat nothing wakes would stand here and ask again and again; its verdict comes to it as a message.
            say(&status);
            say(&bridge::tell_note(&who, ticket_id));
            queue.ensure_dispatcher();
            return EXIT_BUSY;
        }
        if status != last {
            say(&status);
            last = status;
        }
        queue.ensure_dispatcher();
        if interrupts.sleep(crate::base::poll_seconds().min(2.0)) {
            say("");
            say(&format!("==> Ожидание прервано; проверка {ticket_id} осталась в очереди. Вернуться: hitl wait {ticket_id}"));
            return 130;
        }
    }
}

// A check that came with items alone is taken, and the chat is reminded once. He said no to refusing it, «надо гибко
// делать, по ситуации», and yes to a line: 8 of 19 chats of Codex had never seen the skill's words on an item's actions.
const NO_ACTIONS: &str = "==> Ни у одного пункта нет экшна. Если Проверяющему до какого-то пункта придётся что-то делать руками (найти задачу, \
дойти до экрана, переключить настройку, завести данные), дай пункту экшн, чтобы упростить ему работу: \
--check-setup, --check-link или --check-button после его --check; что умеет приложение, сказано у него \
(Realize: «Словарь действий» в docs/qa/manual-test-plan.md и ссылки realize://). Новая проверка с экшнами \
займёт место этой в очереди. Не нужны — оставь как есть.";

/// Whether nothing of a check takes him anywhere: no item with an action, a button or a link of the app, no address
/// the build opens at, no launch arguments of the chat's own.
fn brings_nothing(ticket: &Value, app_arguments: &[String]) -> bool {
    let acts = ticket["items"].as_array().into_iter().flatten()
        .any(|item| ["setup", "buttons", "link"].iter().any(|key| item.get(*key).is_some()));
    !(acts || hitl_host_core::truthy(ticket.get("payloadUrl")) || !app_arguments.is_empty())
}

fn command_check(given: &Given) -> i32 {
    let queue = Queue::named(given.text("udid"), given.text("name"), given.text("core-id"));
    let submission = Submission {
        title: given.text("title").to_string(), items: given.all("item"), checklist: given.text("checklist").to_string(),
        tag: given.text("tag").to_string(), launch_tag: given.one("launch-tag").map(str::to_string),
        branch: given.text("branch").to_string(), commit: given.text("commit").to_string(), project: given.text("project").to_string(),
        bundle_id: given.text("bundle-id").to_string(), app: given.text("app").to_string(), url: given.text("url").to_string(),
        app_arguments: given.words.clone(), force: given.flag("force"),
    };
    let ticket = match submit(&queue, &submission) {
        Ok(ticket) => ticket,
        Err(message) => {
            say(&message);
            return 1;
        }
    };
    let count = ticket["items"].as_array().map_or(0, Vec::len) as i64;
    say(&format!("==> Проверка {} ({}) принята, номер {}.", describe(&ticket), items_word(count), text(&ticket, "id")));
    if hitl_host_core::truthy(ticket.get("overlay")) && brings_nothing(&ticket, &submission.app_arguments) {
        say(NO_ACTIONS);
    }
    if hitl_host_core::truthy(ticket.get("urgent")) {
        say("==> --force: сборка идёт в обход очереди. Так можно только по прямой просьбе Проверяющего в этом чате.");
    }
    if !text(&ticket, "note").is_empty() {
        say(&format!("==> Эта сборка {}.", text(&ticket, "note")));
    }
    if !hitl_host_core::truthy(ticket.get("overlay")) {
        say("==> В сборке нет панели чек-листа (ветка без неё): телефон освободит только \
             «hitl iphone --queue-release» после слов Проверяющего в чате. Чек-лист пришли ему в чат сам.");
    }
    // A chat nothing wakes gets its verdict as a message, as for a check of the window: on 2026-10-12 two chats of
    // Codex queued a check for the phone, ended their turns, and he had another chat kick them to his comments.
    bridge::tell_sweep();
    let mut who = bridge::addressee();
    if let Some(found) = who.clone() {
        if !bridge::start_phone(&ticket, &found, &queue.udid) {
            who = None;
        }
    }
    let id = text(&ticket, "id").to_string();
    if who.is_none() {
        let left = bridge::tell_left_waiting(&id);
        if !left.is_empty() {
            say(&left);
        }
    }
    if let Some(who) = who {
        say(&bridge::tell_note(&who, &id));
        out!("{id}");
        return 0;
    }
    if given.flag("no-wait") {
        say(&format!("==> Не жду. Итог: hitl wait {id}"));
        out!("{id}");
        return 0;
    }
    wait_for(&queue, &id)
}

fn command_wait(given: &Given) -> i32 {
    let queue = match resolve_queue(given.one("udid")) {
        Ok(queue) => queue,
        Err(message) => return stop(&message),
    };
    let wanted = given.words.first().map(String::as_str);
    let ticket = queue.locked(|state| if wanted.is_some() { find_ticket(state, wanted) } else { own_ticket(state) });
    let Some(ticket) = ticket else {
        say(&format!("Error: {} Очередь: hitl queue", if wanted.is_some() { "такой проверки нет." } else {
            "у этого чата нет проверки; назови её номер или метку чата." }));
        return 1;
    };
    wait_for(&queue, text(&ticket, "id"))
}

/// A refusal that ended the command in Python with SystemExit(message): said on stderr, exit 1.
fn stop(message: &str) -> i32 {
    say(message);
    1
}

// --------------------------------------------------------------------------
// Commands: a probe

fn command_probe_begin(given: &Given) -> i32 {
    let queue = Queue::named(given.text("udid"), given.text("name"), given.text("core-id"));
    let tag = Some(given.text("tag").trim()).filter(|tag| !tag.is_empty()).unwrap_or("?").to_string();
    let pid = given.number("pid");
    let (force, no_wait) = (given.flag("force"), given.flag("no-wait"));
    let what = given.text("what").to_string();
    let interrupts = Interrupts::catch();
    let mut last = String::new();
    loop {
        enum Step {
            Go(i32),
            Wait(String),
        }
        let step = queue.locked(|state| {
            let probe = live_probe(state);
            let paused = state.get("paused").filter(|paused| hitl_host_core::truthy(Some(paused))).cloned();
            let busy = hitl_host_core::truthy(state.get("active")) || !state["queue"].as_array().is_none_or(Vec::is_empty)
                || probe.as_ref().is_some_and(|probe| probe.get("pid").and_then(Value::as_i64) != Some(pid))
                || paused.as_ref().is_some_and(|paused| !paused["only"].as_array().is_some_and(|only| only.iter().any(|one| one.as_str() == Some(&tag))));
            if force && busy {
                // Urgent: the phone is taken now, and the check that was on it comes back when this run ends.
                let aside = set_aside(state, &format!("срочная проба {tag}"));
                state["probe"] = json!({"pid": pid, "tag": tag, "since": now(), "what": what, "urgent": true});
                say(&format!("==> Срочная проба: телефон твой сейчас, в обход очереди.{}", aside.map_or(String::new(), |aside|
                    format!(" {} снята с телефона и встанет заново, когда этот запуск закончится.", describe(&aside)))));
                return Step::Go(0);
            }
            if !busy {
                state["probe"] = json!({"pid": pid, "tag": tag, "since": now(), "what": what});
                return Step::Go(0);
            }
            Step::Wait(occupancy_lines(state, None).join(" "))
        });
        let status = match step {
            Step::Go(code) => return code,
            Step::Wait(status) => status,
        };
        if no_wait {
            say(&format!("==> Телефон занят, проба не запущена. {status}"));
            return EXIT_BUSY;
        }
        if status != last {
            say(&format!("==> Проба ждёт, пока телефон освободится. {status}"));
            say("==> Пробы идут только при пустой очереди проверок. Не ждать: --no-wait. Собрать и проверить без телефона: ios-run.sh --mac.");
            last = status;
        }
        queue.ensure_dispatcher();
        if interrupts.sleep(crate::base::poll_seconds().min(2.0)) {
            return 130;
        }
    }
}

fn command_probe_end(given: &Given) -> i32 {
    let queue = Queue::new(given.text("udid"));
    if !queue.state_path().exists() {
        return 0;
    }
    let pid = given.number("pid");
    queue.locked(|state| {
        let probe = state.get("probe").filter(|probe| probe.is_object()).cloned();
        if let Some(probe) = probe.filter(|probe| probe.get("pid").and_then(Value::as_i64) == Some(pid)) {
            state["probe"] = Value::Null;
            if let Some(preempted) = probe.get("preemptedBy").filter(|by| hitl_host_core::truthy(Some(by))) {
                say(&format!("==> Пробу вытеснила проверка {}: то, что ты запустил на телефоне, заменено её сборкой.",
                             preempted.as_str().unwrap_or("")));
            }
        }
    });
    0
}

// --------------------------------------------------------------------------
// Commands: the queue from the terminal

pub fn known_queues() -> Vec<Queue> {
    let root = crate::base::state_root();
    let mut folders: Vec<std::path::PathBuf> = std::fs::read_dir(&root).into_iter().flatten().flatten().map(|entry| entry.path()).collect();
    folders.sort();
    folders.into_iter().filter(|folder| folder.join("state.json").exists())
        .map(|folder| Queue::new(&folder.file_name().unwrap_or_default().to_string_lossy())).collect()
}

/// The phone a terminal command means: named by UDID, CoreDevice id or name, or the only one there is. Err is the
/// refusal Python raised as SystemExit.
pub fn resolve_queue(key: Option<&str>) -> Result<Queue, String> {
    let queues = known_queues();
    if let Some(key) = key.filter(|key| !key.is_empty()) {
        for queue in &queues {
            let state = queue.load();
            if key == queue.udid || key == text(&state, "coreId") || key.to_lowercase() == text(&state, "name").to_lowercase() {
                return Ok(queue.clone());
            }
        }
        return Ok(Queue::new(key));
    }
    let busy: Vec<&Queue> = queues.iter().filter(|queue| {
        let state = queue.load();
        hitl_host_core::truthy(state.get("active")) || !state["queue"].as_array().is_none_or(Vec::is_empty)
    }).collect();
    if busy.len() == 1 {
        return Ok(busy[0].clone());
    }
    if queues.len() == 1 {
        return Ok(queues[0].clone());
    }
    if queues.is_empty() {
        return Err("Очереди нет: на телефон ещё не ставили проверок.".into());
    }
    Err("Телефонов с очередью несколько: добавь --device-id.".into())
}

pub fn status_text(queue: &Queue, state: &mut Value) -> String {
    let name = Some(text(state, "name")).filter(|name| !name.is_empty()).unwrap_or(&queue.udid).to_string();
    let mut lines = vec![name];
    let active = state.get("active").filter(|active| active.is_object()).cloned();
    let probe = live_probe(state);
    if let Some(active) = &active {
        let installed = active.get("installedAt").and_then(Value::as_f64);
        lines.push(format!("  На телефоне: {}, с {} ({}), {}", describe(active), clock(installed),
                           minutes(now() - installed.unwrap_or_else(now)), progress_text(active)));
        for item in active["items"].as_array().into_iter().flatten() {
            lines.push(format!("      {}. {}", text(item, "label"), text(item, "text")));
        }
    } else if let Some(probe) = &probe {
        let tag = Some(text(probe, "tag")).filter(|tag| !tag.is_empty()).unwrap_or("?");
        lines.push(format!("  На телефоне: проба чата {tag} с {}", clock(probe.get("since").and_then(Value::as_f64))));
    } else {
        lines.push("  На телефоне: проверок нет, телефон свободен".into());
    }
    if hitl_host_core::truthy(state.get("paused")) {
        lines.push(format!("  {} Снять: hitl iphone --queue-resume",
                           pause_text(state).replace(" Паузу поставил Проверяющий, он же её снимет.", "")));
    }
    let waiting: Vec<Value> = state["queue"].as_array().cloned().unwrap_or_default();
    if !waiting.is_empty() {
        lines.push("  Очередь:".into());
        for (index, ticket) in waiting.iter().enumerate() {
            let mut extra = if text(ticket, "waitingFor").is_empty() { String::new() } else { format!(" · {}", text(ticket, "waitingFor")) };
            if !installable(state, ticket) {
                extra = " · на паузе".into();
            }
            if hitl_host_core::truthy(ticket.get("urgent")) {
                extra += " · срочная";
            }
            if hitl_host_core::truthy(ticket.get("setAsideBy")) {
                extra += &format!(" · снята с телефона: {}", text(ticket, "setAsideBy"));
            }
            lines.push(format!("    {}. {} · {} · ждёт с {}{extra}", index + 1, describe(ticket),
                               items_word(ticket["items"].as_array().map_or(0, Vec::len) as i64), clock(ticket.get("submittedAt").and_then(Value::as_f64))));
        }
    } else {
        lines.push("  Очередь: пусто".into());
    }
    let finished: Vec<&Value> = state["recent"].as_array().into_iter().flatten()
        .filter(|ticket| matches!(text(ticket, "state"), "done" | "released")).collect();
    let finished = &finished[finished.len().saturating_sub(3)..];
    if !finished.is_empty() {
        lines.push("  Недавние:".into());
        for ticket in finished.iter().rev() {
            let items = ticket.get("result").and_then(|result| result.get("items")).and_then(Value::as_array).cloned().unwrap_or_default();
            lines.push(format!("    {} · {} · {}", describe(ticket), clock(ticket.get("finishedAt").and_then(Value::as_f64)), summarize(&items)));
        }
    }
    if (active.is_some() || !waiting.is_empty()) && !queue.dispatcher_alive() {
        lines.push(format!("  Очередь никто не двигает: её процесс не запустился. Лог: {}", queue.folder.join("dispatcher.log").display()));
    }
    lines.extend(["".to_string(),
        "  Закончить проверку с Мака: hitl iphone --queue-release".to_string(),
        "  Поставить чат следующим:   hitl iphone --queue-first r/NN".to_string(),
        "  Убрать из очереди:         hitl drop r/NN".to_string(),
        "  Пауза для всех, кроме чата: hitl iphone --queue-pause --only r/NN   (снять: --queue-resume)".to_string()]);
    lines.join("\n")
}

/// What a run about to use the phone should know before it builds.
fn command_peek(given: &Given) -> i32 {
    let queue = Queue::new(given.text("udid"));
    if !queue.state_path().exists() {
        return 0;
    }
    for line in queue.locked(|state| occupancy_lines(state, None)) {
        say(&format!("==> {line}"));
    }
    0
}

fn command_status(given: &Given) -> i32 {
    let queues = match given.one("udid") {
        Some(udid) => match resolve_queue(Some(udid)) {
            Ok(queue) => vec![queue],
            Err(message) => return stop(&message),
        },
        None => known_queues(),
    };
    let json_output = given.flag("json");
    let mac = if json_output { Vec::new() } else { lines::other_lines() };
    if queues.is_empty() {
        out!("Очереди нет: на телефон ещё не ставили проверок.");
        if !mac.is_empty() {
            out!("\n{}", mac.join("\n"));
        }
        return 0;
    }
    for (index, queue) in queues.iter().enumerate() {
        let shown = queue.locked(|state| {
            if hitl_host_core::truthy(state.get("active")) || !state["queue"].as_array().is_none_or(Vec::is_empty) {
                queue.ensure_dispatcher();
            }
            if json_output { hitl_host_core::dumps(state) } else { format!("{}{}", if index > 0 { "\n" } else { "" }, status_text(queue, state)) }
        });
        out!("{shown}");
    }
    if !mac.is_empty() {
        out!("\n{}", mac.join("\n"));
    }
    0
}

fn command_release(given: &Given) -> i32 {
    let queue = match resolve_queue(given.one("udid")) {
        Ok(queue) => queue,
        Err(message) => return stop(&message),
    };
    let active = queue.locked(|state| state.get("active").filter(|active| active.is_object()).cloned());
    let Some(active) = active else {
        out!("На телефоне нет проверки: снимать нечего.");
        return 0;
    };
    if refuses_foreign(&active, given.flag("force"), "закончить") {
        return 1;
    }
    // The marks made so far are worth keeping; the phone may be out of reach.
    let report = if hitl_host_core::truthy(active.get("overlay")) {
        dispatcher::real_device(&queue.udid).read_report(text(&active, "bundleId"))
    } else {
        None
    };
    let note = given.one("note");
    let outcome = queue.locked(|state| {
        let Some(active) = state.get("active").filter(|active| active.is_object()).cloned() else { return None };
        finish(&queue, state, active.clone(), "command", report.as_ref(), note, "released");
        bridge::tell_printed(text(&active, "id")); // ended by the chat that asked: it knows, and is not told its own words
        let following = state["queue"].as_array().and_then(|waiting| waiting.first()).map(|ticket| text(ticket, "tag").to_string());
        Some((active, following))
    });
    let Some((active, following)) = outcome else {
        out!("Проверка уже закончена.");
        return 0;
    };
    out!("Проверка {} снята. {}", describe(&active), match following {
        Some(tag) => format!("Следующей встанет {tag}."),
        None => "Очередь пуста.".to_string(),
    });
    queue.ensure_dispatcher();
    0
}

const ONLY_ARTEM_PAUSES: &str = "Паузу ставит и снимает Проверяющий, командой в терминале. Если он прямо сказал это сделать тебе, повтори с --force.";

/// Holds the queue: nothing new goes on the phone, or only the checks of the chats named with --only. A check of
/// another chat that is on the phone then steps back to the head of the queue and installs again on resume.
fn command_pause(given: &Given) -> i32 {
    let queue = match resolve_queue(given.one("udid")) {
        Ok(queue) => queue,
        Err(message) => return stop(&message),
    };
    if !caller_chat().is_empty() && !given.flag("force") {
        out!("{ONLY_ARTEM_PAUSES}");
        return 1;
    }
    let only: Vec<String> = given.all("only").iter().map(|tag| tag.trim().to_string()).filter(|tag| !tag.is_empty()).collect();
    let (aside, marks) = queue.locked(|state| {
        state["paused"] = json!({"since": now(), "only": only});
        let mut aside = None;
        let mut marks = 0;
        let active_tag = state.get("active").filter(|active| active.is_object()).map(|active| text(active, "tag").to_string());
        if !only.is_empty() && active_tag.is_some_and(|tag| !only.contains(&tag)) {
            aside = set_aside(state, "пауза очереди");
            marks = aside.as_ref().map_or(0, |aside| aside.get("lostMarks").and_then(Value::as_i64).unwrap_or(0));
        }
        queue.save(state);
        queue.ensure_dispatcher();
        (aside, marks)
    });
    out!("Пауза: {}", if only.is_empty() { "на телефон ничего не ставится.".to_string() } else { format!("на телефон идёт только {}.", only.join(", ")) });
    if let Some(aside) = aside {
        out!("{} снята с телефона и стоит в очереди первой; после паузы встанет заново.{}", describe(&aside),
                 if marks > 0 { format!(" Её {marks} {} на телефоне пропадут.", plural(marks, "отметка", "отметки", "отметок")) } else { String::new() });
    }
    out!("Снять паузу: hitl iphone --queue-resume");
    0
}

fn command_resume(given: &Given) -> i32 {
    let queue = match resolve_queue(given.one("udid")) {
        Ok(queue) => queue,
        Err(message) => return stop(&message),
    };
    if !caller_chat().is_empty() && !given.flag("force") {
        out!("{ONLY_ARTEM_PAUSES}");
        return 1;
    }
    let (was, following) = queue.locked(|state| {
        let was = hitl_host_core::truthy(state.get("paused"));
        state["paused"] = Value::Null;
        let following = if state.get("active").is_none_or(Value::is_null) {
            state["queue"].as_array().and_then(|waiting| waiting.first()).map(|ticket| text(ticket, "tag").to_string())
        } else {
            None
        };
        queue.save(state);
        queue.ensure_dispatcher();
        (was, following)
    });
    out!("{}", if !was { "Паузы не было.".to_string() } else {
        format!("Пауза снята.{}", following.map_or(String::new(), |tag| format!(" Следующей встанет {tag}.")))
    });
    0
}

fn matches_key(ticket: &Value, key: &str) -> bool {
    key == text(ticket, "id") || key == text(ticket, "tag") || text(ticket, "tag").to_lowercase() == key.to_lowercase()
}

fn command_first(given: &Given) -> i32 {
    let queue = match resolve_queue(given.one("udid")) {
        Ok(queue) => queue,
        Err(message) => return stop(&message),
    };
    let wanted = given.words[0].clone();
    let outcome = queue.locked(|state| {
        let index = state["queue"].as_array().into_iter().flatten().position(|ticket| matches_key(ticket, &wanted));
        let Some(index) = index else {
            out!("В очереди нет «{wanted}». Очередь: hitl queue");
            return None;
        };
        if !caller_chat().is_empty() && !given.flag("force") {
            out!("Порядок очереди меняет Проверяющий, командой в терминале. Если он прямо сказал это сделать тебе, повтори с --force.");
            return None;
        }
        let ticket = list_mut(state, "queue").remove(index);
        list_mut(state, "queue").insert(0, ticket.clone());
        Some(ticket)
    });
    let Some(ticket) = outcome else { return 1 };
    out!("{} встанет на телефон следующей.", describe(&ticket));
    queue.ensure_dispatcher();
    0
}

fn command_drop(given: &Given) -> i32 {
    let queue = match resolve_queue(given.one("udid")) {
        Ok(queue) => queue,
        Err(message) => return stop(&message),
    };
    enum Done {
        Code(i32),
        Removed(Value),
        Released(Value),
        Nothing(String),
    }
    let force = given.flag("force");
    let done = queue.locked(|state| {
        let mut key = given.words.first().cloned();
        if key.is_none() {
            let mine = own_ticket(state);
            key = mine.filter(|mine| matches!(text(mine, "state"), "queued" | "active")).map(|mine| text(&mine, "id").to_string());
        }
        let Some(key) = key else {
            out!("Назови чат или номер проверки: hitl drop r/NN");
            return Done::Code(1);
        };
        let index = state["queue"].as_array().into_iter().flatten().position(|ticket| matches_key(ticket, &key));
        let active = state.get("active").filter(|active| active.is_object()).cloned();
        let active_matches = active.as_ref().is_some_and(|active| matches_key(active, &key));
        let target = match index {
            Some(index) => Some(state["queue"][index].clone()),
            None => active.clone(),
        };
        if let Some(target) = &target {
            if (index.is_some() || active_matches) && refuses_foreign(target, force, "убрать") {
                return Done::Code(1);
            }
        }
        if let Some(index) = index {
            let mut ticket = list_mut(state, "queue").remove(index);
            ticket["state"] = json!("cancelled");
            ticket["finishedAt"] = json!(now());
            ticket["error"] = json!("снята командой --queue-drop");
            let _ = std::fs::remove_dir_all(queue.ticket_folder(text(&ticket, "id")).join("app"));
            archive(state, ticket.clone());
            return Done::Removed(ticket);
        }
        if let Some(active) = active.filter(|_| active_matches) {
            let finished = finish(&queue, state, active, "dropped", None, given.one("note"), "cancelled");
            // Set after the verdict was written, on the ticket the state keeps.
            if let Some(last) = list_mut(state, "recent").last_mut() {
                last["error"] = json!("снята командой --queue-drop");
            }
            return Done::Released(finished);
        }
        Done::Nothing(key)
    });
    match done {
        Done::Code(code) => code,
        Done::Removed(ticket) => {
            out!("{} убрана из очереди.", describe(&ticket));
            queue.ensure_dispatcher(); // retract any queue-owned device action, even after a worker died
            0
        }
        Done::Released(active) => {
            out!("{} снята с телефона: проверка отменена.", describe(&active));
            queue.ensure_dispatcher();
            0
        }
        Done::Nothing(key) => {
            out!("«{key}» нет ни на телефоне, ни в очереди.");
            1
        }
    }
}

fn command_last_check(given: &Given) -> i32 {
    let Some(entry) = last_check(given.text("branch"), None) else { return 1 };
    let take = |key: &str| entry.get(key).cloned().unwrap_or(Value::Null);
    let commit = if hitl_host_core::truthy(entry.get("commit")) { entry["commit"].clone() } else { json!("") };
    let shown = json!({"id": take("id"), "tag": take("tag"), "title": take("title"), "branch": take("branch"), "commit": commit,
                       "finishedAt": take("finishedAt"), "when": clock(entry.get("finishedAt").and_then(Value::as_f64)),
                       "how": take("how"), "summary": take("summary"), "green": is_green(&entry)});
    out!("{}", hitl_host_core::dumps_line(&shown));
    0
}

fn command_chat_tag() -> i32 {
    out!("{}", hitl_host_core::chat_prefix(&hitl_host_core::chat_title(&env()), 16));
    0
}

// --------------------------------------------------------------------------
// Entry

pub fn run(arguments: &[String]) -> i32 {
    let Some((command, rest)) = arguments.split_first() else {
        say("usage: hitl _queues [-h] {chat-tag,check,wait,...} ...");
        say("hitl _queues: error: the following arguments are required: command");
        return EXIT_USAGE;
    };
    const NO_WORDS: (usize, Option<usize>) = (0, Some(0));
    let command = command.as_str();
    let done = |given: Result<Given, i32>, action: &dyn Fn(&Given) -> i32| match given {
        Ok(given) => action(&given),
        Err(code) => code,
    };
    match command {
        "chat-tag" => done(parse(command, rest, &[], NO_WORDS), &|_| command_chat_tag()),
        "check" => done(parse(command, rest, &[required("udid"), option("name"), option("core-id"), required("app"), required("bundle-id"),
            option("tag"), option("launch-tag"), option("branch"), option("commit"), option("project"), option("title"), repeated("item"),
            option("checklist"), option("url"), flag("no-wait"), flag("force")], (0, None)), &command_check),
        "wait" => done(parse(command, rest, &[option("udid")], (0, Some(1))), &command_wait),
        "probe-begin" => done(parse(command, rest, &[required("udid"), option("name"), option("core-id"), option("tag"), required("pid"),
            option("what"), flag("no-wait"), flag("force")], NO_WORDS), &command_probe_begin),
        "probe-end" => done(parse(command, rest, &[required("udid"), required("pid")], NO_WORDS), &command_probe_end),
        "peek" => done(parse(command, rest, &[required("udid")], NO_WORDS), &command_peek),
        "status" => done(parse(command, rest, &[option("udid"), flag("json")], NO_WORDS), &command_status),
        "release" => done(parse(command, rest, &[option("udid"), option("note"), flag("force")], NO_WORDS), &command_release),
        "pause" => done(parse(command, rest, &[option("udid"), repeated("only"), flag("force")], NO_WORDS), &command_pause),
        "resume" => done(parse(command, rest, &[option("udid"), flag("force")], NO_WORDS), &command_resume),
        "first" => done(parse(command, rest, &[option("udid"), flag("force")], (1, Some(1))), &command_first),
        "drop" => done(parse(command, rest, &[option("udid"), option("note"), flag("force")], (0, Some(1))), &command_drop),
        "mac-begin" => done(parse(command, rest, &[required("pid"), option("tag"), option("what"), flag("no-wait")], NO_WORDS), &|given| {
            lines::command_mac_begin(given.number("pid"), given.one("tag"), given.one("what"), given.flag("no-wait"))
        }),
        "mac-end" => done(parse(command, rest, &[required("pid")], NO_WORDS), &|given| lines::command_mac_end(given.number("pid"))),
        "last-check" => done(parse(command, rest, &[required("branch")], NO_WORDS), &command_last_check),
        "dispatch" => done(parse(command, rest, &[required("udid")], NO_WORDS), &|given| dispatcher::command_dispatch(given.text("udid"))),
        _ => {
            say(&format!("usage: hitl _queues [-h] {{chat-tag,check,wait,...}} ..."));
            say(&format!("hitl _queues: error: argument command: invalid choice: '{command}'"));
            EXIT_USAGE
        }
    }
}

#[allow(dead_code)]
fn unused() -> (f64, &'static str) {
    (number(&json!({}), "x"), CHECKLIST_ARGUMENT)
}
