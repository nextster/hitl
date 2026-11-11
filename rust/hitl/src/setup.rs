//! `hitl setup`: the name the verdicts call the person by, and the skill put where Claude Code and Codex find it.
//! The port of `src/hitl/hitl.py` (`forms`, `skill_source`, `link_skill`, `setup`). OWNER: part D.

use crate::common;
use crate::names::{hostile, py_float, py_home, py_join, py_path, py_strip, realpath};
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use std::path::Path;

const SETUP_USAGE: &str = "usage: hitl setup --name NAME [--she] [--genitive FORM] [--instrumental FORM]";

/// The genitive and the instrumental of a Russian first name, by its ending: «Никиты», «Никитой». A name of another
/// language, or a woman's name that ends in a consonant, does not change. `--genitive` and `--instrumental` say them
/// where this guesses wrong.
pub fn forms(name: &str, she: bool) -> (String, String) {
    let same = || (name.to_string(), name.to_string());
    let characters: Vec<char> = name.chars().collect();
    let Some(&end) = characters.last() else { return same() };
    let last: String = end.to_lowercase().collect();
    if !(("а" <= last.as_str() && last.as_str() <= "я") || last == "ё") {
        return same();
    }
    let stem: String = characters[..characters.len() - 1].iter().collect();
    let before: String = characters.len().checked_sub(2).map(|index| characters[index].to_lowercase().collect()).unwrap_or_default();
    let one_of = |text: &str, letters: &[&str]| letters.contains(&text);
    let hushing = one_of(&before, &["ж", "ш", "ч", "щ", "ц"]);
    if last == "а" {
        let genitive = if one_of(&before, &["г", "к", "х", "ж", "ш", "ч", "щ"]) { "и" } else { "ы" };
        return (format!("{stem}{genitive}"), format!("{stem}{}", if hushing { "ей" } else { "ой" }));
    }
    if last == "я" {
        return (format!("{stem}и"), format!("{stem}ей"));
    }
    if she {
        return same();
    }
    if last == "й" || last == "ь" {
        return (format!("{stem}я"), format!("{stem}ем"));
    }
    if "аеёиоуыэюя".contains(last.as_str()) {
        return same();
    }
    let instrumental = if one_of(&last, &["ж", "ш", "ч", "щ", "ц"]) { "ем" } else { "ом" };
    (format!("{name}а"), format!("{name}{instrumental}"))
}

/// The skill's folder by a path that outlives an upgrade: the formula hands its `opt` prefix over as `HITL_ROOT`.
fn skill_source() -> String {
    py_join(&common::root().to_string_lossy(), "skills/hitl")
}

/// Puts the skill into one folder of skills; what was done, or why not.
fn link_skill(base: &str, source: &str) -> std::io::Result<String> {
    let target = py_join(base, "hitl");
    let place = Path::new(&target);
    if std::fs::symlink_metadata(place).is_ok_and(|meta| meta.file_type().is_symlink()) {
        if py_path(&std::fs::read_link(place)?.to_string_lossy()) == py_path(source) {
            return Ok(format!("навык уже на месте: {target}"));
        }
        std::fs::remove_file(place)?;
    } else if place.exists() {
        return Ok(format!("не трогаю {target}: там свой навык hitl, не ссылка. Убери его сам, и запусти hitl setup снова."));
    }
    std::fs::create_dir_all(base)?;
    std::os::unix::fs::symlink(source, place)?;
    Ok(format!("навык: {target} → {source}"))
}

/// `repr` of a text, as Python writes it: in single quotes unless it holds one and no double one, the quote, the
/// backslash and what cannot be printed escaped. Unassigned characters, which Python escapes too, are not known here.
fn py_quote(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') { '"' } else { '\'' };
    let mut out = String::from(quote);
    for character in text.chars() {
        let code = character as u32;
        let separator = matches!(code, 0xA0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000);
        let private = matches!(code, 0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD);
        match character {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ if character == quote => {
                out.push('\\');
                out.push(quote);
            }
            _ if hostile(character) || separator || private => out.push_str(&match code {
                0..=0xFF => format!("\\x{code:02x}"),
                0x100..=0xFFFF => format!("\\u{code:04x}"),
                _ => format!("\\U{code:08x}"),
            }),
            _ => out.push(character),
        }
    }
    out.push(quote);
    out
}

