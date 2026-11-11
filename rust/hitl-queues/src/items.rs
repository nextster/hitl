//! A check's checklist: its items, the file they may come from, what the app is handed (queues.py, "Checklist input").
//!
//! There is no regular-expression crate here: the three patterns of the Python are matched by hand.

use hitl_host_core::git::project_name;
use serde_json::{Map, Value, json};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Joins an item's text to its `setup:`, `buttons:` and `link:` fields; `hitl iphone` writes it, the queue splits it.
pub const FIELD_SEPARATOR: &str = "\u{1f}";

/// What the app understands after: the checklist as base64url of JSON.
pub const CHECKLIST_ARGUMENT: &str = "--ios-run-checklist";

/// What a run that came to the phone without saying why is told.
pub fn device_modes_text() -> String {
    "Error: сборка на телефон ставится через очередь, командой hitl iphone, в одном из двух режимов. Выбери:

  hitl iphone … --check \"что сделать → что должно быть\"    (по одному на пункт; тема: --check-title \"…\")
      Проверяющему нужно проверить руками. Сборка встаёт в очередь проверок: на телефоне
      одна проверка за раз, следующая ставится сама, когда он закончит текущую
      в чек-листе на телефоне («Готово», если отмечены все пункты, иначе
      «Доработать»). Команда ждёт и печатает номера пунктов по отметкам и его
      комментарии, поэтому запускай её в фоне.

  hitl iphone … --probe
      Телефон нужен тебе самому: замер, лог, запуск ради диагностики. Идёт только
      когда проверок нет ни на телефоне, ни в очереди, и уступает следующей
      проверке. Сборку от пробы Проверяющему на проверку не отдают.

Остальные параметры те же, что у ios-run.sh: hitl iphone собирает им.
Собрать и прогнать тесты без телефона: ios-run.sh --mac. Кто сейчас на телефоне: hitl queue."
        .to_string()
}

/// `^\s*(?:[-*]\s+)?(\d+[a-zа-яё]?)[.):]\s+(.*\S)\s*$` (ignoring case): the label an item came with, and its text.
pub fn item_label(text: &str) -> Option<(String, String)> {
    let mut rest = text.trim_start();
    if let Some(after) = rest.strip_prefix(['-', '*']) {
        if after.starts_with(char::is_whitespace) {
            rest = after.trim_start();
        }
    }
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let mut label_end = digits;
    let mut after = rest[digits..].chars();
    let mut next = after.next()?;
    if next.is_alphabetic() && is_label_letter(next) {
        label_end += next.len_utf8();
        next = after.next()?;
    }
    if !matches!(next, '.' | ')' | ':') {
        return None;
    }
    let tail = &rest[label_end + 1..];
    if !tail.starts_with(char::is_whitespace) {
        return None;
    }
    let body = tail.trim();
    if body.is_empty() {
        return None;
    }
    Some((rest[..label_end].to_string(), body.to_string()))
}

fn is_label_letter(letter: char) -> bool {
    let lower = letter.to_lowercase().next().unwrap_or(letter);
    lower.is_ascii_lowercase() || ('а'..='я').contains(&lower) || lower == 'ё'
}

/// `^\s*[-*]\s+(.*\S)\s*$`: a bulleted line's text.
fn bullet(text: &str) -> Option<String> {
    let rest = text.trim_start().strip_prefix(['-', '*'])?;
    if !rest.starts_with(char::is_whitespace) || rest.trim().is_empty() {
        return None;
    }
    Some(rest.trim().to_string())
}

/// `^\s*(setup|buttons?|link)\s*[:=]\s*(.*\S)\s*$` (ignoring case): an item's field, its key in lower case and value.
pub fn item_field(text: &str) -> Option<(String, String)> {
    let rest = text.trim_start();
    let lower = rest.to_lowercase();
    for key in ["buttons", "button", "setup", "link"] {
        if lower.starts_with(key) && rest.is_char_boundary(key.len()) {
            let after = rest[key.len()..].trim_start();
            if let Some(value) = after.strip_prefix([':', '=']) {
                let value = value.trim();
                if !value.is_empty() {
                    return Some((key.to_string(), value.to_string()));
                }
            }
        }
    }
    None
}

