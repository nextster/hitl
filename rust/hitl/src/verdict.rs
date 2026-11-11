//! What a check ended with, said to the chat. The port of the verdict part of `src/hitl/check.py`, with
//! Includes the check number for answer delivery.
//!
//! A verdict is the JSON the window writes into `done/<id>.json` and the history keeps: read as Python read it,
//! a key that is missing and a key that is null alike, and every value said the way Python's `str()` says it.
#![allow(dead_code)]

use crate::common::{self, truthy};
use crate::submit;
use serde_json::{Map, Value, json};

/// The answer «не понял»: the item is to be said again (check.UNCLEAR).
pub const UNCLEAR: &str = "Не понял";
/// The words the last line of a verdict begins with (note.NUMBER).
pub const NUMBER: &str = "Номер проверки";

// --------------------------------------------------------------------------
// Values read as Python read them

/// Python's `str()` of a value read from JSON: a text as it is, `None`, `True`, a number as Python writes it, a
/// list or a dictionary as its `repr`.
pub fn py_str(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => py_repr(other),
    }
}

fn py_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".into(),
        Value::Bool(flag) => if *flag { "True".into() } else { "False".into() },
        Value::Number(number) => {
            if number.is_i64() || number.is_u64() {
                number.to_string()
            } else {
                py_float(number.as_f64().unwrap_or(0.0))
            }
        }
        Value::String(text) => py_quote(text),
        Value::Array(items) => format!("[{}]", items.iter().map(py_repr).collect::<Vec<_>>().join(", ")),
        Value::Object(map) => {
            let pairs: Vec<String> = map.iter().map(|(key, value)| format!("{}: {}", py_quote(key), py_repr(value))).collect();
            format!("{{{}}}", pairs.join(", "))
        }
    }
}

/// A float as Python's `repr` writes it: the shortest digits that read back, `1e+16` and `1e-05` past the bounds.
fn py_float(number: f64) -> String {
    if number.is_nan() {
        return "nan".into();
    }
    if number.is_infinite() {
        return if number > 0.0 { "inf".into() } else { "-inf".into() };
    }
    let written = format!("{number:?}");
    match written.split_once('e') {
        Some((mantissa, exponent)) => {
            let power: i32 = exponent.parse().unwrap_or(0);
            format!("{mantissa}e{}{:02}", if power < 0 { '-' } else { '+' }, power.abs())
        }
        None => written,
    }
}

/// A text in quotes as Python's `repr` writes it.
fn py_quote(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') { '"' } else { '\'' };
    let mut out = String::new();
    out.push(quote);
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// `value or ""` said as a text.
fn text_or_empty(value: Option<&Value>) -> String {
    if truthy(value) { py_str(value.unwrap_or(&Value::Null)) } else { String::new() }
}

/// `entry.get(key, default)` said as a text: the default only for a key that is not there.
fn text_or(value: &Value, key: &str, default: &str) -> String {
    match value.get(key) {
        None => default.to_string(),
        Some(found) => py_str(found),
    }
}

fn label(entry: &Value) -> String {
    text_or(entry, "label", "?")
}

fn results(verdict: &Value) -> &[Value] {
    common::list_of(verdict, "results")
}

/// A kind as a lookup reads it: `kind or ""`, a value that is not a text being no kind of the table.
fn kind_value(value: Option<&Value>) -> Option<String> {
    if truthy(value) { Some(py_str(value.unwrap_or(&Value::Null))) } else { None }
}

/// `entry.get("kind") or verdict.get("kind")`: the item's own kind, else the check's.
fn kind_of(entry: &Value, verdict: &Value) -> Option<String> {
    if truthy(entry.get("kind")) { kind_value(entry.get("kind")) } else { kind_value(verdict.get("kind")) }
}

fn is(value: Option<&Value>, text: &str) -> bool {
    value.and_then(Value::as_str) == Some(text)
}

/// A key's value where null is the same as missing, for Python's `==` between two `.get()`s.
fn present(value: Option<&Value>) -> Option<&Value> {
    value.filter(|found| !found.is_null())
}

// --------------------------------------------------------------------------
// The words of the kinds

/// What a kind is called: «план», «вопрос» (check.KINDS); «чек-лист» for one that is none.
pub fn kind_title(kind: Option<&str>) -> &'static str {
    let wanted = kind.unwrap_or("");
    submit::KINDS.iter().find(|(name, _)| *name == wanted).map(|(_, title)| *title).unwrap_or("чек-лист")
}