/// `str(value)` of what JSON gave.
fn py_str(value: &Value) -> String {
    fn repr(value: &Value) -> String {
        match value {
            Value::Null => "None".into(),
            Value::Bool(flag) => if *flag { "True" } else { "False" }.into(),
            Value::Number(number) if number.is_f64() => py_float(number.as_f64().unwrap_or(0.0)),
            Value::Number(number) => number.to_string(),
            Value::String(text) => py_quote(text),
            Value::Array(items) => format!("[{}]", items.iter().map(repr).collect::<Vec<_>>().join(", ")),
            Value::Object(map) => {
                format!("{{{}}}", map.iter().map(|(key, value)| format!("{}: {}", py_quote(key), repr(value))).collect::<Vec<_>>().join(", "))
            }
        }
    }
    match value {
        Value::String(text) => text.clone(),
        other => repr(other),
    }
}

/// `str(value or fallback)`.
fn str_or(value: Option<&Value>, fallback: &str) -> String {
    match value {
        Some(value) if common::truthy(Some(value)) => py_str(value),
        _ => fallback.to_string(),
    }
}

/// `shutil.which(name)`: an executable file on the PATH (the system's own PATH when there is none).
fn which(name: &str) -> bool {
    let path = std::env::var_os("PATH").map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/usr/bin:/bin:/usr/sbin:/sbin".into());
    let mut seen = HashSet::new();
    for folder in path.split(':').filter(|_| !path.is_empty()) {
        if !seen.insert(folder) {
            continue;
        }
        let candidate = if folder.is_empty() || folder.ends_with('/') { format!("{folder}{name}") } else { format!("{folder}/{name}") };
        let place = Path::new(&candidate);
        let runs = common::c_path(place).is_some_and(|path| unsafe { libc::access(path.as_ptr(), libc::F_OK | libc::X_OK) } == 0);
        if place.exists() && runs && !place.is_dir() {
            return true;
        }
    }
    false
}

fn setup(arguments: &[String]) -> Result<i32, String> {
    let file = common::home().join("person.json");
    let kept = match common::read_json(&file) {
        Some(Value::Object(kept)) => kept,
        _ => Map::new(),
    };
    let mut name = str_or(kept.get("name"), "");
    let mut she = common::truthy(kept.get("she"));
    let (mut genitive, mut instrumental) = (String::new(), String::new());
    let mut position = 0;
    while position < arguments.len() {
        let argument = arguments[position].as_str();
        if argument == "--she" {
            she = true;
        } else if argument == "--he" {
            she = false;
        } else if matches!(argument, "--name" | "--genitive" | "--instrumental") && position + 1 < arguments.len() {
            position += 1;
            let value = py_strip(&arguments[position]).to_string();
            match argument {
                "--name" => (name, genitive, instrumental) = (value, String::new(), String::new()),
                "--genitive" => genitive = value,
                _ => instrumental = value,
            }
        } else {
            common::say(SETUP_USAGE);
            return Ok(common::EXIT_REFUSED);
        }
        position += 1;
    }
    if name.is_empty() {
        common::say("hitl setup: скажи, как тебя звать в итогах проверок: hitl setup --name Никита (--she для женского рода).");
        return Ok(common::EXIT_REFUSED);
    }
    let mut guessed = forms(&name, she);
    if kept.get("name") == Some(&Value::String(name.clone())) && she == common::truthy(kept.get("she")) {
        guessed = (str_or(kept.get("genitive"), &guessed.0), str_or(kept.get("instrumental"), &guessed.1));
    }
    let genitive = if genitive.is_empty() { guessed.0 } else { genitive };
    let instrumental = if instrumental.is_empty() { guessed.1 } else { instrumental };
    let person = json!({"name": name, "genitive": genitive, "instrumental": instrumental, "she": she});
    std::fs::create_dir_all(common::home()).map_err(|error| error.to_string())?;
    common::write_json(&file, &person).map_err(|error| error.to_string())?;
    println!("Имя в итогах: {name} (нет ответа {genitive}, завершено {instrumental}). Не так — hitl setup --name … --genitive … --instrumental ….");
    let source = skill_source();
    let mut done = HashSet::new();
    let home = py_home();
    for base in [py_join(&home, ".agents/skills"), py_join(&home, ".claude/skills")] {
        if !done.insert(realpath(&base)?) {
            continue;
        }
        println!("{}", link_skill(&base, &source).map_err(|error| error.to_string())?);
    }
    // The build for his team carries the window built: swiftc is wanted only where it does not.
    let shipped = common::root().join("window/CheckPanel").exists();
    if !shipped && !which("swiftc") {
        println!("Нет swiftc: окно собирается им при первой проверке. Поставь инструменты Xcode: xcode-select --install");
    }
    if !cfg!(target_os = "macos") {
        println!("Окно проверок есть только на macOS.");
    }
    println!("Готово. Агент находит навык hitl в Claude Code и в Codex{}", if shipped { "." } else { "; первая проверка соберёт окно." });
    Ok(0)
}