/// Items keep the number they came with ("4a. …"), since the numbers are how the reviewer and the chat refer to them; the
/// rest continue the count.
pub fn parse_items(raw_items: &[String]) -> Vec<Value> {
    let mut items = Vec::new();
    let mut used: Vec<String> = Vec::new();
    let mut highest: i64 = 0;
    for raw in raw_items {
        let mut parts = raw.split(FIELD_SEPARATOR);
        let raw_text = parts.next().unwrap_or("");
        let text = raw_text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            continue;
        }
        let (label, text) = match item_label(&text) {
            Some((label, body)) if !used.contains(&label) => (label, body),
            _ => ((highest + 1).to_string(), text),
        };
        used.push(label.clone());
        let number: i64 = label.chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap_or(0);
        highest = highest.max(number);
        let mut item = Map::new();
        item.insert("label".into(), json!(label));
        item.insert("text".into(), json!(text));
        for field in parts {
            let Some((key, value)) = item_field(field) else { continue };
            if key == "link" {
                item.insert("link".into(), json!(value));
            } else {
                let key = if key.starts_with("button") { "buttons" } else { "setup" };
                let slot = item.entry(key.to_string()).or_insert_with(|| json!([]));
                let words = value.split(|c: char| c.is_whitespace() || c == ',').filter(|word| !word.is_empty());
                slot.as_array_mut().expect("list").extend(words.map(|word| json!(word)));
            }
        }
        items.push(Value::Object(item));
    }
    items
}

/// A checklist file: an optional first line or "# heading" as the title, then one item per line, numbered or
/// bulleted. A line "setup: …", "buttons: …" or "link: …" under an item belongs to that item.
pub fn read_checklist_file(path: &str) -> std::io::Result<(String, Vec<String>)> {
    let content = std::fs::read_to_string(path)?;
    let lines: Vec<&str> = content.lines().collect();
    let mut title = String::new();
    let mut items: Vec<String> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let stripped = line.trim();
        if stripped.is_empty() {
            continue;
        }
        if stripped.starts_with('#') {
            if title.is_empty() {
                title = stripped.trim_start_matches('#').trim().to_string();
            }
            continue;
        }
        if !items.is_empty() && item_field(stripped).is_some() {
            let last = items.last_mut().expect("an item");
            *last = format!("{last}{FIELD_SEPARATOR}{stripped}");
        } else if item_label(stripped).is_some() {
            items.push(stripped.to_string());
        } else if let Some(text) = bullet(stripped) {
            items.push(text);
        } else if title.is_empty() && items.is_empty() && index + 1 < lines.len() {
            title = stripped.to_string();
        } else if !items.is_empty() && !items.last().expect("an item").contains(FIELD_SEPARATOR) {
            // A wrapped line continues the item above it.
            let last = items.last_mut().expect("an item");
            *last = format!("{last} {stripped}");
        } else {
            items.push(stripped.to_string());
        }
    }
    Ok((title, items))
}

/// base64url without padding.
pub fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[n as usize & 63] as char);
        }
    }
    out
}

/// What the app receives after --ios-run-checklist: base64url of JSON, so no quoting on the way through devicectl can
/// touch the text.
pub fn checklist_payload(ticket: &Value) -> String {
    let mut body = Map::new();
    for key in ["id", "tag", "title", "items"] {
        body.insert(key.into(), ticket.get(key).cloned().unwrap_or(Value::Null));
    }
    // Which of his projects the build is of, by the name he knows: the band says it first, as the window does.
    let project = ticket.get("project").and_then(Value::as_str).filter(|project| !project.is_empty());
    if let Some(project) = project {
        let name = project_name(Some(project));
        if !name.is_empty() {
            body.insert("project".into(), json!(name));
        }
    }
    base64url(serde_json::to_string(&Value::Object(body)).expect("JSON").as_bytes())
}