fn answer_names(kind: Option<&str>) -> &'static [&'static str] {
    let names = submit::answers(kind.unwrap_or(""));
    if names.is_empty() { submit::answers("checklist") } else { names }
}

/// The groups of a verdict, by state: yes, no, later, and the items with no mark (check.groups).
pub fn groups(kind: Option<&str>) -> [(Option<&'static str>, &'static str); 4] {
    let [yes, no, later] = submit::groups_of(kind.unwrap_or(""));
    [(Some("ok"), yes), (Some("problem"), no), (Some("skipped"), later), (None, "Без отметки")]
}

/// What a check ended with every answer yes means, said of whoever answers (`common.person`).
pub fn green(kind: &str) -> String {
    let ended = format!("{} проверку {} {}", common::did("закончил"), common::did("сам"), common::who());
    let his = common::his();
    match kind {
        "checklist" => format!("Все пункты отмечены как рабочие, и {ended}: это {his} «ок» на то, что проверялось."),
        "result" => format!("Все пункты приняты, и {ended}: это {his} «ок» на этот результат."),
        "plan" => format!("Все шаги приняты, и {ended}: это {his} «ок» делать по этому плану."),
        "question" => format!("На каждый вопрос есть ответ, и {ended}."),
        "action" => format!("{} {}, что всё сделал{}.", common::who(), common::did("отметил"), common::he("", "а")),
        "explain" => format!("Всё понятно, и {ended}."),
        "mixed" => format!("На каждый пункт ответ положительный, и {ended}: это {his} «ок» на то, о чём спрашивал каждый пункт."),
        _ => green("checklist"),
    }
}

pub fn unclear_advice() -> String {
    format!(
        "«Не понял» — не «нет»: эти пункты {} не {}. Скажи их заново, иначе и короче, новой проверкой: чем это для {}, без \
         слов, которых {} не {}, с примером или картинкой. Остальные ответы в силе.",
        common::he("он", "она"),
        common::did("понял"),
        common::he("него", "неё"),
        common::he("он", "она"),
        common::did("говорил")
    )
}

// --------------------------------------------------------------------------
// The answers of the items

/// How many items are answered yes, no, later, and not at all.
#[derive(Debug, Default, PartialEq)]
pub struct Marks {
    pub ok: i64,
    pub problem: i64,
    pub skipped: i64,
    pub none: i64,
}

pub fn count_marks(results: &[Value]) -> Marks {
    let mut marks = Marks::default();
    for entry in results {
        match state_of(entry) {
            Some("ok") => marks.ok += 1,
            Some("problem") => marks.problem += 1,
            Some("skipped") => marks.skipped += 1,
            _ => marks.none += 1,
        }
    }
    marks
}

/// Ended with «Готово» and every item answered yes.
pub fn is_green(verdict: &Value) -> bool {
    let marks = count_marks(results(verdict));
    is(verdict.get("how"), "done") && marks.ok > 0 && marks.problem == 0 && marks.skipped == 0 && marks.none == 0
}

pub fn state_of(entry: &Value) -> Option<&'static str> {
    match entry.get("state").and_then(Value::as_str) {
        Some("ok") => Some("ok"),
        Some("problem") => Some("problem"),
        Some("skipped") => Some("skipped"),
        _ => None,
    }
}

/// He answered «не понял»: the item is to be said again. The window keeps it as a no with a word beside it.
pub fn is_unclear(entry: &Value) -> bool {
    truthy(entry.get("unclear")) && state_of(entry) == Some("problem")
}

/// A question answered in his own words, with none of its options.
pub fn is_own_answer(entry: &Value) -> bool {
    truthy(entry.get("question")) && state_of(entry) == Some("problem") && !is_unclear(entry)
}

/// What he chose among the item's options: the one, or every one he ticked, with a word that his own answer is
/// ticked beside them; "" when the item was not answered by a choice.
pub fn chosen(entry: &Value) -> String {
    if truthy(entry.get("choice")) {
        return py_str(&entry["choice"]);
    }
    let mut ticked: Vec<String> = match entry.get("choices") {
        Some(Value::Array(items)) => items.iter().map(py_str).collect(),
        Some(Value::String(text)) => text.chars().map(String::from).collect(),
        Some(Value::Object(map)) => map.keys().cloned().collect(),
        _ => Vec::new(),
    };
    if truthy(entry.get("other")) {
        ticked.push(if ticked.is_empty() { "своё, в комментарии" } else { "и своё, в комментарии" }.to_string());
    }
    ticked.join("; ")
}

