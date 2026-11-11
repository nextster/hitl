//! A check's marks from the two screens, the verdict a chat gets back, and the end of a check (queues.py, "Verdicts"
//! and "The same check on two screens").

use crate::bridge;
use crate::state::{Queue, STATE_GROUPS, archive, count_marks, describe, is_green, summarize, window_ticket};
use crate::base::now;
use hitl_host_core::{clock, dumps, minutes, read_json};
use serde_json::{Map, Value, json};
use std::collections::HashMap;

fn text_field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

/// Python's `x or 0` on a number read from JSON.
fn at_of(entry: &Value) -> f64 {
    entry.get("at").and_then(Value::as_f64).unwrap_or(0.0)
}

/// What he has marked and written in the window on the Mac for this check.
pub fn said_on_the_mac(queue: &Queue, ticket: &Value) -> Option<Value> {
    let mine = read_json(&queue.ticket_folder(text_field(ticket, "id")).join("mac.json"))?;
    (mine.is_object() && mine.get("id") == ticket.get("id")).then_some(mine)
}

/// What the app is told of the window on the Mac: the items touched there, and that the check was ended there.
pub fn for_the_phone(mine: &Value) -> Value {
    let results: Vec<Value> = mine.get("results").and_then(Value::as_array).into_iter().flatten()
        .filter(|entry| entry.is_object() && entry.get("label").is_some_and(Value::is_string) && hitl_host_core::truthy(entry.get("at")))
        .map(|entry| json!({"label": entry["label"], "state": entry.get("state").cloned().unwrap_or(Value::Null),
                            "note": if hitl_host_core::truthy(entry.get("note")) { entry["note"].clone() } else { Value::Null },
                            "at": entry["at"]}))
        .collect();
    let mut said = Map::new();
    said.insert("id".into(), mine["id"].clone());
    said.insert("results".into(), Value::Array(results));
    said.insert("finished".into(), json!(hitl_host_core::truthy(mine.get("finish"))));
    // The item he is at in the window, and since when: the panel on the phone goes there too.
    if hitl_host_core::truthy(mine.get("current")) && hitl_host_core::truthy(mine.get("currentAt")) {
        said.insert("current".into(), mine["current"].clone());
        said.insert("currentAt".into(), mine["currentAt"].clone());
    }
    // A button of an item pressed in the window: the app runs it as a tap on its own button, once for a press.
    if let Some(run) = mine.get("run").filter(|run| run.is_object()) {
        if run.get("label").is_some_and(Value::is_string) && run.get("action").is_some_and(Value::is_string)
            && hitl_host_core::truthy(run.get("at")) {
            said.insert("run".into(), json!({"label": run["label"], "action": run["action"], "at": run["at"]}));
        }
    }
    Value::Object(said)
}

/// Days since 1970-01-01 of a civil date (proleptic Gregorian).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146097 + day_of_era - 719468
}

/// A moment the app wrote as text (`2026-10-07T12:00:00Z`, with a fraction or an offset), in seconds; 0 when it is
/// not one. A time with no offset is the Mac's local time, as Python's `fromisoformat(...).timestamp()` takes it.
pub fn seconds(stamp: &Value) -> f64 {
    let Some(text) = stamp.as_str() else { return 0.0 };
    parse_stamp(text).unwrap_or(0.0)
}

fn parse_stamp(text: &str) -> Option<f64> {
    let text = text.trim();
    let (date, time) = text.split_once(['T', ' ']).unwrap_or((text, "00:00:00"));
    let mut parts = date.split('-');
    let (year, month, day): (i64, i64, i64) = (parts.next()?.parse().ok()?, parts.next()?.parse().ok()?, parts.next()?.parse().ok()?);
    if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // The offset: Z, +HH:MM, -HH:MM, +HHMM.
    let (clock_part, offset): (&str, Option<i64>) = if let Some(rest) = time.strip_suffix(['Z', 'z']) {
        (rest, Some(0))
    } else if let Some(position) = time.rfind(['+', '-']) {
        let (head, zone) = time.split_at(position);
        let sign = if zone.starts_with('-') { -1 } else { 1 };
        let digits: String = zone[1..].chars().filter(char::is_ascii_digit).collect();
        if digits.len() < 2 {
            return None;
        }
        let hours: i64 = digits[..2].parse().ok()?;
        let minutes: i64 = if digits.len() >= 4 { digits[2..4].parse().ok()? } else { 0 };
        (head, Some(sign * (hours * 3600 + minutes * 60)))
    } else {
        (time, None)
    };
    let mut pieces = clock_part.split(':');
    let hour: i64 = pieces.next()?.parse().ok()?;
    let minute: i64 = pieces.next().map_or(Some(0), |piece| piece.parse().ok())?;
    let second_text = pieces.next().unwrap_or("0");
    let second: f64 = second_text.parse().ok()?;
    if hour > 23 || minute > 59 || second >= 60.0 {
        return None;
    }
    let days = days_from_civil(year, month, day);
    let wall = (days * 86400 + hour * 3600 + minute * 60) as f64 + second;
    match offset {
        Some(offset) => Some(wall - offset as f64),
        None => {
            // Local time: ask the system what the offset was at that wall-clock moment.
            let mut parts: libc::tm = unsafe { std::mem::zeroed() };
            parts.tm_year = (year - 1900) as i32;
            parts.tm_mon = (month - 1) as i32;
            parts.tm_mday = day as i32;
            parts.tm_hour = hour as i32;
            parts.tm_min = minute as i32;
            parts.tm_isdst = -1;
            let local = unsafe { libc::mktime(&mut parts) };
            Some(local as f64 + second)
        }
    }
}