/// Whether the built app knows the argument: its name is a string in the binary. A build from a branch without the
/// overlay is checked by word.
pub fn app_supports_checklist(app_path: &Path) -> bool {
    let needle = CHECKLIST_ARGUMENT.as_bytes();
    let mut files = Vec::new();
    collect_files(app_path, &mut files);
    for path in files {
        let Ok(mut handle) = std::fs::File::open(&path) else { continue };
        if handle.metadata().map(|meta| meta.len() < needle.len() as u64).unwrap_or(true) {
            continue;
        }
        let mut head = [0u8; 4];
        if handle.read_exact(&mut head).is_err() {
            continue;
        }
        // Mach-O thin and fat binaries, either byte order.
        const MAGICS: [[u8; 4]; 6] = [[0xcf, 0xfa, 0xed, 0xfe], [0xfe, 0xed, 0xfa, 0xcf], [0xca, 0xfe, 0xba, 0xbe],
                                      [0xbe, 0xba, 0xfe, 0xca], [0xce, 0xfa, 0xed, 0xfe], [0xfe, 0xed, 0xfa, 0xce]];
        if !MAGICS.contains(&head) || handle.seek(SeekFrom::Start(0)).is_err() {
            continue;
        }
        let mut tail: Vec<u8> = Vec::new();
        let mut chunk = vec![0u8; 1 << 20];
        loop {
            let Ok(count) = handle.read(&mut chunk) else { break };
            if count == 0 {
                break;
            }
            let mut window = tail.clone();
            window.extend_from_slice(&chunk[..count]);
            if window.windows(needle.len()).any(|slice| slice == needle) {
                return true;
            }
            tail = chunk[..count][count.saturating_sub(needle.len())..].to_vec();
        }
    }
    false
}