/// A check whose items are of more than one kind: an explanation, a question about it, a step of his.
pub fn is_mixed(verdict: &Value) -> bool {
    let own = present(verdict.get("kind"));
    results(verdict).iter().any(|entry| truthy(entry.get("kind")) && entry.get("kind") != own)
}

/// What he answered to an item, in the words of the item's kind.
pub fn answer_of(entry: &Value, kind: Option<&str>) -> String {
    if is_unclear(entry) {
        return UNCLEAR.to_lowercase();
    }
    let choice = chosen(entry);
    if !choice.is_empty() {
        return format!("выбрал «{choice}»");
    }
    if is_own_answer(entry) {
        return "свой ответ".into();
    }
    let Some(state) = state_of(entry) else { return "без отметки".into() };
    let names = answer_names(kind);
    let place = ["ok", "problem", "skipped"].iter().position(|known| *known == state).unwrap_or(0);
    if place < names.len() { names[place].to_string() } else { groups(kind)[place].1.to_lowercase() }
}

/// The answers of a check of several kinds, an item a line, in their order. A comment stands with its item.
pub fn by_items(verdict: &Value, compact: bool) -> Vec<String> {
    let mut lines = Vec::new();
    for entry in results(verdict) {
        let kind = kind_of(entry, verdict);
        let raw = text_or_empty(entry.get("note"));
        let note = if compact { raw.trim().to_string() } else { common::squeeze(&raw) };
        let prefix = if compact {
            format!("{} ", label(entry))
        } else {
            format!("{}. {}: ", label(entry), kind_title(kind.as_deref()))
        };
        let said = if note.is_empty() { String::new() } else { format!(". «{note}»") };
        lines.push(format!("{prefix}{}{said}", answer_of(entry, kind.as_deref())));
    }
    lines
}

/// The items he answered by one of their options or in his own words: such an answer is kept as a yes whatever he
/// chose, and says what he chose, not that the thing is accepted.
pub fn answered_by_choice(verdict: &Value) -> Vec<String> {
    results(verdict).iter().filter(|entry| truthy(entry.get("question")) || !chosen(entry).is_empty()).map(label).collect()
}

pub fn summary(verdict: &Value) -> String {
    let results = results(verdict);
    if is_mixed(verdict) {
        let mut counts: Vec<(String, i64)> = Vec::new();
        for entry in results {
            let answer = if chosen(entry).is_empty() { answer_of(entry, kind_of(entry, verdict).as_deref()) } else { "выбор".into() };
            if answer == "без отметки" {
                continue;
            }
            match counts.iter_mut().find(|(known, _)| *known == answer) {
                Some((_, count)) => *count += 1,
                None => counts.push((answer, 1)),
            }
        }
        let parts: Vec<String> = counts.iter().map(|(answer, count)| format!("{answer} {count}")).collect();
        return if parts.is_empty() { "без отметок".into() } else { parts.join(", ") };
    }
    let plain: Vec<&Value> = results.iter().filter(|entry| is_plain(entry)).collect();
    let mut parts = Vec::new();
    for (state, name) in groups(kind_value(verdict.get("kind")).as_deref()) {
        let count = plain.iter().filter(|entry| state_of(entry) == state).count();
        if count > 0 {
            parts.push(format!("{} {count}", name.to_lowercase()));
        }
    }
    let unclear = results.iter().filter(|entry| is_unclear(entry)).count();
    let choices = results.iter().filter(|entry| !chosen(entry).is_empty()).count();
    let own = results.iter().filter(|entry| is_own_answer(entry)).count();
    for (word, count) in [(UNCLEAR.to_lowercase(), unclear), ("выбор".into(), choices), ("свой ответ".into(), own)] {
        if count > 0 {
            parts.push(format!("{word} {count}"));
        }
    }
    if parts.is_empty() { "без отметок".into() } else { parts.join(", ") }
}

/// Answered by none of the ways that are said apart: no choice, no answer of his own, no «не понял».
fn is_plain(entry: &Value) -> bool {
    chosen(entry).is_empty() && !is_own_answer(entry) && !is_unclear(entry)
}

// --------------------------------------------------------------------------
// The verdict as the chat reads it