/// The ticket's items with their marks, matched by label. A mark may have been made on the phone or in the window on
/// the Mac: of two marks of one item the later one stays. An app that does not say when a mark was made is taken at the
/// time of its whole report.
pub fn merge_report(ticket: &Value, report: Option<&Value>, mine: Option<&Value>) -> Vec<Value> {
    let mut reported: HashMap<String, Value> = HashMap::new();
    if let Some(report) = report.filter(|report| report.get("id") == ticket.get("id") && report.get("id").is_some()) {
        for entry in report.get("results").and_then(Value::as_array).into_iter().flatten() {
            let Some(label) = entry.get("label").and_then(Value::as_str).filter(|_| entry.is_object()) else { continue };
            let touched = hitl_host_core::truthy(entry.get("state")) || hitl_host_core::truthy(entry.get("note"));
            let mut copy = entry.clone();
            let at = if hitl_host_core::truthy(entry.get("at")) {
                entry["at"].clone()
            } else if touched {
                json!(seconds(report.get("updatedAt").unwrap_or(&Value::Null)))
            } else {
                json!(0)
            };
            copy["at"] = at;
            reported.insert(label.to_string(), copy);
        }
    }
    let mut typed: HashMap<String, &Value> = HashMap::new();
    if let Some(mine) = mine.filter(|mine| mine.get("id") == ticket.get("id") && mine.get("id").is_some()) {
        for entry in mine.get("results").and_then(Value::as_array).into_iter().flatten() {
            if let Some(label) = entry.get("label").and_then(Value::as_str).filter(|_| entry.is_object()) {
                typed.insert(label.to_string(), entry);
            }
        }
    }
    let empty = json!({});
    let mut merged = Vec::new();
    for item in ticket.get("items").and_then(Value::as_array).into_iter().flatten() {
        let label = text_field(item, "label");
        let phone = reported.get(label).unwrap_or(&empty);
        let mac = typed.get(label).copied().unwrap_or(&empty);
        let entry = if at_of(mac) > at_of(phone) { mac } else { phone };
        let state = entry.get("state").and_then(Value::as_str).filter(|state| ["ok", "problem", "skipped"].contains(state));
        let note = entry.get("note").and_then(Value::as_str).filter(|note| !note.trim().is_empty());
        let mut result = Map::new();
        result.insert("label".into(), json!(label));
        result.insert("text".into(), item.get("text").cloned().unwrap_or(Value::Null));
        result.insert("state".into(), state.map_or(Value::Null, |state| json!(state)));
        result.insert("note".into(), note.map_or(Value::Null, |note| json!(note)));
        if hitl_host_core::truthy(entry.get("at")) {
            result.insert("at".into(), entry["at"].clone());
        }
        // Screenshots are attached on the Mac only, whichever screen the mark came from.
        let shots: Vec<Value> = mac.get("shots").and_then(Value::as_array).into_iter().flatten().filter(|shot| shot.is_string()).cloned().collect();
        if !shots.is_empty() {
            result.insert("shots".into(), Value::Array(shots));
        }
        merged.push(Value::Object(result));
    }
    merged
}

fn group_title(mark: Option<&str>) -> &'static str {
    STATE_GROUPS.iter().find(|(key, _)| *key == mark).map_or("Без отметки", |(_, title)| title)
}