/// The regular files under a folder, symlinks not followed.
fn collect_files(folder: &Path, found: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            collect_files(&entry.path(), found);
        } else if kind.is_file() {
            found.push(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn items_keep_the_numbers_they_came_with() {
        let items = parse_items(&strings(&["3. Зажми чип → он едет", "4a) Меню открылось", "Без номера", "3. Дубль номера"]));
        let labels: Vec<&str> = items.iter().map(|item| item["label"].as_str().unwrap()).collect();
        assert_eq!(labels, ["3", "4a", "5", "6"]);
        assert_eq!(items[0]["text"], "Зажми чип → он едет");
        assert_eq!(items[3]["text"], "3. Дубль номера");
    }

    #[test]
    fn an_item_carries_what_the_app_can_do_for_it() {
        let s = FIELD_SEPARATOR;
        let items = parse_items(&[
            format!("1. Открой настройки → видна строка{s}setup=open-settings, dock-fold{s}buttons=dock-unfold{s}buttons=range-week"),
            format!("2. Открой задачу{s}link=realize://screen/main"),
            "3. Без действий".to_string(),
        ]);
        assert_eq!(items, vec![
            json!({"label": "1", "text": "Открой настройки → видна строка", "setup": ["open-settings", "dock-fold"],
                   "buttons": ["dock-unfold", "range-week"]}),
            json!({"label": "2", "text": "Открой задачу", "link": "realize://screen/main"}),
            json!({"label": "3", "text": "Без действий"}),
        ]);
    }

    #[test]
    fn a_file_gives_the_fields_of_its_items() {
        let folder = std::env::temp_dir().join(format!("items-test-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("actions.md");
        std::fs::write(&path, "Тема\n1. Открой настройки → видна строка\n   setup: open-settings, dock-fold\n   button: dock-unfold\n\
                               2. Открой задачу\n   link: realize://screen/main\n").unwrap();
        let (title, lines) = read_checklist_file(path.to_str().unwrap()).unwrap();
        assert_eq!(title, "Тема");
        assert_eq!(parse_items(&lines), vec![
            json!({"label": "1", "text": "Открой настройки → видна строка", "setup": ["open-settings", "dock-fold"], "buttons": ["dock-unfold"]}),
            json!({"label": "2", "text": "Открой задачу", "link": "realize://screen/main"}),
        ]);
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_file_gives_the_title_and_the_items() {
        let folder = std::env::temp_dir().join(format!("items-test-title-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("check.md");
        std::fs::write(&path, "# Бары внизу\n\n1. Свайпни между экранами → футер меняется плавно\n   и без рывка\n- Шевроны фейдятся\n").unwrap();
        let (title, items) = read_checklist_file(path.to_str().unwrap()).unwrap();
        assert_eq!(title, "Бары внизу");
        assert_eq!(items, strings(&["1. Свайпни между экранами → футер меняется плавно и без рывка", "Шевроны фейдятся"]));
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn the_checklist_says_which_project_the_build_is_of() {
        // «На телефоне хотелось бы тот же функционал»: the band says the project first, as the window does.
        let folder = std::env::temp_dir().join(format!("items-test-project-{}", std::process::id()));
        std::fs::create_dir_all(folder.join("demo")).unwrap();
        let project = folder.join("demo").display().to_string();
        let ticket = json!({"id": "1", "tag": "r/38", "title": "Бары", "items": [{"label": "1", "text": "Свайпни"}], "project": project});
        let decoded: Value = serde_json::from_slice(&unbase64(&checklist_payload(&ticket))).unwrap();
        assert_eq!(decoded["project"], project_name(Some(&project)));
        assert!(!decoded["project"].as_str().unwrap().is_empty());
        let without = json!({"id": "1", "tag": "r/38", "title": "Бары", "items": [], "project": ""});
        let decoded: Value = serde_json::from_slice(&unbase64(&checklist_payload(&without))).unwrap();
        assert!(decoded.get("project").is_none());
        let _ = std::fs::remove_dir_all(folder);
    }

    fn unbase64(text: &str) -> Vec<u8> {
        let value = |c: char| match c {
            'A'..='Z' => c as u32 - 'A' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 26,
            '0'..='9' => c as u32 - '0' as u32 + 52,
            '-' => 62,
            _ => 63,
        };
        let chars: Vec<u32> = text.chars().map(value).collect();
        let mut out = Vec::new();
        for chunk in chars.chunks(4) {
            let n = chunk.iter().enumerate().fold(0u32, |acc, (i, v)| acc | v << (18 - 6 * i));
            out.push((n >> 16) as u8);
            if chunk.len() > 2 {
                out.push((n >> 8) as u8);
            }
            if chunk.len() > 3 {
                out.push(n as u8);
            }
        }
        out
    }

    #[test]
    fn the_app_is_known_by_the_argument_in_its_binary() {
        let folder = std::env::temp_dir().join(format!("items-test-app-{}", std::process::id()));
        let app = folder.join("Demo.app");
        std::fs::create_dir_all(&app).unwrap();
        let mut with = vec![0xcf, 0xfa, 0xed, 0xfe];
        with.extend(vec![0u8; 64]);
        with.extend(b"--ios-run-checklist\0");
        std::fs::write(app.join("Demo"), &with).unwrap();
        assert!(app_supports_checklist(&app));
        let mut without = vec![0xcf, 0xfa, 0xed, 0xfe];
        without.extend(vec![0u8; 64]);
        without.extend(b"--ios-run-tag\0");
        std::fs::write(app.join("Demo"), &without).unwrap();
        assert!(!app_supports_checklist(&app));
        // Not a Mach-O file: the word in it does not count.
        std::fs::write(app.join("Demo"), b"plain --ios-run-checklist text").unwrap();
        assert!(!app_supports_checklist(&app));
        let _ = std::fs::remove_dir_all(folder);
    }
}