/// The verdict as the chat reads it; `compact` for a message into a chat (check.verdict_text).
pub fn verdict_text(verdict: &Value, compact: bool) -> String {
    let results = results(verdict);
    let tag = if truthy(verdict.get("tag")) { py_str(&verdict["tag"]) } else { "чат".into() };
    let how = verdict.get("how");
    let done = is(how, "done");
    let dropped = is(how, "dropped");
    let finished = if truthy(verdict.get("finishedAt")) { verdict["finishedAt"].as_f64() } else { None };
    let ended = common::clock(Some(finished.unwrap_or_else(common::now)));
    let title = text_or(verdict, "title", "");
    let head = if done {
        format!("Проверка {tag} «{title}» закончена в {ended}: отмечены все пункты.")
    } else if dropped {
        format!("Проверка {tag} «{title}» снята в {ended}: её не смотрели.")
    } else {
        let marked = results.iter().filter(|entry| truthy(entry.get("state"))).count();
        format!("Проверка {tag} «{title}» возвращена на доработку в {ended}: отмечено {marked} из {}.", results.len())
    };
    let mut lines: Vec<String> = vec![if compact { format!("«{title}»") } else { head }, String::new()];
    if compact && dropped {
        lines.push(format!("Снята: {} не {} проверку.", common::who(), common::did("смотрел")));
    }
    if !dropped {
        let unclear: Vec<String> = results.iter().filter(|entry| is_unclear(entry)).map(label).collect();
        if compact || is_mixed(verdict) {
            lines.extend(by_items(verdict, compact));
        } else {
            let plain: Vec<&Value> = results.iter().filter(|entry| is_plain(entry)).collect();
            for (state, name) in groups(kind_value(verdict.get("kind")).as_deref()) {
                let labels: Vec<String> = plain.iter().filter(|entry| state_of(entry) == state).map(|entry| label(entry)).collect();
                if !labels.is_empty() {
                    lines.push(format!("{name}: {}", labels.join(", ")));
                }
            }
            if !unclear.is_empty() {
                lines.push(format!("{UNCLEAR}: {}", unclear.join(", ")));
            }
            let own: Vec<String> = results.iter().filter(|entry| is_own_answer(entry)).map(label).collect();
            if !own.is_empty() {
                lines.push(format!("Свой ответ, не из вариантов: {}", own.join(", ")));
            }
            let choices: Vec<(String, String)> =
                results.iter().map(|entry| (label(entry), chosen(entry))).filter(|(_, choice)| !choice.is_empty()).collect();
            if !choices.is_empty() {
                lines.extend([String::new(), "Выбор:".into()]);
                lines.extend(choices.iter().map(|(label, choice)| format!("{label}. {choice}")));
            }
            let notes: Vec<(String, String)> = results.iter()
                .map(|entry| (label(entry), text_or_empty(entry.get("note")).trim().to_string()))
                .filter(|(_, note)| !note.is_empty())
                .collect();
            if !notes.is_empty() {
                lines.extend([String::new(), "Комментарии:".into()]);
                lines.extend(notes.iter().map(|(label, note)| format!("{label}. {note}")));
            }
        }
        let pages: Vec<(String, &Value)> =
            results.iter().filter(|entry| truthy(entry.get("values"))).map(|entry| (label(entry), &entry["values"])).collect();
        if !pages.is_empty() {
            lines.extend([String::new(), "Со страницы, что было выставлено при ответе:".into()]);
            for (label, values) in pages {
                let pairs: Vec<String> = py_items(values).iter().filter_map(pair_of).map(|(name, value)| format!("{name}: {value}")).collect();
                lines.push(format!("{label}. {}", pairs.join(" · ")));
            }
        }
        let shots: Vec<(String, String)> = results.iter()
            .flat_map(|entry| {
                let label = label(entry);
                let kept = entry.get("shots").filter(|shots| truthy(Some(shots))).map(py_items).unwrap_or_default();
                kept.into_iter().map(move |shot| (label.clone(), py_str(&shot)))
            })
            .collect();
        if !shots.is_empty() {
            lines.extend([String::new(), "Снимки экрана к комментариям, открой файлы:".into()]);
            lines.extend(shots.iter().map(|(label, shot)| format!("{label}. {shot}")));
        }
        // He may have ended it in the app on the phone, which shows the window's queue: the same answer, said where.
        let place = if is(verdict.get("endedOn"), "phone") { "в приложении «Конвейер» на телефоне" } else { "в окне на Маке" };
        if compact {
            lines.push(if done {
                format!("Завершено {}: {place} · Готово.", common::by_whom())
            } else {
                format!("{} {} на доработку: {place}.", common::who(), common::did("вернул"))
            });
        } else {
            lines.push(String::new());
            lines.push(if done {
                format!("Как закончена: кнопкой «Готово» {place}.")
            } else {
                format!("Как закончена: кнопкой «Доработать» {place}, раньше чем отмечены все пункты.")
            });
        }
        if !unclear.is_empty() {
            lines.push(if compact { "«Не понял» — не «нет»: переформулируй эти пункты; остальные ответы в силе.".into() } else { unclear_advice() });
        }
        // Where an item is likely to come to him again, the chat is told how to bring his answer along.
        let again = !unclear.is_empty() || !done
            || results.iter().any(|entry| state_of(entry) == Some("problem") && !truthy(entry.get("question")));
        if again && said_before(verdict).as_object().is_some_and(|said| !said.is_empty()) {
            let id = text_or(verdict, "id", "");
            lines.push(if compact {
                format!("Повторная проверка: --after {id}, те же номера пунктов.")
            } else {
                format!("Спросишь эти пункты заново — добавь к новой проверке --after {id} и оставь пунктам их номера: окно покажет {} над \
                         каждым, что {} {} сейчас.", common::he("ему", "ей"), common::he("он", "она"), common::did("ответил"))
            });
        }
        if is_green(verdict) {
            if compact {
                if !is_mixed(verdict) && answered_by_choice(verdict).is_empty() {
                    let permission = match verdict.get("kind").and_then(Value::as_str) {
                        Some("plan") => Some("План принят: выполнить его шаги."),
                        Some("result") => Some("Результат принят."),
                        Some("checklist") => Some("Проверка принята в её объёме."),
                        _ => None,
                    };
                    if let Some(permission) = permission {
                        lines.push(permission.into());
                    }
                }
            } else {
                let kind = if is_mixed(verdict) { "mixed".into() } else { text_or_empty(verdict.get("kind")) };
                lines.push(green(&kind));
            }
        }
    }
    if truthy(verdict.get("id")) {
        // The last line names the check: by it the command run after a call of Claude Code (`hitl note`) knows
        // whose verdict the call printed, and tells Claude's protection what he answered.
        if lines.last().is_some_and(|line| !line.is_empty()) {
            lines.push(String::new());
        }
        lines.push(line_of(&py_str(&verdict["id"])));
    }
    lines.into_iter().filter(|line| !line.is_empty() || !compact).collect::<Vec<_>>().join("\n")
}

