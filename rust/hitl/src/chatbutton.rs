//! The «Чат» button, as a setting. hitl names no program: how to find the link of the chat a check is queued from, and
//! what to run when the button is pressed, are in a file that the setup of the tool writes, `chat-button.json`
//! beside the rest of its state (`CONVEYOR_CHAT_BUTTON` names another file):
//!
//! ```json
//! {"link": ["/path/to/command", "chat", "link"],
//!  "open": {"my-chat": ["/path/to/command", "chat", "show", "{host}", "{path}"]}}
//! ```
//!
//! `link` is run when a check is queued, with the environment of the chat, and prints one address on its first line.
//! `open` says, by the scheme of an address, what the window runs when the button is pressed; `{link}`, `{scheme}`,
//! `{host}` and `{path}` (the two halves of `scheme://host/path`) are put into its words, which are never given to a
//! shell. An address whose scheme `open` does not list is opened as any address is. No file is no button.

use crate::common::{self, Env};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

/// How long the command that finds the link may take: a check is never held back by it.
const LINK_SECONDS: f64 = 3.0;
const LINK_LONGEST: usize = 1000;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Settings {
    pub link: Vec<String>,
    pub open: BTreeMap<String, Vec<String>>,
}

/// The file the settings are in.
pub fn settings_file(environment: &Env) -> PathBuf {
    match environment.get("CONVEYOR_CHAT_BUTTON").filter(|named| !named.is_empty()) {
        Some(named) => PathBuf::from(named),
        None => common::home().join("chat-button.json"),
    }
}

fn words(value: Option<&Value>) -> Vec<String> {
    let listed: Vec<String> = value.and_then(Value::as_array).map(|list| list.iter().filter_map(|word| word.as_str().map(str::to_string)).collect()).unwrap_or_default();
    if listed.iter().any(|word| word.is_empty()) { Vec::new() } else { listed }
}

/// The settings a text says; nothing for a text that is no settings.
pub fn parse(text: &str) -> Settings {
    let Ok(Value::Object(file)) = serde_json::from_str::<Value>(text) else { return Settings::default() };
    let mut settings = Settings { link: words(file.get("link")), open: BTreeMap::new() };
    if let Some(Value::Object(open)) = file.get("open") {
        for (scheme, command) in open {
            let command = words(Some(command));
            if !command.is_empty() {
                settings.open.insert(scheme.to_lowercase(), command);
            }
        }
    }
    settings
}

/// The settings of this machine; none when the file is not there or is not readable.
pub fn load(environment: &Env) -> Settings {
    std::fs::read_to_string(settings_file(environment)).map(|text| parse(&text)).unwrap_or_default()
}

/// The scheme of an address, lowercase: what stands before the first colon, when that is a scheme.
pub fn scheme_of(link: &str) -> Option<String> {
    let scheme = link.trim().split(':').next()?;
    let proper = !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        && link.trim().len() > scheme.len() + 1;
    proper.then(|| scheme.to_lowercase())
}

/// What the window runs for an address, when the settings say so; None when the address opens as any address does.
pub fn open_command(settings: &Settings, link: &str) -> Option<Vec<String>> {
    let link = link.trim();
    let scheme = scheme_of(link)?;
    let command = settings.open.get(&scheme)?;
    let rest = link[scheme.len() + 1..].trim_start_matches("//");
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    Some(command.iter().map(|word| word.replace("{link}", link).replace("{scheme}", &scheme).replace("{host}", host).replace("{path}", path)).collect())
}

/// Whether an address needs a command of this Mac, so that a phone has nothing to open it with.
pub fn needs_command(environment: &Env, link: &str) -> bool {
    open_command(&load(environment), link).is_some()
}

/// An address a command printed: one word, no longer than a link may be.
fn proper(printed: &str) -> String {
    let line = printed.lines().next().unwrap_or("").trim();
    if line.is_empty() || line.chars().count() > LINK_LONGEST || line.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return String::new();
    }
    line.to_string()
}

/// The address that opens the chat this process runs in, "" for a chat that has none. `CONVEYOR_CHAT_LINK` names it
/// by hand; a manual name alone (`CONVEYOR_CHAT`) supplies no identity. The command in the settings resolves the address with
/// the environment of the chat, and an answer that does not come in time, or is no address, is no button.
pub fn link_of(environment: &Env) -> String {
    if let Some(named) = environment.get("CONVEYOR_CHAT_LINK") {
        return named.trim().to_string();
    }
    if environment.contains_key("CONVEYOR_CHAT") {
        return String::new();
    }
    let settings = load(environment);
    let Some((program, arguments)) = settings.link.split_first() else { return String::new() };
    let mut command = Command::new(program);
    command.args(arguments).env_clear().envs(environment);
    match common::run(&mut command, None, LINK_SECONDS) {
        Some(done) if done.code == 0 => proper(&done.stdout),
        _ => String::new(),
    }
}

#[cfg(test)]
#[path = "chatbutton_tests.rs"]
mod tests;
