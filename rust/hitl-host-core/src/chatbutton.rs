//! The setting of the «Чат» button on this Mac (`hitl::chatbutton` reads it): which command finds the address of a chat,
//! and which opens a chat that lives in a terminal. hitl names no program; what is written here is hitl's.

use hitl::common::Env;

/// What the setting of the «Чат» button says on this Mac: the address is asked of `hitl chat link`, and an address
/// of a chat in a terminal is opened by `hitl chat reveal`.
pub fn setting() -> serde_json::Value {
    let program = crate::command(&[]);
    setting_for(&program)
}

fn setting_for(program: &str) -> serde_json::Value {
    serde_json::json!({"link": [program, "chat", "link"], "open": {"conveyor-chat": [program, "chat", "reveal", "{host}", "{path}"]}})
}

/// A default from a former Conveyor install can be upgraded without changing custom settings.
pub fn needs_update(environment: &Env) -> bool {
    let file = hitl::chatbutton::settings_file(environment);
    if !file.exists() {
        return true;
    }
    let Some(home) = environment.get("HOME") else { return false };
    let old = std::path::Path::new(home).join("Code/scripts/conveyor");
    let expected = setting_for(&old.display().to_string());
    crate::read_json(&file).as_ref() == Some(&expected)
}

/// Install the default or upgrade a recognized legacy default. Leave custom settings in place. True when it was written. `hitl install` and every `hitl check` do this.
pub fn ensure_setting(environment: &Env) -> bool {
    let file = hitl::chatbutton::settings_file(environment);
    if !needs_update(environment) {
        return false;
    }
    let Some(folder) = file.parent() else { return false };
    let temporary = folder.join(format!(".chat-button.json.{}", std::process::id()));
    let written = std::fs::create_dir_all(folder)
        .and_then(|_| std::fs::write(&temporary, format!("{}\n", serde_json::to_string_pretty(&setting()).unwrap_or_default())))
        .and_then(|_| std::fs::rename(&temporary, &file));
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    written.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Env {
        pairs.iter().map(|(name, value)| (name.to_string(), value.to_string())).collect()
    }

    #[test]
    fn the_setting_is_written_once_and_a_changed_one_is_left_alone() {
        let folder = std::env::temp_dir().join(format!("conveyor-chatlink-{}", std::process::id()));
        let file = folder.join("deeper").join("chat-button.json");
        let environment = env(&[("CONVEYOR_CHAT_BUTTON", file.to_str().unwrap())]);
        assert!(ensure_setting(&environment));
        let written: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(written["link"][1], "chat");
        assert_eq!(written["open"]["conveyor-chat"][3], "{host}");
        // What hitl reads from it is what the window will run.
        assert!(hitl::chatbutton::needs_command(&environment, "conveyor-chat://Codex/abc"));
        std::fs::write(&file, "{}").unwrap();
        assert!(!ensure_setting(&environment));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "{}");
        let _ = std::fs::remove_dir_all(&folder);
        // A place that cannot be written is no failure.
        assert!(!ensure_setting(&env(&[("CONVEYOR_CHAT_BUTTON", "/nonexistent-folder/x/chat-button.json")])));
    }
}
