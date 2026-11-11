//! His settings of Claude Code, as `hitl install` puts the hook and the rule of `hitl note` into them, takes
//! them out, and puts in the status line that keeps Claude's limits.
//!
//! The port of the settings half of `note.py` (`settings_file` to `launcher`). `hitl note` itself, the hook
//! that runs after every Bash call, is in `hitl-claude`, which also holds the words of the rule.

pub use hitl_claude::note::{MARK, RULE, RULE_HEAD};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};


/// `shlex.quote`: a word as a shell reads it.
pub fn quote(word: &str) -> String {
    let safe = |c: char| c.is_ascii_alphanumeric() || "@%+=:,./-_".contains(c);
    if !word.is_empty() && word.chars().all(safe) {
        return word.to_string();
    }
    format!("'{}'", word.replace('\'', "'\"'\"'"))
}

/// This project's command as a shell reads it: by its link among the shared scripts, when there is one.
pub fn program() -> String {
    quote(&hitl_host_core::command(&[]))
}

pub fn hook_command() -> String {
    program() + " note"
}

pub fn settings_file() -> PathBuf {
    match hitl_host_core::var("CONVEYOR_CLAUDE_SETTINGS").filter(|value| !value.is_empty()) {
        Some(named) => PathBuf::from(named),
        None => hitl_host_core::paths::expand("~/.claude/settings.json"),
    }
}

/// `hitl' note` or `hitl note` at the end of a command, after a slash or at its start.
pub fn is_our_hook(handler: &Value) -> bool {
    let Some(command) = handler.get("command").and_then(Value::as_str) else { return false };
    let Some(rest) = command.trim().strip_suffix(" note") else { return false };
    let rest = rest.strip_suffix('\'').unwrap_or(rest);
    match rest.strip_suffix("hitl") {
        Some(before) => before.is_empty() || before.ends_with('/'),
        None => false,
    }
}

fn groups(settings: &Map<String, Value>) -> &[Value] {
    settings.get("hooks").and_then(|hooks| hooks.get("PostToolUse")).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

pub fn has_hook(settings: &Map<String, Value>) -> bool {
    groups(settings).iter().filter(|group| group.is_object()).any(|group| {
        group.get("hooks").and_then(Value::as_array).is_some_and(|handlers| handlers.iter().any(is_our_hook))
    })
}

pub fn has_rule(settings: &Map<String, Value>) -> bool {
    settings
        .get("autoMode")
        .and_then(|auto| auto.get("allow"))
        .and_then(Value::as_array)
        .is_some_and(|allow| allow.iter().any(|entry| entry.as_str() == Some(RULE)))
}

/// This rule in any wording: an older one gives way to the one this version writes.
pub fn is_ours(entry: &Value) -> bool {
    entry.as_str().is_some_and(|text| text.starts_with(RULE_HEAD))
}

/// Whether his settings of Claude Code hold the hook and the rule as this version writes them.
pub fn state(path: &Path) -> (bool, bool) {
    match hitl_host_core::read_json(path) {
        Some(Value::Object(settings)) => (has_hook(&settings), has_rule(&settings)),
        _ => (false, false),
    }
}

/// His settings, or {} where there are none yet. A file that is not what Claude Code writes is left alone.
pub fn read_settings(path: &Path) -> Result<Map<String, Value>, String> {
    if !path.exists() {
        return Ok(Map::new());
    }
    let text = std::fs::read_to_string(path).map_err(|problem| unreadable(path, &problem.to_string()))?;
    match serde_json::from_str::<Value>(&text) {
        Err(problem) => Err(unreadable(path, &problem.to_string())),
        Ok(Value::Object(settings)) => Ok(settings),
        Ok(_) => Err(format!("{} не похож на настройки Claude Code: ничего не менял.", path.display())),
    }
}

fn unreadable(path: &Path, problem: &str) -> String {
    format!("{} не читается как настройки Claude Code ({problem}): ничего не менял.", path.display())
}

/// What his settings were before this command first touched them.
pub fn kept_copy(path: &Path) -> PathBuf {
    let name = path.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default();
    path.with_file_name(format!("{name}.before-conveyor"))
}

/// As Python's `json.dumps(settings, ensure_ascii=False, indent=2)` and a newline.
fn text_of(settings: &Map<String, Value>) -> String {
    serde_json::to_string_pretty(settings).expect("JSON") + "\n"
}

pub fn write_settings(path: &Path, settings: &Map<String, Value>) -> Result<(), String> {
    let failed = |problem: std::io::Error| format!("{}: {problem}", path.display());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(failed)?;
    }
    if path.exists() && !kept_copy(path).exists() {
        std::fs::copy(path, kept_copy(path)).map_err(failed)?;
    }
    let name = path.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default();
    let temporary = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::write(&temporary, text_of(settings)).map_err(failed)?;
    std::fs::rename(&temporary, path).map_err(failed)
}