/// What the chat gets back: the items by number, grouped by mark, and the comments. The chat wrote the items, so
/// their numbers are enough.
pub fn verdict_text(ticket: &Value, compact: bool) -> String {
    let result = ticket.get("result").cloned().unwrap_or(json!({}));
    let items: Vec<Value> = result.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
    let unmarked: Vec<&Value> = items.iter().filter(|item| item.get("state").is_none_or(Value::is_null)).collect();
    let how = result.get("how").and_then(Value::as_str);
    let moment = clock(ticket.get("finishedAt").and_then(Value::as_f64));
    let where_ = if how == Some("mac") { "в окне на Маке" } else { "на телефоне" };
    let sent_back = if how == Some("mac") { result.get("ended").and_then(Value::as_str) == Some("rework") } else { !unmarked.is_empty() };
    let (headline, ending);
    if matches!(how, Some("phone") | Some("mac")) && !sent_back {
        headline = format!("Проверка {} закончена в {moment}: отмечены все пункты.", describe(ticket));
        ending = format!("кнопкой «Готово» {where_}");
    } else if matches!(how, Some("phone") | Some("mac")) {
        headline = format!("Проверка {} возвращена на доработку в {moment}: отмечено {} из {}.", describe(ticket),
                           items.len() - unmarked.len(), items.len());
        ending = format!("кнопкой «Доработать» {where_}");
    } else if how == Some("dropped") {
        headline = format!("Проверка {} отменена в {moment}.", describe(ticket));
        ending = "командой --queue-drop с Мака".to_string();
    } else {
        headline = format!("Проверка {} снята с Мака в {moment}.", describe(ticket));
        ending = "командой --queue-release с Мака".to_string();
    }
    let mut lines: Vec<String> = vec![if compact { format!("«{}»", text_field(ticket, "title")) } else { headline }, String::new()];
    if compact {
        for item in &items {
            let answer = group_title(item.get("state").and_then(Value::as_str)).to_lowercase();
            let comment = text_field(item, "note").trim().to_string();
            lines.push(format!("{} {answer}{}", text_field(item, "label"), if comment.is_empty() { String::new() } else { format!(". «{comment}»") }));
        }
    } else {
        for (mark, title) in STATE_GROUPS {
            let labels: Vec<&str> = items.iter().filter(|item| item.get("state").and_then(Value::as_str) == mark).map(|item| text_field(item, "label")).collect();
            if !labels.is_empty() {
                lines.push(format!("{title}: {}", labels.join(", ")));
            }
        }
    }
    let comments: Vec<(String, String)> = if compact { Vec::new() } else {
        items.iter().filter(|item| hitl_host_core::truthy(item.get("note")))
            .map(|item| (text_field(item, "label").to_string(), text_field(item, "note").split_whitespace().collect::<Vec<_>>().join(" "))).collect()
    };
    let comment = text_field(&result, "comment").split_whitespace().collect::<Vec<_>>().join(" ");
    if !comments.is_empty() || !comment.is_empty() {
        lines.push(String::new());
        lines.push("Комментарии:".into());
        lines.extend(comments.iter().map(|(label, text)| format!("{label}. {text}")));
        if !comment.is_empty() {
            lines.push(format!("Ко всей проверке: {comment}"));
        }
    }
    let shots: Vec<(String, String)> = items.iter().flat_map(|item| {
        item.get("shots").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str)
            .map(|shot| (text_field(item, "label").to_string(), shot.to_string())).collect::<Vec<_>>()
    }).collect();
    if !shots.is_empty() {
        lines.push(String::new());
        lines.push("Снимки экрана к комментариям, открой файлы:".into());
        lines.extend(shots.iter().map(|(label, shot)| format!("{label}. {shot}")));
    }
    if hitl_host_core::truthy(result.get("note")) {
        lines.push(String::new());
        lines.push(format!("Сказано при снятии: {}", text_field(&result, "note").split_whitespace().collect::<Vec<_>>().join(" ")));
    }
    lines.push(String::new());
    let mut held = String::new();
    if hitl_host_core::truthy(ticket.get("installedAt")) && hitl_host_core::truthy(ticket.get("finishedAt")) {
        held = format!(" Сборка была на телефоне {}.", minutes(ticket["finishedAt"].as_f64().unwrap_or(0.0) - ticket["installedAt"].as_f64().unwrap_or(0.0)));
    }
    lines.push(if compact && matches!(how, Some("phone") | Some("mac")) {
        format!("Завершено {}: {ending}.", hitl::common::by_whom())
    } else {
        format!("Как закончена: {ending}.{held}")
    });
    if is_green(&json!({"how": how, "ended": result.get("ended"), "marks": count_marks(&items)})) {
        lines.push("Проверка принята в её объёме.".into());
    }
    if !matches!(how, Some("phone") | Some("mac")) && !hitl_host_core::truthy(ticket.get("overlay")) {
        lines.push("В этой сборке нет панели чек-листа, отметок с телефона нет: итог проверяющий даёт словами в чате.".into());
    } else if !unmarked.is_empty() {
        lines.push("Пункты без отметки не проверены.".into());
    }
    if !compact {
        lines.push("Телефон уже свободен для следующей проверки. Правки показывай новой проверкой (hitl iphone --check): она встанет в конец очереди.".into());
    }
    lines.into_iter().filter(|line| !line.is_empty() || !compact).collect::<Vec<_>>().join("\n")
}

