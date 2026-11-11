//! Opens the originating session from the HITL window.

mod terminal;

use hitl::common::{self, Env};

/// The address of the chat this environment is of, "" for a chat that has none.
fn link(environment: &Env) -> String {
    // T3 Code's provider session is a different chat. Desktop has no external thread link.
    if environment.contains_key("T3CODE_THREAD_ID") {
        return String::new();
    }
    if let Some(thread) = environment.get("CODEX_THREAD_ID").filter(|thread| common::plain_id(thread)) {
        return format!("conveyor-chat://Codex/{thread}");
    }
    // A chat of OpenCode is its TUI as well, `opencode --session ID` (a subagent's session shows its parent's).
    let (session, _) = hitl::tell::opencode_target(environment);
    if common::plain_id(&session) {
        return format!("conveyor-chat://OpenCode/{session}");
    }
    for kept in common::claude_sessions(environment) {
        for remote in common::list_of(&kept, "bridgeSessionIds") {
            let Some(remote) = remote.as_str() else { continue };
            let id = remote.strip_prefix("cse_").or_else(|| remote.strip_prefix("session_"));
            if let Some(id) = id.filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric())) {
                return format!("claude://code/session_{id}");
            }
        }
    }
    String::new()
}

/// Codex uses "vscode" for several app-server clients. Only the Desktop originator identifies its own chats.
fn is_codex_desktop_thread(environment: &Env, thread: &str) -> bool {
    if !common::plain_id(thread) {
        return false;
    }
    let Ok(connection) = common::read_only(&common::codex_state(environment), 1.0) else {
        return false;
    };
    connection.query_row(
        "select exists(select 1 from threads where id = ? and source = 'vscode' and originator in ('Codex Desktop', 'codex_work_desktop'))",
        [thread],
        |row| row.get(0),
    ).unwrap_or(false)
}

/// The two routes used by existing chat-button settings and queued HITL links.
pub fn dispatch(arguments: &[String]) -> Option<i32> {
    match arguments {
        [action] if action == "link" => {
            let found = link(&common::env());
            if !found.is_empty() {
                println!("{found}");
            }
            Some(0)
        }
        [action, app, id] if action == "reveal" => Some(reveal(app, id, &common::env())),
        _ => None,
    }
}

fn reveal(app: &str, id: &str, environment: &Env) -> i32 {
    if !common::plain_id(id) {
        hitl_host_core::say("Неверный идентификатор чата.");
        return 1;
    }
    if app == "Codex" && is_codex_desktop_thread(environment, id) {
        let link = format!("codex://threads/{id}");
        return match std::process::Command::new("open").args(["-b", "com.openai.codex", &link]).status() {
            Ok(status) if status.success() => 0,
            Ok(_) => 1,
            Err(error) => {
                hitl_host_core::say(&format!("Не удалось открыть Codex Desktop: {error}"));
                1
            }
        };
    }
    if hitl_host_core::paths::state("helpers").join(id).join("record.json").is_file() {
        hitl_host_core::say("Это ссылка на сохранённого помощника Conveyor.");
        return 1;
    }
    match terminal::show_tui(app, id) {
        Ok(()) => 0,
        Err(error) => {
            hitl_host_core::say(&error);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_button_uses_native_session_ids_without_confusing_t3_for_its_provider() {
        let mut environment = Env::from([("CODEX_THREAD_ID".into(), "native-thread".into())]);
        assert_eq!(link(&environment), "conveyor-chat://Codex/native-thread");
        environment.insert("T3CODE_THREAD_ID".into(), "t3-thread".into());
        assert_eq!(link(&environment), "");
        assert!(dispatch(&["send".into(), "Codex".into(), "native-thread".into(), "message".into()]).is_none());
    }
}
