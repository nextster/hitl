//! `hitl install`: the links, the skills and the settings this project needs on a Mac, and what is missing.
//!
//! Moved from `install.py` (see `docs/rust-move.md`). Every place it touches is named once, in `Places`, which the
//! command builds from the environment (`HOME`, `CONVEYOR_HOME`, `CONVEYOR_CLAUDE_SETTINGS`) and its tests from
//! made-up folders: nothing here reaches for his real home on its own.

#[path = "install_settings.rs"]
pub mod settings;

use hitl_host_core::process::{Options, run};
use hitl_host_core::{EXIT_REFUSED, dumps_line, read_json, say, truthy};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

const USAGE: &str = "Puts in place what this project needs on a Mac, and says what is missing.

Usage:
  hitl install [--dry-run]
  hitl install --window-ok | --window-ok-off | --remind | --remind-off

What it does, and does again safely:
  - ~/Code/scripts/hitl links to this project's command, and so does
    ~/Code/scripts/ios-run-gate: ios-run.sh looks for its gate under that name.
    Links to commands this project no longer has are removed.
  - ~/.agents/skills/<name> links to each folder of skills/ here; links to skills
    that are no longer here are removed.
  - ~/.zshenv names the builder for every shell: IOS_RUN_COMMAND.
  - What the tools kept in ~/.agent-checks and in Xcode's DerivedData before there
    was one home moves into ~/.conveyor, with its history, while nothing waits there.
Then it says what it found: the tools it calls, the builder, the links.

  --dry-run   Say what would be done, change nothing.

  --window-ok      Lets Claude Code's auto mode hear what he answered in the check window.
                   Puts two things into his settings of Claude Code (~/.claude/settings.json):
                   the command `hitl note`, run after every Bash call, and a rule of his
                   own for auto mode, that what such a note reports is his own answer. What his
                   settings were is kept beside them once, as settings.json.before-conveyor.
                   His to run, at a terminal: called without one, as a chat calls it, it changes nothing, writes
                   ~/.conveyor/window-ok.command, which does it when he opens it, and names
                   that file for the chat to give him as the --link of an action.
  --window-ok-off  Takes the two out again, and nothing else. His to run as well.

  --remind         Makes sure a chat of Claude Code takes the answer he gave it in the window. Puts the
                   command `hitl _remind` into his settings of Claude Code (~/.claude/settings.json),
                   at SessionStart and at UserPromptSubmit, beside his own hooks: a chat that starts or
                   is written to is told, in one line, of the answers it has not read. The wake of the
                   command that waits can be lost; this cannot. His to run, at a terminal, like
                   --window-ok: a chat gets ~/.conveyor/remind.command to give him instead.
  --remind-off     Takes the command out again, and nothing else.
";

const BUILDER_LINE: &str = "export IOS_RUN_COMMAND=\"$HOME/Code/scripts/ios-run.sh\"";

/// Every place this command touches.
pub struct Places {
    pub scripts: PathBuf,
    pub skills: PathBuf,
    pub zshenv: PathBuf,
    pub old_window_home: PathBuf,
    pub old_phone_home: PathBuf,
    pub claude_settings: PathBuf,
    /// The project (`root`), its command, and the one home of the state.
    pub root: PathBuf,
    pub entry: PathBuf,
    pub home: PathBuf,
}

impl Places {
    pub fn from_environment() -> Places {
        let user = |path: &str| hitl_host_core::paths::expand(path);
        Places {
            scripts: user("~/Code/scripts"),
            skills: user("~/.agents/skills"),
            zshenv: user("~/.zshenv"),
            old_window_home: user("~/.agent-checks"),
            old_phone_home: user("~/Library/Developer/Xcode/DerivedData/.ios-run-device"),
            claude_settings: settings::settings_file(),
            root: hitl_host_core::root(),
            entry: hitl_host_core::entry(),
            home: hitl_host_core::home(),
        }
    }
}

struct Steps {
    dry: bool,
    missing: Vec<String>,
}

impl Steps {
    fn do_it(&mut self, what: &str, action: impl FnOnce() -> std::io::Result<()>) {
        say(&format!("{}{what}", if self.dry { "would: " } else { "==> " }));
        if !self.dry {
            if let Err(error) = action() {
                self.missing.push(format!("{what}: {error}"));
            }
        }
    }

    fn lacks(&mut self, what: String) {
        self.missing.push(what);
    }
}