/// Ends the check that is on the phone and frees the phone. The ticket is the caller's copy of the one in the state;
/// the finished ticket, as it was archived, is returned.
pub fn finish(queue: &Queue, state: &mut Value, mut ticket: Value, how: &str, report: Option<&Value>, note: Option<&str>,
              final_state: &str) -> Value {
    let mine = said_on_the_mac(queue, &ticket);
    ticket["state"] = json!(final_state);
    ticket["finishedAt"] = json!(now());
    let own_report = report.filter(|report| report.get("id") == ticket.get("id"));
    let mut result = Map::new();
    result.insert("how".into(), json!(how));
    result.insert("items".into(), Value::Array(merge_report(&ticket, report, mine.as_ref())));
    result.insert("comment".into(), own_report.and_then(|report| report.get("comment")).filter(|comment| hitl_host_core::truthy(Some(comment))).cloned().unwrap_or(Value::Null));
    result.insert("note".into(), note.map_or(Value::Null, |note| json!(note)));
    if how == "mac" {
        let rework = mine.as_ref().and_then(|mine| mine.get("finish")).and_then(Value::as_str) == Some("rework");
        result.insert("ended".into(), json!(if rework { "rework" } else { "done" }));
    }
    ticket["result"] = Value::Object(result);
    let ticket_id = text_field(&ticket, "id").to_string();
    let _ = std::fs::remove_file(window_ticket(&ticket_id));
    if state.get("active").is_some_and(|active| active.get("id") == ticket.get("id")) {
        state["active"] = Value::Null;
    }
    archive(state, ticket.clone());
    let folder = queue.ticket_folder(&ticket_id);
    let _ = std::fs::create_dir_all(&folder);
    let _ = std::fs::write(folder.join("verdict.md"), verdict_text(&ticket, false) + "\n");
    let _ = std::fs::write(folder.join("verdict.json"), dumps(&ticket));
    // One line per check that was on the phone, kept for good: when it was there and how it ended. Time on hand
    // checks is read from here.
    if hitl_host_core::truthy(ticket.get("installedAt")) {
        let items: Vec<Value> = ticket["result"]["items"].as_array().cloned().unwrap_or_default();
        let take = |key: &str| ticket.get(key).cloned().unwrap_or(Value::Null);
        let or_empty = |key: &str| if hitl_host_core::truthy(ticket.get(key)) { ticket[key].clone() } else { json!("") };
        let results: Vec<Value> = items.iter().map(|item| json!({
            "label": item["label"], "text": item.get("text").cloned().unwrap_or(Value::Null), "state": item.get("state").cloned().unwrap_or(Value::Null),
            "note": if hitl_host_core::truthy(item.get("note")) { item["note"].clone() } else { json!("") }})).collect();
        let mut entry = Map::new();
        for (key, value) in [
            ("id", take("id")), ("screen", json!("iphone")), ("udid", json!(queue.udid)), ("tag", take("tag")), ("chat", take("chat")),
            ("title", take("title")), ("project", take("project")), ("branch", take("branch")), ("installedAt", take("installedAt")),
            ("finishedAt", take("finishedAt")),
            ("lastMarkAt", ticket.get("progress").and_then(|progress| progress.get("updatedAt")).cloned().unwrap_or(Value::Null)),
            ("how", json!(how)), ("ended", ticket["result"].get("ended").cloned().unwrap_or(Value::Null)),
            ("summary", json!(summarize(&items))), ("urgent", json!(hitl_host_core::truthy(ticket.get("urgent")))),
            ("commit", or_empty("commit")), ("marks", count_marks(&items)), ("agent", or_empty("agent")), ("results", Value::Array(results)),
            ("comment", if hitl_host_core::truthy(ticket["result"].get("comment")) { ticket["result"]["comment"].clone() } else { json!("") }),
        ] {
            entry.insert(key.into(), value);
        }
        hitl_host_core::record(&Value::Object(entry));
    }
    // The build was only needed until it left the phone.
    let _ = std::fs::remove_dir_all(folder.join("app"));
    let _ = bridge::window_done_folder;
    ticket
}