/// What Python's `for x in value` goes through: a list's items, a text's characters, a dictionary's keys.
fn py_items(value: &Value) -> Vec<Value> {
    match value {
        Value::Array(items) => items.clone(),
        Value::String(text) => text.chars().map(|character| Value::String(character.to_string())).collect(),
        Value::Object(map) => map.keys().map(|key| Value::String(key.clone())).collect(),
        _ => Vec::new(),
    }
}

/// A name and a value set on a page, where the pair has the two.
fn pair_of(pair: &Value) -> Option<(String, String)> {
    match pair {
        Value::Array(items) if items.len() == 2 => Some((py_str(&items[0]), py_str(&items[1]))),
        Value::String(text) if text.chars().count() == 2 => {
            let mut characters = text.chars();
            Some((characters.next()?.to_string(), characters.next()?.to_string()))
        }
        _ => None,
    }
}

/// The last line of a verdict, which names the check (note.line_of).
pub fn line_of(ticket_id: &str) -> String {
    format!("{NUMBER}: {ticket_id}.")
}

// --------------------------------------------------------------------------
// An ended check, asked again

/// An ended check by its number: the history keeps it for good (check.ended).
pub fn ended(ticket_id: &str) -> Option<Value> {
    let mut found = None;
    for verdict in common::history() {
        if is(verdict.get("id"), ticket_id) && !is(verdict.get("how"), "dropped") && truthy(verdict.get("results")) {
            found = Some(verdict);
        }
    }
    found
}