/// A path's components, which is how Python's `sorted` orders paths.
fn by_parts(paths: &mut [PathBuf]) {
    paths.sort_by(|a, b| a.components().cmp(b.components()));
}

fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path).map(|meta| meta.file_type().is_symlink()).unwrap_or(false)
}

/// A link into this project, alive or left dangling by a file that has moved.
fn points_here(places: &Places, link: &Path) -> bool {
    if !is_symlink(link) {
        return false;
    }
    let Ok(read) = std::fs::read_link(link) else { return false };
    let target = if read.is_absolute() { read } else { link.parent().unwrap_or(Path::new("")).join(read) };
    target.starts_with(&places.root)
}

fn same_place(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn link(steps: &mut Steps, path: &Path, target: &Path) {
    if is_symlink(path) && same_place(path, target) {
        return;
    }
    if path.exists() && !is_symlink(path) {
        steps.lacks(format!("{} is a file of its own, not a link to {}: look at it and remove it by hand.", path.display(), target.display()));
        return;
    }
    let (place, to) = (path.to_path_buf(), target.to_path_buf());
    steps.do_it(&format!("link {} -> {}", path.display(), target.display()), move || {
        if let Some(parent) = place.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let name = place.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default();
        let temporary = place.with_file_name(format!(".{name}.new"));
        let _ = std::fs::remove_file(&temporary);
        std::os::unix::fs::symlink(&to, &temporary)?;
        std::fs::rename(&temporary, &place)
    });
}

fn children(folder: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(folder).map(|entries| entries.flatten().map(|entry| entry.path()).collect()).unwrap_or_default()
}

fn links(places: &Places, steps: &mut Steps) {
    for name in ["hitl", "ios-run-gate"] {
        link(steps, &places.scripts.join(name), &places.entry);
    }
    // What chats called before there was one command, and the files those commands were made of.
    let mut old = children(&places.scripts);
    let deeper: Vec<PathBuf> = old.iter().filter(|path| path.is_dir()).flat_map(|folder| children(folder)).collect();
    old.extend(deeper);
    by_parts(&mut old);
    for path in old {
        if points_here(places, &path) && !path.exists() {
            let gone = path.clone();
            steps.do_it(&format!("remove the link {}: what it led to is gone", path.display()), move || std::fs::remove_file(gone));
        }
    }
    let mut skills: Vec<PathBuf> =
        children(&places.root.join("skills")).into_iter().filter(|folder| folder.join("SKILL.md").is_file()).collect();
    by_parts(&mut skills);
    for folder in skills {
        let name = folder.file_name().map(PathBuf::from).unwrap_or_default();
        link(steps, &places.skills.join(name), &folder);
    }
    if places.skills.is_dir() {
        let mut old = children(&places.skills);
        by_parts(&mut old);
        for path in old {
            if points_here(places, &path) && !path.exists() {
                let gone = path.clone();
                steps.do_it(&format!("remove the link {}: the skill it led to is gone", path.display()), move || std::fs::remove_file(gone));
            }
        }
    }
}

fn zshenv_line(places: &Places, steps: &mut Steps) {
    let text = std::fs::read_to_string(&places.zshenv).unwrap_or_default();
    if text.contains("IOS_RUN_COMMAND") {
        return;
    }
    let path = places.zshenv.clone();
    steps.do_it(&format!("add to {}: {BUILDER_LINE}", places.zshenv.display()), move || {
        use std::io::Write;
        let mut handle = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
        let lead = if text.ends_with('\n') || text.is_empty() { "" } else { "\n" };
        handle.write_all(format!("{lead}{BUILDER_LINE}\n").as_bytes())
    });
}

/// Remove only the status line previously installed by Conveyor.
fn remove_old_status_line(places: &Places, steps: &mut Steps) {
    let path = &places.claude_settings;
    if !path.is_file() {
        return;
    }
    let owned = json!({"type": "command", "command": format!("{} statusline", settings::program()), "padding": 0});
    let kept = match settings::read_settings(path) {
        Ok(kept) => kept,
        Err(problem) => return steps.lacks(problem),
    };
    if kept.get("statusLine") != Some(&owned) {
        return;
    }
    let target = path.clone();
    steps.do_it(&format!("remove the retired Conveyor status line from {}", path.display()), move || {
        let mut now = settings::read_settings(&target).map_err(std::io::Error::other)?;
        if now.get("statusLine") == Some(&owned) {
            now.remove("statusLine");
            settings::write_settings(&target, &now).map_err(std::io::Error::other)?;
        }
        Ok(())
    });
}

/// The setting of the «Чат» button (`chat-button.json` in the state folder): how a check finds the address of its chat
/// and what the window runs for it. One that is there, a changed one too, is left as it is.
fn chat_button(places: &Places, steps: &mut Steps) {
    let mut environment = hitl::common::env();
    environment.entry("CONVEYOR_CHAT_BUTTON".into()).or_insert_with(|| places.home.join("chat-button.json").display().to_string());
    let file = hitl::chatbutton::settings_file(&environment);
    if !hitl_host_core::chatbutton::needs_update(&environment) {
        return;
    }
    steps.do_it(&format!("write {}: the «Чат» button of the check window finds and opens its chat by it", file.display()), move || {
        if hitl_host_core::chatbutton::ensure_setting(&environment) { Ok(()) } else { Err(std::io::Error::other("не записалось")) }
    });
}

fn lines_of(path: &Path) -> Vec<Map<String, Value>> {
    let Ok(raw) = std::fs::read_to_string(path) else { return Vec::new() };
    raw.lines()
        .filter_map(|line| match serde_json::from_str::<Value>(line) {
            Ok(Value::Object(entry)) => Some(entry),
            _ => None,
        })
        .collect()
}

/// What still waits where the tools kept their state before: it is not moved from under a waiting command.
fn waiting_in_the_old_places(places: &Places) -> Vec<String> {
    let mut found = Vec::new();
    let queue = places.old_window_home.join("queue");
    let waits = children(&queue).iter().any(|path| path.extension().is_some_and(|extension| extension == "json"));
    if queue.is_dir() && waits {
        found.push(format!("проверки в окне на Маке ({})", queue.display()));
    }
    if places.old_phone_home.is_dir() {
        let mut folders = children(&places.old_phone_home);
        folders.sort();
        for folder in folders {
            let state_file = folder.join("state.json");
            if !state_file.is_file() {
                continue;
            }
            let state = read_json(&state_file).unwrap_or(Value::Null);
            if ["active", "queue", "probe", "running", "waiting"].iter().any(|key| truthy(state.get(key))) {
                let name = folder.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default();
                found.push(format!("очередь {name} ({})", state_file.display()));
            }
        }
    }
    found
}

/// Moves a file or a folder into its place; a folder that is there already takes the children one by one, and a
/// file that is there already is left where it was.
fn carry(source: &Path, destination: &Path) -> std::io::Result<()> {
    if std::fs::symlink_metadata(source).is_err() {
        return Ok(());
    }
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if source.is_dir() && destination.is_dir() {
        let mut names = children(source);
        names.sort();
        for child in names {
            if let Some(name) = child.file_name() {
                carry(&child, &destination.join(name))?;
            }
        }
        std::fs::remove_dir(source)
    } else if std::fs::symlink_metadata(destination).is_err() {
        move_across(source, destination)
    } else {
        Ok(())
    }
}

fn move_across(source: &Path, destination: &Path) -> std::io::Result<()> {
    if std::fs::rename(source, destination).is_ok() {
        return Ok(());
    }
    // Another volume: copy, then remove.
    let (from, to) = (source.display().to_string(), destination.display().to_string());
    let copied = run("cp".to_string(), &["-R".to_string(), "-p".to_string(), from, to], &Options::new("install.carry"))?;
    if !copied.success() {
        return Err(std::io::Error::other(copied.err()));
    }
    if source.is_dir() { std::fs::remove_dir_all(source) } else { std::fs::remove_file(source) }
}

fn number_of(entry: &Map<String, Value>, key: &str) -> f64 {
    entry.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn move_state(places: &Places, steps: &mut Steps) {
    if !places.old_window_home.exists() && !places.old_phone_home.exists() {
        return;
    }
    let waiting = waiting_in_the_old_places(places);
    if !waiting.is_empty() {
        return steps.lacks(format!(
            "состояние не перенесено в ~/.conveyor: ещё ждут {}. Повтори hitl install, когда там станет пусто.",
            waiting.join("; ")
        ));
    }
    let (old_window, old_phone, home) = (places.old_window_home.clone(), places.old_phone_home.clone(), places.home.clone());
    let what = format!(
        "move the state from {} and {} into {}, with the history",
        old_window.display(),
        old_phone.display(),
        home.display()
    );
    steps.do_it(&what, move || {
        std::fs::create_dir_all(&home)?;
        let history_file = home.join("history.jsonl");
        let history = lines_of(&history_file);
        let key = |entry: &Map<String, Value>| (entry.get("id").cloned(), entry.get("finishedAt").cloned());
        let known: Vec<_> = history.iter().map(key).collect();
        let mut added: Vec<Map<String, Value>> = Vec::new();
        for mut entry in lines_of(&old_window.join("history.jsonl")) {
            entry.insert("screen".into(), json!("mac"));
            added.push(entry);
        }
        let mut phones = if old_phone.is_dir() { children(&old_phone) } else { Vec::new() };
        phones.sort();
        let is_turns = |folder: &Path| {
            let name = folder.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default();
            name == "mac-tests" || name.starts_with("landing-")
        };
        for folder in &phones {
            if is_turns(folder) {
                continue;
            }
            let udid = folder.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default();
            for mut entry in lines_of(&folder.join("history.jsonl")) {
                entry.insert("screen".into(), json!("iphone"));
                entry.insert("udid".into(), json!(udid));
                added.push(entry);
            }
        }
        let mut merged = history;
        merged.extend(added.into_iter().filter(|entry| !known.contains(&key(entry))));
        merged.sort_by(|a, b| number_of(a, "finishedAt").partial_cmp(&number_of(b, "finishedAt")).unwrap_or(std::cmp::Ordering::Equal));
        let temporary = home.join(".history.jsonl.new");
        let text: String = merged.into_iter().map(|entry| dumps_line(&Value::Object(entry)) + "\n").collect();
        std::fs::write(&temporary, text)?;
        std::fs::rename(&temporary, &history_file)?;
        for name in ["queue", "done", "shots"] {
            carry(&old_window.join(name), &home.join(name))?;
        }
        carry(&old_window.join("place.json"), &home.join("window").join("place.json"))?;
        for folder in &phones {
            let name = folder.file_name().map(PathBuf::from).unwrap_or_default();
            carry(folder, &home.join(if is_turns(folder) { "turns" } else { "iphone" }).join(name))?;
        }
        // What is left is the old window's program and logs: kept in one place, deleted by nobody.
        for (old, name) in [(&old_window, "agent-checks"), (&old_phone, "ios-run-device")] {
            if old.exists() {
                carry(old, &home.join("old").join(name))?;
            }
        }
        Ok(())
    });
}

fn resolve(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn report(places: &Places, steps: &mut Steps) {
    for (tool, why) in [
        ("swiftc", "the window on the Mac is compiled with it"),
        ("git", "every check remembers its branch"),
        ("xcrun", "the phone is reached through devicectl"),
    ] {
        if hitl_host_core::which(tool).is_none() {
            steps.lacks(format!("нет команды {tool}: {why}."));
        }
    }
    let builder = places.scripts.join("ios-run.sh");
    if !builder.is_file() {
        steps.lacks(format!("нет {}: hitl iphone собирает им.", builder.display()));
    } else if !String::from_utf8_lossy(&std::fs::read(&builder).unwrap_or_default()).contains("\"$SCRIPT_DIR/hitl\"") {
        steps.lacks(format!(
            "{} ещё зовёт очередь по старому имени: он должен передавать вызовы в hitl iphone.",
            builder.display()
        ));
    }
    for name in ["hitl", "ios-run-gate"] {
        let path = places.scripts.join(name);
        if !steps.dry && resolve(&path) != resolve(&places.entry) {
            steps.lacks(format!("{} не ведёт к {}.", path.display(), places.entry.display()));
        }
    }
    say(&format!(
        "==> hitl: {}; состояние: {}.",
        places.entry.display(),
        places.home.display()
    ));
}

/// Whether a person runs this at a terminal. A command a chat runs has no terminal, and that is the whole test:
/// the marks an agent leaves on its commands say nothing here. The terminal the window opens for him is started
/// by the window, which a chat's command started, so his own shell there carries that chat's marks: on
/// 2026-10-12 the file he opened refused him as a chat of Claude.
fn by_his_hand() -> bool {
    // SAFETY: isatty on the standard descriptors.
    unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 }
}

/// His settings of Claude Code get the hook and the rule of `hitl note`, or lose them: by his hand only. A chat
/// does not change the settings of its own protection, so a call from one changes nothing and hands the chat the
/// file he opens instead.
fn window_ok(places: &Places, off: bool, by_hand: bool) -> i32 {
    let option = if off { "--window-ok-off" } else { "--window-ok" };
    let path = &places.claude_settings;
    if !by_hand {
        let (hook, rule) = settings::state(path);
        let now = if hook && rule {
            "both are there now"
        } else if !(hook || rule) {
            "neither is there now"
        } else {
            "one of the two is there now"
        };
        say(&format!(
            "hitl install {option}: this is his to run, not a chat's. It changes his settings of Claude Code, the hook \
             `hitl note` and a rule of his own for auto mode ({now}), and a chat does not change the settings of its own \
             protection. Nothing was changed."
        ));
        if !off {
            say(&format!(
                "Give him the button: an action in the check window whose --link is {} . Opened by his \
                 hand, that file runs this command in his terminal. Say in the item what it does for him: after it, what he \
                 answers in the window is known to Claude Code's auto mode too.",
                settings::launcher(&places.home).display()
            ));
        }
        return EXIT_REFUSED;
    }
    let done = if off { settings::switch_off(path) } else { settings::switch_on(path) };
    let done = match done {
        Ok(done) => done,
        Err(problem) => {
            say(&format!("Не внесено: {problem}"));
            return 1;
        }
    };
    if done.is_empty() {
        say(&format!(
            "==> Менять нечего: {}",
            if off { "в настройках Claude Code этого и не было." } else { "в настройках Claude Code всё уже стоит." }
        ));
        return 0;
    }
    say(&format!("==> Настройки Claude Code ({}):", path.display()));
    for line in done {
        say(&format!("    {line}"));
    }
    if settings::kept_copy(path).exists() {
        say(&format!("==> Какими настройки были до hitl: {}", settings::kept_copy(path).display()));
    }
    if !off {
        say("==> Теперь режим Auto узнаёт, что ты ответил в окне проверок. Действует в чатах, открытых после этого;");
        say("    уже открытому чату может понадобиться перезапуск. Что видит режим Auto: claude auto-mode config");
        say(&format!("==> Убрать: {}", hitl_host_core::command(&["install", "--window-ok-off"])));
    }
    0
}

/// The reminder of unread answers goes into his settings of Claude Code, or comes out: by his hand only, as the hook of
/// `hitl note` does. It is a hook of his settings, which a chat does not change; a call from one changes nothing and
/// hands the chat the file he opens instead.
fn remind(places: &Places, off: bool, by_hand: bool) -> i32 {
    let option = if off { "--remind-off" } else { "--remind" };
    let path = &places.claude_settings;
    if !by_hand {
        say(&format!(
            "hitl install {option}: this is his to run, not a chat's. It changes his settings of Claude Code, the hook \
             `hitl _remind` at SessionStart and UserPromptSubmit ({} now), and a chat does not change those. Nothing was changed.",
            if settings::remind_state(path) { "it is there" } else { "it is not there" }
        ));
        if !off {
            say(&format!(
                "Give him the button: an action in the check window whose --link is {} . Opened by his hand, that file runs this \
                 command in his terminal. Say in the item what it does for him: after it, a chat of Claude Code that starts or is \
                 written to is told of the answers he gave it in the window that it has not read.",
                settings::remind_launcher(&places.home).display()
            ));
        }
        return EXIT_REFUSED;
    }
    let done = if off { settings::remind_off(path) } else { settings::remind_on(path) };
    match done {
        Err(problem) => {
            say(&format!("Не внесено: {problem}"));
            1
        }
        Ok(done) if done.is_empty() => {
            say(&format!("==> Менять нечего: {}", if off { "напоминания в настройках Claude Code и не было." } else { "напоминание уже стоит." }));
            0
        }
        Ok(done) => {
            say(&format!("==> Настройки Claude Code ({}):", path.display()));
            for line in done {
                say(&format!("    {line}"));
            }
            if !off {
                say("==> Теперь чат Claude Code узнаёт о твоём ответе в окне, который не прочёл, когда начинается или получает сообщение.");
                say(&format!("==> Убрать: {}", hitl_host_core::command(&["install", "--remind-off"])));
            }
            0
        }
    }
}

/// The command in given places (the tests use made-up ones). `by_hand`: whether a person runs it at a terminal.
pub fn run_in(places: &Places, arguments: &[String], by_hand: bool) -> i32 {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    if matches!(words.first(), Some(&"-h" | &"--help" | &"help")) {
        print!("{USAGE}");
        return 0;
    }
    if words == ["--window-ok"] || words == ["--window-ok-off"] {
        return window_ok(places, words[0] == "--window-ok-off", by_hand);
    }
    if words == ["--remind"] || words == ["--remind-off"] {
        return remind(places, words[0] == "--remind-off", by_hand);
    }
    if words.iter().any(|word| *word != "--dry-run") {
        say("usage: hitl install [--dry-run] | --window-ok | --window-ok-off | --remind | --remind-off");
        return EXIT_REFUSED;
    }
    let mut steps = Steps { dry: words.contains(&"--dry-run"), missing: Vec::new() };
    links(places, &mut steps);
    zshenv_line(places, &mut steps);
    remove_old_status_line(places, &mut steps);
    chat_button(places, &mut steps);
    move_state(places, &mut steps);
    report(places, &mut steps);
    for line in &steps.missing {
        say(&format!("Не хватает: {line}"));
    }
    if steps.missing.is_empty() {
        say(if steps.dry { "==> Больше ничего не нужно." } else { "==> Всё на месте." });
    }
    if steps.missing.is_empty() { 0 } else { 1 }
}

pub fn main(arguments: &[String]) -> i32 {
    run_in(&Places::from_environment(), arguments, by_his_hand())
}

/// Answers `command` with its arguments, or None when this module does not have it (yet).
pub fn dispatch(command: &str, rest: &[String]) -> Option<i32> {
    (command == "install").then(|| main(rest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static NUMBER: AtomicU32 = AtomicU32::new(0);

    /// Made-up folders for everything the command touches, with a project of its own to link to.
    struct Made {
        top: PathBuf,
        places: Places,
    }

    impl Drop for Made {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.top);
        }
    }

    impl Made {
        fn new() -> Made {
            let top = std::fs::canonicalize(std::env::temp_dir())
                .unwrap()
                .join(format!("conveyor-install-rust-{}-{}", std::process::id(), NUMBER.fetch_add(1, Ordering::SeqCst)));
            let _ = std::fs::remove_dir_all(&top);
            let root = top.join("project");
            std::fs::create_dir_all(root.join("skills/one")).unwrap();
            std::fs::write(root.join("skills/one/SKILL.md"), "one\n").unwrap();
            std::fs::write(root.join("hitl"), "#!/bin/sh\n").unwrap();
            let home = top.join("user");
            std::fs::create_dir_all(home.join("Code/scripts")).unwrap();
            let places = Places {
                scripts: home.join("Code/scripts"),
                skills: home.join(".agents/skills"),
                zshenv: home.join(".zshenv"),
                old_window_home: home.join(".agent-checks"),
                old_phone_home: home.join("Library/Developer/Xcode/DerivedData/.ios-run-device"),
                claude_settings: home.join(".claude/settings.json"),
                entry: root.join("hitl"),
                root,
                home: home.join(".conveyor"),
            };
            Made { top, places }
        }
    }

    #[test]
    fn links_and_the_setting_are_put_in_place_once_and_a_dry_run_changes_nothing() {
        let made = Made::new();
        let places = &made.places;
        assert_eq!(run_in(places, &["--dry-run".to_string()], false), 1);
        assert!(!places.scripts.join("hitl").exists(), "a dry run changes nothing");
        assert!(!places.zshenv.exists());
        assert_eq!(run_in(places, &[], false), 1, "there is no builder in this home, and it says so");
        for name in ["hitl", "ios-run-gate"] {
            assert_eq!(std::fs::canonicalize(places.scripts.join(name)).unwrap(), std::fs::canonicalize(&places.entry).unwrap());
        }
        assert!(places.skills.join("one").is_symlink());
        assert!(std::fs::read_to_string(&places.zshenv).unwrap().contains(BUILDER_LINE));
        assert!(!places.claude_settings.exists());
        std::fs::write(places.scripts.join("ios-run.sh"), "#!/bin/bash\nqueue_command=\"$SCRIPT_DIR/hitl\"\n").unwrap();
        let missing_tools = ["swiftc", "git", "gh", "xcrun"].iter().any(|tool| hitl_host_core::which(tool).is_none());
        if !missing_tools {
            assert_eq!(run_in(places, &[], false), 0);
        }
        assert_eq!(std::fs::read_to_string(&places.zshenv).unwrap().matches("IOS_RUN_COMMAND").count(), 1);
    }

    #[test]
    fn only_the_status_line_installed_by_conveyor_is_removed() {
        let made = Made::new();
        let places = &made.places;
        std::fs::create_dir_all(places.claude_settings.parent().unwrap()).unwrap();
        let old = json!({"model": "opus", "statusLine": {
            "type": "command", "command": format!("{} statusline", settings::program()), "padding": 0
        }});
        std::fs::write(&places.claude_settings, old.to_string()).unwrap();
        let mut steps = Steps { dry: true, missing: Vec::new() };
        remove_old_status_line(places, &mut steps);
        assert_eq!(std::fs::read_to_string(&places.claude_settings).unwrap(), old.to_string());
        steps.dry = false;
        remove_old_status_line(places, &mut steps);
        let kept = settings::read_settings(&places.claude_settings).unwrap();
        assert_eq!(kept["model"], "opus");
        assert!(!kept.contains_key("statusLine"));
        let custom = r#"{"statusLine": {"type": "command", "command": "~/my-line.sh"}}"#;
        std::fs::write(&places.claude_settings, custom).unwrap();
        remove_old_status_line(places, &mut steps);
        assert_eq!(std::fs::read_to_string(&places.claude_settings).unwrap(), custom);
        assert!(steps.missing.is_empty());
    }

    #[test]
    fn links_to_what_the_project_no_longer_has_go_and_others_stay() {
        let made = Made::new();
        let places = &made.places;
        std::fs::create_dir_all(places.scripts.join("lib")).unwrap();
        std::os::unix::fs::symlink(places.root.join("mac-check"), places.scripts.join("mac-check")).unwrap();
        std::os::unix::fs::symlink(places.root.join("lib/x.py"), places.scripts.join("lib/x.py")).unwrap();
        std::fs::write(places.scripts.join("iphone-check"), "his own file\n").unwrap();
        let elsewhere = made.top.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, places.scripts.join("land-pr")).unwrap();
        std::fs::create_dir_all(&places.skills).unwrap();
        std::os::unix::fs::symlink(places.root.join("skills/gone"), places.skills.join("gone-skill")).unwrap();
        std::fs::create_dir_all(places.skills.join("his-skill")).unwrap();
        run_in(places, &[], false);
        assert!(!is_symlink(&places.scripts.join("mac-check")));
        assert!(!is_symlink(&places.scripts.join("lib/x.py")));
        assert!(!is_symlink(&places.skills.join("gone-skill")));
        assert!(places.scripts.join("iphone-check").is_file());
        assert!(is_symlink(&places.scripts.join("land-pr")));
        assert!(places.skills.join("his-skill").is_dir());
    }

    #[test]
    fn what_was_kept_in_two_places_moves_into_the_one_home_with_its_history_and_not_twice() {
        let made = Made::new();
        let places = &made.places;
        let window = &places.old_window_home;
        std::fs::create_dir_all(window.join("done")).unwrap();
        std::fs::create_dir_all(window.join("queue")).unwrap();
        std::fs::create_dir_all(window.join("panel")).unwrap();
        std::fs::write(window.join("panel/CheckPanel-old"), "old\n").unwrap();
        std::fs::write(window.join("done/a.json"), r#"{"id": "a"}"#).unwrap();
        std::fs::write(window.join("place.json"), r#"{"corner": [900, 700]}"#).unwrap();
        std::fs::write(
            window.join("history.jsonl"),
            "{\"id\": \"a\", \"tag\": \"r/1\", \"results\": [{\"state\": \"ok\"}], \"how\": \"done\", \"finishedAt\": 30}\n\
             {\"id\": \"c\", \"tag\": \"r/3\", \"results\": [], \"how\": \"dropped\", \"finishedAt\": 10}\nnot json\n",
        )
        .unwrap();
        let phone = places.old_phone_home.join("UDID-1");
        std::fs::create_dir_all(phone.join("tickets/b")).unwrap();
        std::fs::write(phone.join("state.json"), r#"{"active": null, "queue": [], "recent": [{"id": "b"}]}"#).unwrap();
        std::fs::write(phone.join("history.jsonl"), "{\"id\": \"b\", \"tag\": \"r/2\", \"how\": \"phone\", \"finishedAt\": 20}\n").unwrap();
        std::fs::create_dir_all(places.old_phone_home.join("mac-tests")).unwrap();
        std::fs::write(places.old_phone_home.join("mac-tests/state.json"), r#"{"running": null, "waiting": []}"#).unwrap();
        std::fs::create_dir_all(places.old_phone_home.join("landing-owner-app")).unwrap();
        std::fs::write(places.old_phone_home.join("landing-owner-app/history.jsonl"), "{\"what\": \"#1\"}\n").unwrap();

        let mut steps = Steps { dry: false, missing: Vec::new() };
        move_state(places, &mut steps);
        assert!(steps.missing.is_empty(), "{:?}", steps.missing);
        let history: Vec<Value> =
            std::fs::read_to_string(places.home.join("history.jsonl")).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        let seen: Vec<(String, String)> =
            history.iter().map(|entry| (entry["id"].as_str().unwrap().into(), entry["screen"].as_str().unwrap().into())).collect();
        assert_eq!(seen, [("c".into(), "mac".into()), ("b".into(), "iphone".into()), ("a".into(), "mac".into())]);
        assert_eq!(history[1]["udid"], "UDID-1");
        assert!(places.home.join("window/place.json").is_file() && places.home.join("done/a.json").is_file());
        assert!(places.home.join("iphone/UDID-1/tickets/b").is_dir());
        assert!(places.home.join("turns/mac-tests/state.json").is_file());
        assert!(places.home.join("turns/landing-owner-app/history.jsonl").is_file());
        assert!(places.home.join("old/agent-checks/panel/CheckPanel-old").is_file(), "nothing is deleted");
        assert!(!window.exists() && !places.old_phone_home.exists());
        let mut again = Steps { dry: false, missing: Vec::new() };
        move_state(places, &mut again);
        assert_eq!(std::fs::read_to_string(places.home.join("history.jsonl")).unwrap().lines().count(), 3);
    }

    #[test]
    fn nothing_is_moved_from_under_a_check_that_waits() {
        let made = Made::new();
        let places = &made.places;
        std::fs::create_dir_all(places.old_window_home.join("queue")).unwrap();
        std::fs::write(places.old_window_home.join("queue/waiting.json"), r#"{"id": "waiting"}"#).unwrap();
        std::fs::write(places.old_window_home.join("history.jsonl"), "{\"id\": \"a\"}\n").unwrap();
        let mut steps = Steps { dry: false, missing: Vec::new() };
        move_state(places, &mut steps);
        assert!(steps.missing[0].contains("состояние не перенесено"), "{:?}", steps.missing);
        assert!(places.old_window_home.join("history.jsonl").is_file() && !places.home.exists());
    }

    #[test]
    fn a_chat_may_not_switch_the_reminder_and_is_handed_the_file_instead() {
        let made = Made::new();
        let places = &made.places;
        assert_eq!(run_in(places, &["--remind".to_string()], false), EXIT_REFUSED);
        assert!(!settings::remind_state(&places.claude_settings));
        assert!(places.home.join("remind.command").exists());
        assert_eq!(run_in(places, &["--remind".to_string()], true), 0, "by his hand it is switched on");
        assert!(settings::remind_state(&places.claude_settings));
        assert_eq!(run_in(places, &["--remind-off".to_string()], false), EXIT_REFUSED);
        assert!(settings::remind_state(&places.claude_settings), "a chat takes nothing out either");
        assert_eq!(run_in(places, &["--remind-off".to_string()], true), 0);
        assert!(!settings::remind_state(&places.claude_settings));
    }

    #[test]
    fn a_chat_may_not_switch_the_window_ok_and_is_handed_the_file_instead() {
        let made = Made::new();
        let places = &made.places;
        assert_eq!(run_in(places, &["--window-ok".to_string()], false), EXIT_REFUSED);
        assert!(!places.claude_settings.exists(), "nothing was changed");
        assert_eq!(settings::state(&places.claude_settings), (false, false));
        assert_eq!(run_in(places, &["--window-ok-off".to_string()], false), EXIT_REFUSED);
        assert_eq!(run_in(places, &["--window-ok".to_string(), "--dry-run".to_string()], false), EXIT_REFUSED);
        assert_eq!(run_in(places, &["--window-ok".to_string()], true), 0, "by his hand it is switched on");
        assert_eq!(settings::state(&places.claude_settings), (true, true));
    }
}