/// Puts the hook and the rule into his settings; says what it did, nothing when both were there.
pub fn switch_on(path: &Path) -> Result<Vec<String>, String> {
    let mut settings = read_settings(path)?;
    let mut done = Vec::new();
    if !has_hook(&settings) {
        let wrong = || format!("в {} раздел hooks записан не так, как его пишет Claude Code: ничего не менял.", path.display());
        let hooks = settings.entry("hooks").or_insert_with(|| json!({}));
        let Some(hooks) = hooks.as_object_mut() else { return Err(wrong()) };
        let Some(list) = hooks.entry("PostToolUse").or_insert_with(|| json!([])).as_array_mut() else { return Err(wrong()) };
        list.push(json!({"matcher": "Bash", "hooks": [{"type": "command", "command": hook_command(), "timeout": 10}]}));
        done.push(format!("команда после каждого вызова Bash: {}", hook_command()));
    }
    if !has_rule(&settings) {
        let wrong = || format!("в {} раздел autoMode записан не так, как его пишет Claude Code: ничего не менял.", path.display());
        let auto = settings.entry("autoMode").or_insert_with(|| json!({}));
        let Some(auto) = auto.as_object_mut() else { return Err(wrong()) };
        if auto.get("allow").is_some_and(|allow| !allow.is_array()) {
            return Err(wrong());
        }
        // With no list of his own, the built-in exceptions stay: "$defaults" stands for them. A list he wrote
        // himself is his, with or without them.
        let own = auto
            .get("allow")
            .and_then(Value::as_array)
            .filter(|list| !list.is_empty())
            .cloned()
            .unwrap_or_else(|| vec![json!("$defaults")]);
        let mut allow: Vec<Value> = own.into_iter().filter(|entry| !is_ours(entry)).collect();
        allow.push(json!(RULE));
        auto.insert("allow".into(), Value::Array(allow));
        done.push("правило для режима Auto: ответ в окне проверок — твоё слово".to_string());
    }
    if !done.is_empty() {
        write_settings(path, &settings)?;
    }
    Ok(done)
}

/// Takes the hook and the rule out of his settings, and nothing else.
pub fn switch_off(path: &Path) -> Result<Vec<String>, String> {
    let mut settings = read_settings(path)?;
    let mut done = Vec::new();
    if settings.get("hooks").and_then(|hooks| hooks.get("PostToolUse")).is_some_and(Value::is_array) && has_hook(&settings) {
        let hooks = settings.get_mut("hooks").and_then(Value::as_object_mut).expect("hooks");
        let mut kept = Vec::new();
        for mut group in hooks["PostToolUse"].as_array().cloned().unwrap_or_default() {
            if let Some(handlers) = group.get("hooks").and_then(Value::as_array).cloned() {
                let left: Vec<Value> = handlers.into_iter().filter(|handler| !is_our_hook(handler)).collect();
                if left.is_empty() {
                    continue;
                }
                group["hooks"] = Value::Array(left);
            }
            kept.push(group);
        }
        if kept.is_empty() {
            hooks.shift_remove("PostToolUse");
        } else {
            hooks.insert("PostToolUse".into(), Value::Array(kept));
        }
        if hooks.is_empty() {
            settings.shift_remove("hooks");
        }
        done.push("команда после вызова Bash убрана".to_string());
    }
    let ours =
        settings.get("autoMode").and_then(|auto| auto.get("allow")).and_then(Value::as_array).is_some_and(|allow| allow.iter().any(is_ours));
    if ours {
        let auto = settings.get_mut("autoMode").and_then(Value::as_object_mut).expect("autoMode");
        let allow: Vec<Value> =
            auto["allow"].as_array().cloned().unwrap_or_default().into_iter().filter(|entry| !is_ours(entry)).collect();
        if allow.is_empty() || allow == vec![json!("$defaults")] {
            auto.shift_remove("allow"); // as it was before: the built-in exceptions alone
        } else {
            auto.insert("allow".into(), Value::Array(allow));
        }
        if auto.is_empty() {
            settings.shift_remove("autoMode");
        }
        done.push("правило для режима Auto убрано".to_string());
    }
    if !done.is_empty() {
        write_settings(path, &settings)?;
    }
    Ok(done)
}

/// When a chat of Claude Code is told of the answers it has not read: at its start (resumed or compacted too) and at
/// every message. The wake of the command that waits can be lost; these cannot.
pub const REMIND_AT: [&str; 2] = ["SessionStart", "UserPromptSubmit"];

pub fn remind_command() -> String {
    program() + " _remind"
}