/// What he answered to the items of an ended check, by their numbers: only where he wrote something or did not say a
/// plain yes. That is what the window shows over an item that is asked again (check.said_before).
pub fn said_before(verdict: &Value) -> Value {
    let mut said = Map::new();
    let on_mac = common::screen_of(verdict) == "mac";
    for entry in results(verdict) {
        let note = common::squeeze(&text_or_empty(entry.get("note")));
        let state = state_of(entry);
        let plain_yes = state == Some("ok") && chosen(entry).is_empty();
        if (state.is_none() && note.is_empty()) || (plain_yes && note.is_empty()) {
            continue;
        }
        let kind = if on_mac { kind_of(entry, verdict) } else { None };
        let at = if truthy(verdict.get("finishedAt")) { verdict["finishedAt"].clone() } else { json!(common::now()) };
        let answer = if state.is_none() { String::new() } else { answer_of(entry, kind.as_deref()) };
        said.insert(label(entry), json!({"at": at, "answer": answer, "note": note}));
    }
    Value::Object(said)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    /// The tests that set the process's environment take turns: it is one for every thread.
    pub(crate) static ENVIRONMENT: Mutex<()> = Mutex::new(());

    pub(crate) struct Place {
        pub home: std::path::PathBuf,
        _turn: MutexGuard<'static, ()>,
    }

    impl Drop for Place {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.home);
        }
    }

    /// A made-up home, no window and a chat of its own, as hitl's core (`HITL_TOOL=hitl-host`) or as hitl.
    pub(crate) fn place(hitl: bool) -> Place {
        let turn = ENVIRONMENT.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = std::env::temp_dir().join(format!("hitl-part-b-{}-{}", std::process::id(), common::now().to_bits()));
        std::fs::create_dir_all(&home).unwrap();
        let set = |name: &str, value: &str| unsafe { std::env::set_var(name, value) };
        for (name, value) in [("CONVEYOR_HOME", home.to_str().unwrap()), ("HITL_HOME", home.to_str().unwrap()),
                              ("CONVEYOR_WINDOW", ""), ("CONVEYOR_NOTIFY", "0"), ("CONVEYOR_CHAT", "r/66 · проба"),
                              ("CONVEYOR_CODEX", ""), ("CONVEYOR_CLAUDE", ""), ("CONVEYOR_OPENCODE", ""),
                              ("CONVEYOR_JOURNAL", "0"), ("HITL_COMMAND", "/made/up/hitl")] {
            set(name, value);
        }
        if hitl { set("HITL_TOOL", "hitl-host") } else { unsafe { std::env::remove_var("HITL_TOOL") } }
        Place { home, _turn: turn }
    }

    fn checklist(results: Value) -> Value {
        json!({"id": "1", "tag": "r/66", "title": "Плагин", "kind": "checklist", "how": "done", "finishedAt": 2.0, "results": results})
    }

    #[test]
    fn only_a_check_ended_with_done_and_all_yes_is_green() {
        assert!(is_green(&json!({"how": "done", "results": [{"state": "ok"}]})));
        for other in [json!({"how": "rework", "results": [{"state": "ok"}]}),
                      json!({"how": "done", "results": [{"state": "ok"}, {"state": "skipped"}]}),
                      json!({"how": "done", "results": [{"state": "problem"}]}),
                      json!({"how": "done", "results": [{"state": null}]}),
                      json!({"how": "done", "results": []})] {
            assert!(!is_green(&other), "{other}");
        }
    }

    #[test]
    fn a_verdict_names_items_by_mark_with_comments_and_choices() {
        let _place = place(true);
        let verdict = checklist(json!([{"label": "1", "state": "ok", "note": ""},
                                       {"label": "2", "state": "problem", "note": "не открывается"},
                                       {"label": "3", "state": "ok", "choice": "В свой репозиторий", "question": true, "note": ""}]));
        let text = verdict_text(&verdict, false);
        for part in ["закончена", "Работает: 1\n", "Не работает: 2", "\nВыбор:\n3. В свой репозиторий", "\nКомментарии:\n2. не открывается",
                     "Как закончена: кнопкой «Готово» в окне на Маке.", "\n\nНомер проверки: 1."] {
            assert!(text.contains(part), "{part}\n{text}");
        }
        assert!(!text.contains("это его «ок»"));
        assert_eq!(summary(&verdict), "работает 1, не работает 1, выбор 1");
    }

    #[test]
    fn one_check_carries_an_explanation_a_question_and_a_step_of_his() {
        let _place = place(true);
        let mut verdict = json!({"id": "1", "tag": "r/84", "title": "Проверки GitHub стоят", "kind": "question", "how": "done",
                                 "finishedAt": common::now(),
                                 "results": [{"label": "1", "kind": "explain", "state": "ok", "note": ""},
                                             {"label": "2", "state": "ok", "choice": "20 $", "question": true, "note": ""},
                                             {"label": "3", "kind": "action", "state": "skipped", "note": "вечером"},
                                             {"label": "4", "state": "problem", "unclear": true, "note": ""}]});
        let text = verdict_text(&verdict, false);
        assert!(text.contains("1. объяснение: понятно\n2. вопрос: выбрал «20 $»\n3. действие: позже. «вечером»\n4. вопрос: не понял"), "{text}");
        for gathered in ["Комментарии:", "Выбор:", "Понятно: 1"] {
            assert!(!text.contains(gathered));
        }
        assert!(text.contains("«Не понял» — не «нет»"));
        assert_eq!(summary(&verdict), "понятно 1, выбор 1, позже 1, не понял 1");
        verdict["results"][2]["state"] = json!("ok");
        verdict["results"][2]["note"] = json!("");
        verdict["results"][3] = json!({"label": "4", "state": "ok", "note": ""});
        assert!(verdict_text(&verdict, false).contains("На каждый пункт ответ положительный"));
        verdict["results"][0].as_object_mut().unwrap().remove("kind");
        verdict["results"][2].as_object_mut().unwrap().remove("kind");
        assert!(verdict_text(&verdict, false).contains("Да: 1, 3, 4"));
    }

    #[test]
    fn several_options_at_once_come_back_as_a_list() {
        let _place = place(true);
        let verdict = json!({"id": "x", "tag": "r/66", "title": "Плагин", "kind": "question", "how": "done", "finishedAt": 2.0,
                             "results": [{"label": "1", "state": "ok", "choices": ["Сервер", "Документация"], "question": true, "note": ""},
                                         {"label": "2", "state": "ok", "other": true, "question": true, "note": "ещё тесты"},
                                         {"label": "3", "state": "ok", "choice": "Б", "question": true, "note": ""}]});
        let text = verdict_text(&verdict, false);
        assert!(text.contains("Выбор:\n1. Сервер; Документация\n2. своё, в комментарии\n3. Б"), "{text}");
        assert!(text.contains("2. ещё тесты"));
        assert!(!text.contains("Да: "));
        assert_eq!(summary(&verdict), "выбор 3");
    }

    #[test]
    fn what_was_set_on_a_page_and_screenshots_come_with_the_verdict() {
        let _place = place(true);
        let verdict = checklist(json!([{"label": "1", "state": "problem", "note": "смотри снимок", "values": [["ширина", "520"], ["поля", 14]],
                                        "shots": ["/tmp/shots/one.png", "/tmp/shots/two.png"]},
                                       {"label": "2", "state": "problem", "question": true, "note": "ни туда ни туда"}]));
        let text = verdict_text(&verdict, false);
        for part in ["Не работает: 1\n", "Свой ответ, не из вариантов: 2", "2. ни туда ни туда",
                     "Со страницы, что было выставлено при ответе:\n1. ширина: 520 · поля: 14",
                     "Снимки экрана к комментариям, открой файлы:\n1. /tmp/shots/one.png\n1. /tmp/shots/two.png"] {
            assert!(text.contains(part), "{part}\n{text}");
        }
        assert_eq!(summary(&verdict), "не работает 1, свой ответ 1");
    }

    #[test]
    fn something_only_he_can_do_and_a_plan_are_answered_in_their_own_words() {
        let _place = place(true);
        let action = json!({"id": "a", "tag": "r/66", "title": "Вход", "kind": "action", "how": "done", "finishedAt": 2.0,
                            "results": [{"label": "1", "state": "ok", "note": ""}, {"label": "2", "state": "skipped", "note": ""}]});
        let text = verdict_text(&action, false);
        assert!(text.contains("Сделано: 1") && text.contains("Позже: 2"), "{text}");
        let one = json!({"id": "a", "tag": "r/66", "title": "Вход", "kind": "action", "how": "done", "finishedAt": 2.0,
                         "results": [{"label": "1", "state": "ok", "note": ""}]});
        assert!(verdict_text(&one, false).contains("Проверяющий отметил, что всё сделал."));
        let plan = json!({"id": "p", "tag": "r/66", "title": "План", "kind": "plan", "how": "rework", "finishedAt": 2.0,
                          "results": [{"label": "1", "state": "problem", "note": "сначала плагин"}]});
        let text = verdict_text(&plan, false);
        assert!(text.contains("возвращена на доработку") && text.contains("Нет: 1"), "{text}");
        assert!(text.contains("добавь к новой проверке --after p и оставь пунктам их номера: окно покажет ему над каждым, что он ответил сейчас."));
    }

    #[test]
    fn compact_delivery_is_number_first_and_keeps_its_marker() {
        let _place = place(true);
        let verdict = json!({"id": "1016-012348-663-Автовход", "title": "Что произошло с просьбой пнуть Adopt", "kind": "explain",
                             "how": "done", "results": [{"label": "1", "state": "ok"}, {"label": "2", "state": "ok"}]});
        assert_eq!(verdict_text(&verdict, true),
                   "«Что произошло с просьбой пнуть Adopt»\n1 понятно\n2 понятно\nЗавершено Проверяющим: в окне на Маке · Готово.\n\
                    Номер проверки: 1016-012348-663-Автовход.");
        let full = verdict_text(&verdict, false);
        assert!(full.contains("Понятно: 1, 2") && full.contains("Всё понятно, и закончил проверку сам Проверяющий"), "{full}");
        let mixed = json!({"id": "1", "title": "Т", "kind": "question", "how": "done", "results": [
            {"label": "1", "kind": "explain", "state": "ok", "note": "Первая строка\nВторая строка"},
            {"label": "2", "state": "ok", "choices": ["A", "Б"], "other": true, "values": [["Размер", 7]], "shots": ["/owned/shot.png"]},
            {"label": "3", "kind": "action", "state": "skipped", "note": "вечером"},
            {"label": "4", "state": "problem", "question": true, "note": "Свой вариант"},
            {"label": "5", "state": "problem", "unclear": true},
            {"label": "6", "state": null}]});
        let message = verdict_text(&mixed, true);
        for part in ["1 понятно. «Первая строка\nВторая строка»", "2 выбрал «A; Б; и своё, в комментарии»", "3 позже. «вечером»",
                     "4 свой ответ. «Свой вариант»", "5 не понял", "6 без отметки", "2. Размер: 7", "2. /owned/shot.png", "«Не понял» — не «нет»"] {
            assert!(message.contains(part), "{part}\n{message}");
        }
        assert!(!message.contains("План принят"));
        let dropped = json!({"id": "d", "title": "Т", "how": "dropped", "results": [{"label": "1", "state": null}]});
        assert_eq!(verdict_text(&dropped, true), "«Т»\nСнята: Проверяющий не смотрел проверку.\nНомер проверки: d.");
    }

    #[test]
    fn a_green_result_reports_approval_without_a_merge_instruction() {
        let _place = place(true);
        let verdict = json!({"id": "1", "kind": "result", "branch": "example/login", "commit": "abc",
                             "results": [{"label": "1", "state": "ok"}], "how": "done"});
        let message = verdict_text(&verdict, true);
        assert!(message.contains("Результат принят."), "{message}");
        assert!(!message.contains("merge") && !message.contains("вливание"), "{message}");
    }

    #[test]
    fn an_item_asked_again_brings_his_last_answer_along() {
        let place = place(true);
        let verdict = json!({"id": "first", "screen": "mac", "kind": "plan", "how": "rework", "finishedAt": 2.0, "results": [
            {"label": "5", "state": "ok", "note": ""}, {"label": "6", "state": "problem", "note": "не понял что  и почему здесь"},
            {"label": "7", "state": null, "note": ""}, {"label": "8", "state": null, "note": "и ещё вот что"}]});
        assert_eq!(said_before(&verdict), json!({"6": {"at": 2.0, "answer": "нет", "note": "не понял что и почему здесь"},
                                                 "8": {"at": 2.0, "answer": "", "note": "и ещё вот что"}}));
        assert!(verdict_text(&verdict, false).contains("добавь к новой проверке --after first и оставь пунктам их номера"));
        assert!(verdict_text(&verdict, true).contains("Повторная проверка: --after first, те же номера пунктов."));
        common::record(&verdict);
        common::record(&json!({"id": "first", "how": "dropped", "results": [{"label": "1"}]}));
        assert_eq!(ended("first").map(|found| found["how"].clone()), Some(json!("rework")));
        assert_eq!(ended("none"), None);
        let yes = json!({"id": "y", "kind": "plan", "how": "done", "results": [{"label": "1", "state": "ok", "note": ""}]});
        assert!(!verdict_text(&yes, false).contains("--after"));
        drop(place);
    }

    #[test]
    fn the_person_of_hitl_is_named_as_kept() {
        let place = place(false);
        common::write_json(&place.home.join("person.json"),
                           &json!({"name": "Маша", "genitive": "Маши", "instrumental": "Машей", "she": true})).unwrap();
        let verdict = json!({"id": "1", "title": "Т", "kind": "action", "how": "done",
                             "results": [{"label": "1", "state": "problem", "unclear": true, "note": ""}]});
        let text = verdict_text(&verdict, false);
        assert!(text.contains("эти пункты она не поняла."), "{text}");
        assert!(text.contains("окно покажет ей над каждым, что она ответила сейчас."), "{text}");
        let compact = verdict_text(&json!({"title": "Т", "how": "rework", "results": [{"label": "1", "state": "ok"}]}), true);
        assert_eq!(compact, "«Т»\n1 работает\nМаша вернула на доработку: в окне на Маке.");
        assert!(green("action").starts_with("Маша отметила, что всё сделала."));
    }
}
