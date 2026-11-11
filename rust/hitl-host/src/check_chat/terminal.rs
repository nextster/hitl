//! Opens a native terminal session for the HITL button.

use std::path::PathBuf;
use std::process::Command;

fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The script that brings forward the window of iTerm whose session has this tty; says "found" or "none".
fn focus_script(tty: &str) -> String {
    format!(
        "tell application \"iTerm\"\n repeat with w in windows\n repeat with t in tabs of w\n repeat with s in sessions of t\n \
         if tty of s is {} then\n activate\n select w\n tell t to select\n tell s to select\n return \"found\"\n end if\n \
         end repeat\n end repeat\n end repeat\n return \"none\"\nend tell",
        quoted(tty))
}

/// The script that opens a new tab of iTerm running this program, in the window iTerm has in front; a window when it has none.
fn tab_script(program: &str) -> String {
    format!(
        "tell application \"iTerm\"\n activate\n if (count of windows) is 0 then\n create window with default profile command {0}\n \
         else\n tell current window to create tab with default profile command {0}\n end if\n return \"new\"\nend tell",
        quoted(program))
}

fn osascript(script: &str) -> Result<String, String> {
    let program = std::env::var("CONVEYOR_OSASCRIPT").ok().filter(|given| !given.is_empty()).unwrap_or_else(|| "osascript".into());
    let done = Command::new(&program).args(["-e", script]).output().map_err(|e| format!("{program}: {e}"))?;
    if done.status.success() {
        Ok(String::from_utf8_lossy(&done.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&done.stderr).trim().to_string())
    }
}

/// What starts the TUI of a chat of each app: `codex resume ID`, `opencode --session ID`.
fn tui_words(app: &str, id: &str) -> Option<(&'static str, Vec<String>)> {
    match app {
        "Codex" => Some(("codex", vec!["resume".into(), id.into()])),
        "OpenCode" => Some(("opencode", vec!["--session".into(), id.into()])),
        _ => None,
    }
}

/// The terminal the TUI of this chat runs in, from `ps -axo pid=,tty=,args=`: its tty as a device, or None.
fn tui_tty(ps: &str, app: &str, id: &str) -> Option<String> {
    let (program, words) = tui_words(app, id)?;
    ps.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let (_pid, tty) = (fields.next()?, fields.next()?);
        let args: Vec<&str> = fields.collect();
        let named = args.first().is_some_and(|first| first.rsplit('/').next() == Some(program));
        (named && words.iter().all(|word| args.contains(&word.as_str())) && tty != "??" && tty != "?").then(|| format!("/dev/{tty}"))
    })
}

/// The folder a chat of Codex or OpenCode works in, from the app's own database, if it is there.
fn folder_of(app: &str, id: &str) -> Option<String> {
    let environment = hitl_host_core::env();
    let (database, queries): (PathBuf, &[&str]) = match app {
        "Codex" => (hitl::common::codex_state(&environment), &["select cwd from threads where id = ?"]),
        _ => (hitl::common::opencode_state(&environment),
              &["select directory from session_v2 where id = ?", "select directory from session where id = ?"]),
    };
    if !database.exists() {
        return None;
    }
    let connection = hitl::common::read_only(&database, 1.0).ok()?;
    queries.iter().find_map(|query| connection.query_row(query, [id], |row| row.get::<_, Option<String>>(0)).ok().flatten())
        .filter(|folder| !folder.is_empty() && std::path::Path::new(folder).is_dir())
}

fn ps_output() -> String {
    let program = std::env::var("CONVEYOR_PS").ok().filter(|given| !given.is_empty()).unwrap_or_else(|| "ps".into());
    Command::new(program).args(["-axo", "pid=,tty=,args="]).output().map(|done| String::from_utf8_lossy(&done.stdout).into_owned()).unwrap_or_default()
}

/// The TUI of a chat of Codex or OpenCode in iTerm: the tab that runs it comes forward, else a new tab runs it.
pub(super) fn show_tui(app: &str, id: &str) -> Result<(), String> {
    let (program, words) = tui_words(app, id).ok_or_else(|| format!("у чата {app} нет программы для iTerm"))?;
    if let Some(tty) = tui_tty(&ps_output(), app, id) {
        if osascript(&focus_script(&tty))? == "found" {
            return Ok(());
        }
    }
    let folder = hitl_host_core::paths::state("tui");
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let file = folder.join(format!("{}-{id}.sh", app.to_lowercase()));
    let go = folder_of(app, id).map(|place| format!("cd '{}'\n", place.replace('\'', "'\\''"))).unwrap_or_default();
    let body = format!(
        "#!/bin/zsh -l\nprintf '\\033]0;%s\\007' '{app} {id}'\n{go}{program} {} || {{ printf '\\n'; read -rk1 '?Нажми любую клавишу'; }}\n",
        words.iter().map(|word| format!("'{word}'")).collect::<Vec<_>>().join(" "));
    std::fs::write(&file, body).map_err(|e| e.to_string())?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    osascript(&tab_script(&file.display().to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tui_of_a_chat_is_found_by_its_resume_and_its_terminal() {
        let ps = "  101 ttys003 /usr/local/bin/zsh -l\n  202 ttys004 /opt/homebrew/bin/codex resume 019a8f2e-7c41\n  303 ?? codex resume 019a8f2e-7c99\n  404 ttys005 node codex resume 019a8f2e-7c41\n";
        assert_eq!(tui_tty(ps, "Codex", "019a8f2e-7c41").as_deref(), Some("/dev/ttys004"));
        assert_eq!(tui_tty(ps, "Codex", "019a8f2e-7c99"), None); // no terminal: not a tab of iTerm
        assert_eq!(tui_tty(ps, "Codex", "019a"), None);
        let oc = "  1 ttys007 /opt/homebrew/bin/opencode --session ses_1abc\n  2 ttys008 opencode run --session ses_2\n";
        assert_eq!(tui_tty(oc, "OpenCode", "ses_1abc").as_deref(), Some("/dev/ttys007"));
        assert_eq!(tui_tty(oc, "OpenCode", "ses_9"), None);
        assert_eq!(tui_tty(oc, "Claude", "ses_1abc"), None);
    }

    #[test]
    fn the_scripts_name_the_tty_and_the_program_and_quote_them() {
        let focus = focus_script("/dev/ttys012");
        assert!(focus.contains("tty of s is \"/dev/ttys012\"") && focus.contains("return \"none\""));
        let new = tab_script("/a b/\"c\"");
        assert!(new.contains("create tab with default profile command \"/a b/\\\"c\\\"\""), "{new}");
    }
}