/// `hitl' _remind` or `hitl _remind`, as `is_our_hook` knows `note`.
pub fn is_remind_hook(handler: &Value) -> bool {
    let Some(command) = handler.get("command").and_then(Value::as_str) else { return false };
    let Some(rest) = command.trim().strip_suffix(" _remind") else { return false };
    let rest = rest.strip_suffix('\'').unwrap_or(rest);
    match rest.strip_suffix("hitl") {
        Some(before) => before.is_empty() || before.ends_with('/'),
        None => false,
    }
}

fn has_remind_at(settings: &Map<String, Value>, event: &str) -> bool {
    settings.get("hooks").and_then(|hooks| hooks.get(event)).and_then(Value::as_array).is_some_and(|groups| {
        groups.iter().filter(|group| group.is_object()).any(|group| {
            group.get("hooks").and_then(Value::as_array).is_some_and(|handlers| handlers.iter().any(is_remind_hook))
        })
    })
}

/// Whether his settings remind a chat at every event in `REMIND_AT`.
pub fn remind_state(path: &Path) -> bool {
    match hitl_host_core::read_json(path) {
        Some(Value::Object(settings)) => REMIND_AT.iter().all(|event| has_remind_at(&settings, event)),
        _ => false,
    }
}

/// Puts the reminder hook into his settings beside his own hooks; says what it did, nothing when it was there.
pub fn remind_on(path: &Path) -> Result<Vec<String>, String> {
    let mut settings = read_settings(path)?;
    let mut done = Vec::new();
    for event in REMIND_AT {
        if has_remind_at(&settings, event) {
            continue;
        }
        let wrong = || format!("в {} раздел hooks записан не так, как его пишет Claude Code: ничего не менял.", path.display());
        let hooks = settings.entry("hooks").or_insert_with(|| json!({}));
        let Some(hooks) = hooks.as_object_mut() else { return Err(wrong()) };
        let Some(list) = hooks.entry(event).or_insert_with(|| json!([])).as_array_mut() else { return Err(wrong()) };
        list.push(json!({"hooks": [{"type": "command", "command": remind_command(), "timeout": 5}]}));
        done.push(format!("команда при событии {event}: {}", remind_command()));
    }
    if !done.is_empty() {
        write_settings(path, &settings)?;
    }
    Ok(done)
}

/// Takes the reminder hook out of his settings, and nothing else.
pub fn remind_off(path: &Path) -> Result<Vec<String>, String> {
    let mut settings = read_settings(path)?;
    let mut done = Vec::new();
    for event in REMIND_AT {
        if !has_remind_at(&settings, event) {
            continue;
        }
        let hooks = settings.get_mut("hooks").and_then(Value::as_object_mut).expect("hooks");
        let mut kept = Vec::new();
        for mut group in hooks[event].as_array().cloned().unwrap_or_default() {
            if let Some(handlers) = group.get("hooks").and_then(Value::as_array).cloned() {
                let left: Vec<Value> = handlers.into_iter().filter(|handler| !is_remind_hook(handler)).collect();
                if left.is_empty() {
                    continue;
                }
                group["hooks"] = Value::Array(left);
            }
            kept.push(group);
        }
        if kept.is_empty() {
            hooks.shift_remove(event);
        } else {
            hooks.insert(event.into(), Value::Array(kept));
        }
        if hooks.is_empty() {
            settings.shift_remove("hooks");
        }
        done.push(format!("команда при событии {event} убрана"));
    }
    if !done.is_empty() {
        write_settings(path, &settings)?;
    }
    Ok(done)
}

/// The file he opens to put the reminder in with his own hand.
pub fn remind_launcher(home: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = home.join("remind.command");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let text = format!(
        "#!/bin/bash\n# Opened by the reviewer's own hand: puts the reminder of unread answers into his settings of Claude Code.\n{} install --remind\necho\necho 'Это окно можно закрыть.'\n",
        program()
    );
    let _ = std::fs::write(&path, text);
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
    path
}