/// Drops what a finished check no longer needs: folders of tickets that are neither on the phone, nor waiting, nor
/// among the recent ones.
pub fn prune(queue: &Queue, state: &Value) {
    let mut keep: Vec<String> = Vec::new();
    for key in ["queue", "recent"] {
        keep.extend(state[key].as_array().into_iter().flatten().map(|ticket| text_field(ticket, "id").to_string()));
    }
    if let Some(active) = state.get("active").filter(|active| active.is_object()) {
        keep.push(text_field(active, "id").to_string());
    }
    let Ok(entries) = std::fs::read_dir(queue.folder.join("tickets")) else { return };
    for entry in entries.flatten() {
        if !keep.contains(&entry.file_name().to_string_lossy().to_string()) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticket() -> Value {
        json!({"id": "t1", "tag": "r/38", "title": "Бары", "overlay": true,
               "items": [{"label": "1", "text": "Первый"}, {"label": "2", "text": "Второй"}, {"label": "3", "text": "Третий"}]})
    }

    #[test]
    fn a_stamp_is_read_as_the_app_writes_it() {
        assert_eq!(seconds(&json!("2026-10-07T12:00:00Z")), 1791374400.0);
        assert_eq!(seconds(&json!("2026-10-07T15:00:00+03:00")), 1791374400.0);
        assert_eq!(seconds(&json!("2026-10-07T12:00:00.500Z")), 1791374400.5);
        assert_eq!(seconds(&json!("not a time")), 0.0);
        assert_eq!(seconds(&Value::Null), 0.0);
    }

    #[test]
    fn of_two_marks_of_one_item_the_later_one_stays() {
        let report = json!({"id": "t1", "results": [{"label": "1", "state": "ok", "note": null, "at": 10},
                                                     {"label": "2", "state": "problem", "note": "криво", "at": 20}]});
        let mine = json!({"id": "t1", "results": [{"label": "2", "state": "ok", "at": 30, "shots": ["/x.png"]},
                                                  {"label": "3", "state": "skipped", "at": 5}]});
        let merged = merge_report(&ticket(), Some(&report), Some(&mine));
        assert_eq!(merged[0], json!({"label": "1", "text": "Первый", "state": "ok", "note": null, "at": 10}));
        assert_eq!(merged[1], json!({"label": "2", "text": "Второй", "state": "ok", "note": null, "at": 30, "shots": ["/x.png"]}));
        assert_eq!(merged[2]["state"], "skipped");
    }

    #[test]
    fn an_app_that_does_not_say_when_is_taken_at_the_time_of_its_report() {
        let report = json!({"id": "t1", "updatedAt": "2026-10-07T12:00:00Z", "results": [{"label": "1", "state": "ok"}, {"label": "2"}]});
        let merged = merge_report(&ticket(), Some(&report), None);
        assert_eq!(merged[0]["at"], json!(1791374400.0));
        assert!(merged[1].get("at").is_none());
        // A report of another check says nothing of this one.
        let other = json!({"id": "other", "results": [{"label": "1", "state": "ok", "at": 1}]});
        assert_eq!(merge_report(&ticket(), Some(&other), None)[0]["state"], Value::Null);
    }

    #[test]
    fn the_verdict_groups_the_items_by_mark() {
        let mut done = ticket();
        done["finishedAt"] = json!(100.0);
        done["installedAt"] = json!(40.0);
        done["result"] = json!({"how": "phone", "comment": "всё хорошо", "items": [
            {"label": "1", "text": "Первый", "state": "ok", "note": null}, {"label": "2", "text": "Второй", "state": "problem", "note": "не  едет"},
            {"label": "3", "text": "Третий", "state": null, "note": null}]});
        let text = verdict_text(&done, false);
        assert!(text.contains("Работает: 1\nНе работает: 2\nБез отметки: 3"), "{text}");
        assert!(text.contains("Комментарии:\n2. не едет\nКo всей проверке: всё хорошо".replace('o', "о").as_str()), "{text}");
        assert!(text.contains("возвращена на доработку"), "{text}");
        assert!(text.contains("Пункты без отметки не проверены."));
        let compact = verdict_text(&done, true);
        assert!(compact.starts_with("«Бары»\n1 работает\n2 не работает. «не  едет»\n3 без отметки"), "{compact}");
    }
}