/// `hitl setup ...`: 0 when the name is kept and the skill linked, 64 for what it cannot take, 1 when a file could
/// not be written (where Python ended with a traceback).
pub fn main(arguments: &[String]) -> i32 {
    match setup(arguments) {
        Ok(code) => code,
        Err(error) => {
            common::say(&format!("hitl setup: {error}"));
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_russian_name_is_declined_as_python_declined_it() {
        let cases = [
            ("Никита", false, "Никиты", "Никитой"), ("Вадим", false, "Вадима", "Вадимом"), ("Проверяющий", true, "Проверяющий", "Проверяющий"),
            ("Мария", false, "Марии", "Марией"), ("Илья", false, "Ильи", "Ильей"), ("Саша", true, "Саши", "Сашей"),
            ("Игорь", false, "Игоря", "Игорем"), ("Игорь", true, "Игорь", "Игорь"), ("Любовь", false, "Любовя", "Любовем"),
            ("Олег", false, "Олега", "Олегом"), ("Лёша", false, "Лёши", "Лёшей"), ("Алёна", true, "Алёны", "Алёной"),
            ("Николай", false, "Николая", "Николаем"), ("Ольга", false, "Ольги", "Ольгой"), ("Inna", false, "Inna", "Inna"),
            ("", false, "", ""), ("Кузьма", false, "Кузьмы", "Кузьмой"), ("Лука", false, "Луки", "Лукой"),
            ("Юрий", false, "Юрия", "Юрием"), ("Кац", false, "Каца", "Кацем"), ("Лёха", false, "Лёхи", "Лёхой"),
            ("Ксю", false, "Ксю", "Ксю"), ("Лулу", false, "Лулу", "Лулу"), ("Ё", false, "Ё", "Ё"), ("А", false, "ы", "ой"),
            ("НИКИТА", false, "НИКИТы", "НИКИТой"), ("Ия", false, "Ии", "Ией"),
        ];
        for (name, she, genitive, instrumental) in cases {
            assert_eq!(forms(name, she), (genitive.to_string(), instrumental.to_string()), "{name} {she}");
        }
    }

    #[test]
    fn the_skill_is_linked_once_and_a_folder_of_his_own_is_left() {
        let root = std::env::temp_dir().join(format!("hitl-setup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let base = root.join("skills").display().to_string();
        let source = root.join("opt/skills/hitl").display().to_string();
        assert_eq!(link_skill(&base, &source).unwrap(), format!("навык: {base}/hitl → {source}"));
        assert_eq!(link_skill(&base, &source).unwrap(), format!("навык уже на месте: {base}/hitl"));
        assert_eq!(link_skill(&base, &format!("{source}/")).unwrap(), format!("навык уже на месте: {base}/hitl"));
        let other = root.join("other").display().to_string();
        assert_eq!(link_skill(&base, &other).unwrap(), format!("навык: {base}/hitl → {other}"));
        std::fs::remove_file(format!("{base}/hitl")).unwrap();
        std::fs::create_dir_all(format!("{base}/hitl")).unwrap();
        assert!(link_skill(&base, &source).unwrap().starts_with("не трогаю "));
        let _ = std::fs::remove_dir_all(&root);
    }
}