/// A file he opens to switch it on with his own hand: the link of an action in the window opens it in his terminal.
pub fn launcher(home: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = home.join("window-ok.command");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let text = format!(
        "#!/bin/bash\n# Opened by the reviewer's own hand: puts the hook and the rule of `hitl note` into his settings of Claude Code.\n{} install --window-ok\necho\necho 'Это окно можно закрыть.'\n",
        program()
    );
    let _ = std::fs::write(&path, text);
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static NUMBER: AtomicU32 = AtomicU32::new(0);

    fn made_up_settings(text: Option<&str>) -> (PathBuf, PathBuf) {
        let folder = std::fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("conveyor-install-settings-{}-{}", std::process::id(), NUMBER.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("settings.json");
        if let Some(text) = text {
            std::fs::write(&path, text).unwrap();
        }
        (folder, path)
    }

    #[test]
    fn the_reminder_goes_in_beside_his_hooks_and_comes_out_alone() {
        let (folder, path) = made_up_settings(Some(r#"{"hooks": {"UserPromptSubmit": [{"hooks": [{"type": "command", "command": "mine"}]}], "PostToolUse": [{"matcher": "Edit", "hooks": [{"type": "command", "command": "fmt"}]}]}}"#));
        assert!(!remind_state(&path));
        assert_eq!(remind_on(&path).unwrap().len(), 2);
        assert!(remind_state(&path));
        assert!(remind_on(&path).unwrap().is_empty(), "again changes nothing");
        let settings = read_settings(&path).unwrap();
        assert_eq!(settings["hooks"]["UserPromptSubmit"][0]["hooks"][0]["command"], "mine", "his own hook stays first");
        assert_eq!(settings["hooks"]["UserPromptSubmit"].as_array().unwrap().len(), 2);
        assert_eq!(settings["hooks"]["SessionStart"][0]["hooks"][0]["timeout"], 5);
        assert!(kept_copy(&path).exists());
        assert_eq!(remind_off(&path).unwrap().len(), 2);
        let after = read_settings(&path).unwrap();
        assert!(!remind_state(&path) && !after["hooks"].as_object().unwrap().contains_key("SessionStart"));
        assert_eq!(after["hooks"]["UserPromptSubmit"].as_array().unwrap().len(), 1);
        assert_eq!(after["hooks"]["PostToolUse"][0]["matcher"], "Edit");
        // A hooks section that is not a list of groups is left as it is.
        let (_other, wrong) = made_up_settings(Some(r#"{"hooks": {"SessionStart": 3}}"#));
        assert!(remind_on(&wrong).unwrap_err().contains("раздел hooks записан не так"));
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_word_is_quoted_as_a_shell_reads_it() {
        assert_eq!(quote("/Users/a/Code/scripts/hitl"), "/Users/a/Code/scripts/hitl");
        assert_eq!(quote("/Users/a b/hitl"), "'/Users/a b/hitl'");
        assert_eq!(quote("it's"), "'it'\"'\"'s'");
    }

    #[test]
    fn our_hook_is_known_by_its_command() {
        for command in ["/x/hitl note", "hitl note", "'/x y/hitl' note", " /a/hitl note "] {
            assert!(is_our_hook(&json!({"command": command})), "{command}");
        }
        for command in ["/x/notconveyor note", "/x/hitl notes", "other note"] {
            assert!(!is_our_hook(&json!({"command": command})), "{command}");
        }
        assert!(!is_our_hook(&json!("hitl note")));
    }

    #[test]
    fn the_hook_and_the_rule_go_in_once_and_out_again_with_his_own_left() {
        let (folder, path) = made_up_settings(Some(r#"{"model": "opus", "hooks": {"PostToolUse": [{"matcher": "Edit", "hooks": [{"type": "command", "command": "fmt"}]}]}}"#));
        let done = switch_on(&path).unwrap();
        assert_eq!(done.len(), 2, "{done:?}");
        assert_eq!(state(&path), (true, true));
        assert!(kept_copy(&path).is_file(), "his settings are kept beside them once");
        assert!(switch_on(&path).unwrap().is_empty(), "nothing to change the second time");
        let settings = read_settings(&path).unwrap();
        assert_eq!(settings["autoMode"]["allow"], json!(["$defaults", RULE]));
        let done = switch_off(&path).unwrap();
        assert_eq!(done.len(), 2, "{done:?}");
        assert_eq!(state(&path), (false, false));
        let settings = read_settings(&path).unwrap();
        assert_eq!(settings["model"], "opus");
        assert_eq!(settings["hooks"]["PostToolUse"][0]["matcher"], "Edit", "his own hook stays");
        assert!(!settings.contains_key("autoMode"));
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_file_that_is_not_settings_is_left_alone() {
        let (folder, path) = made_up_settings(Some("not json"));
        let problem = switch_on(&path).unwrap_err();
        assert!(problem.contains("не читается как настройки Claude Code") && problem.ends_with("ничего не менял."), "{problem}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "not json");
        let (other, list) = made_up_settings(Some("[1]"));
        assert!(switch_on(&list).unwrap_err().contains("не похож на настройки Claude Code"));
        let (third, wrong) = made_up_settings(Some(r#"{"hooks": 3}"#));
        assert!(switch_on(&wrong).unwrap_err().contains("раздел hooks записан не так"));
        for folder in [folder, other, third] {
            let _ = std::fs::remove_dir_all(folder);
        }
    }
}
