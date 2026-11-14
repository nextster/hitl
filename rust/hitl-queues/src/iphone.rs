//! `hitl iphone`: a build on the shared iPhone, through the queue of hand checks (iphone.py).
//!
//! One phone serves many agent chats, so a build reaches it in one of two ways: for the reviewer to check by hand
//! (`--check`, behind the checks queued before it) or as the caller's own run (`--probe`, only while no check is on
//! the phone or waits). The building is ios-run.sh's work: every option this command does not know is passed on to
//! it. The queue is `crate::commands` (queues.py).
//!
//! The code is split so that the parser (`Call::parse`), the choice of what to do (`decide`), the argument lists
//! (`queue_arguments`, `check_arguments`, `Call::for_ios_run`) and the environment of ios-run.sh are plain functions;
//! `main` only joins them to the programs it starts.

use crate::items::{FIELD_SEPARATOR, device_modes_text};
use hitl_host_core::process::{self, Options};
use hitl_host_core::say;
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};

pub const USAGE: &str = "Put a build on the shared iPhone through the queue of hand checks, or look at
the queue and steer it.

Usage:
  hitl iphone [PROJECT_OR_DIRECTORY] --check TEXT [--check TEXT]... [options]
  hitl iphone [PROJECT_OR_DIRECTORY] --probe [options] [-- app-argument ...]
  hitl iphone --queue-release | --queue-first ID|TAG | --queue-pause | ...

The build is made by ios-run.sh: the project, --device-name, --device-id,
--scheme, --configuration, --bundle-id, --url, --tag, the test options and
whatever else this command does not list below go to it unchanged. See
`ios-run.sh --help`.

A hand check:
  --check TEXT             Queue the build for a hand check. TEXT is one
                           checklist line, \"what to do → what should happen\";
                           repeat it per item. A leading \"3.\" or \"4a.\" stays
                           the item's number.
  --launch-env-file FILE   Private mode-600 JSON launch input (version, bundleID,
                           udid, variables). Its parent must be mode 700. Values
                           go only to this ticket’s device launch, never the build.
  --check-title TEXT       What the check is about, in a few words.
  --check-setup IDS        For the --check before it, in an app that can act on a
                           checklist: actions the app runs when the tester comes
                           to the item, by the ids of the app's own vocabulary,
                           comma-separated (\"open-settings,dock-fold\").
  --check-button IDS       Actions offered as buttons with that item. An app
                           that has said what its actions are draws each by
                           its kind: a link under its address, two actions
                           that undo each other as one switch, a group of
                           which one holds as a row of options, an action
                           done once as a symbol. Naming one action of a
                           switch or of a choice brings the whole button.
  --check-link URL         A deep link offered as a button with that item,
                           under its address.
  --checklist FILE         Checklist from a file: a title line, then one item
                           per line; \"setup: …\", \"buttons: …\" and \"link: …\"
                           lines under an item belong to it. Combines with
                           --check.

The caller's own run:
  --probe                  Not for a hand check: a measurement, a log, a launch
                           to diagnose. Waits until no check is on the phone or
                           queued. Every test run on the phone is one too, with
                           or without the word.

Both:
  --no-wait                With --check: queue the build and return its ticket.
                           With --probe or device tests: fail with exit 75 when
                           the phone is busy instead of waiting.
  --force                  Urgent, and only on the word of the phone's owner.
                           With --check: the build goes on the phone now, ahead
                           of the queue and through a pause; a check that is on
                           the phone steps back to the head of the queue and
                           installs again afterwards. With --probe or device
                           tests: the run starts now the same way. With the
                           --queue commands below, from an agent chat: touch
                           another chat's check, the order or the pause.

The queue:
  What waits, on the phone and in the window on the Mac, is shown by
  `hitl queue`; `hitl wait [ID|TAG]` waits for a check again and
  `hitl drop [ID|TAG]` takes the caller's own check back. To steer the phone:

  --queue-release          End the check that is on the phone from the Mac; the
                           next one installs. --note TEXT adds what was said.
  --queue-first ID|TAG     Put a waiting check next in line.
  --queue-drop [ID|TAG]    Take a check out of the queue, or off the phone; the
                           caller's own when omitted.
  --queue-pause            Hold the queue: nothing new goes on the phone. With
                           --only TAG (repeatable) that chat's checks still do,
                           and another chat's check that is on the phone steps
                           back to the head of the queue.
  --queue-resume           Let the queue move again.
  -h, --help               Show this help.

How it goes:
  A hand check waits its turn. One check is on the phone at a time; the app
  shows its checklist and the phone stays with it until the tester ends it
  there: \"Готово\" once every item has a mark, \"Доработать\" before that. Either
  frees the phone, or --queue-release does from the Mac, and the next build
  installs by itself. The command waits through all of that and prints the
  items by number, grouped by mark, with the tester's comments; so run it in
  the background. A wait that was cut short loses nothing, `hitl wait`
  picks the check up again. A newer build of a chat that still waits takes that place
  in the queue; a recheck after a verdict goes to the end.

  A probe starts only while no check is on the phone or queued, and a check
  submitted meanwhile replaces it. A probe is never handed over for a hand
  check.

  The app is told who it came from: \"--ios-run-tag\" carries the first words of
  the chat's title and the Git branch. An app that declares the argument
  \"--ios-run-checklist\" receives the checklist with it and writes the marks to
  \"Library/Application Support/ios-run/check.json\" in its data container; for
  any other app the check ends with --queue-release.

  ios-run.sh itself refuses to install on the phone or launch there around
  this queue, and says so; test runs on the Mac take turns through it without
  this command (`ios-run.sh --mac --test`).

Examples:
  hitl iphone --device-name \"My iPhone\" --check-title \"Reorder\" \\
      --check \"1. Drag a row → it follows the finger\" --check \"2. Drop it → it stays\"
  hitl iphone --device-name \"My iPhone\" --probe
  hitl iphone --device-name \"My iPhone\" --probe --tag \"reorder animation\"
  hitl queue
";

/// Options of ios-run.sh that take a value: what follows one is its value, whatever it looks like.
pub const IOS_RUN_VALUE_OPTIONS: [&str; 13] = [
    "--device-name", "--device-id", "--sim", "--sim-runtime", "--scheme", "--configuration", "--bundle-id", "--url",
    "--tag", "--only-testing", "--skip-testing", "--test-plan", "--parallel-workers",
];
pub const TEST_OPTIONS: [&str; 5] = ["--test", "--test-without-building", "--only-testing", "--skip-testing", "--test-plan"];
/// What does not put a build on the phone, so needs no word about why.
pub const ASIDE_OPTIONS: [&str; 3] = ["--build-only", "--list-devices", "--list-simulators"];
pub const ITEM_FIELDS: [(&str, &str); 3] = [("--check-setup", "setup"), ("--check-button", "buttons"), ("--check-link", "link")];
pub const FORCED_QUEUE_COMMANDS: [&str; 5] = ["release", "drop", "first", "pause", "resume"];

/// Whether what a link leads to is something to open (`common.is_address`): a path on this Mac, or a scheme and
/// something after it with no space.
pub fn is_address(target: &str) -> bool {
    if target.starts_with('/') || target.starts_with("~/") {
        return true;
    }
    let mut characters = target.chars();
    if !characters.next().is_some_and(|first| first.is_ascii_alphabetic()) {
        return false;
    }
    let Some(colon) = target.find(':') else { return false };
    let (scheme, rest) = (&target[..colon], &target[colon + 1..]);
    scheme.chars().skip(1).all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c)) && !rest.is_empty() && !rest.chars().any(char::is_whitespace)
}

/// Why a call stopped before it ran anything.
#[derive(Debug, PartialEq)]
pub enum Stop {
    /// `-h`: the usage on stdout, exit 0.
    Help,
    /// `Error: …` on stderr, exit 1 (`die`).
    Die(String),
    /// A program this command started ended with this code, which becomes the command's.
    Exit(i32),
}

fn die<T>(message: impl Into<String>) -> Result<T, Stop> {
    Err(Stop::Die(message.into()))
}

/// One command line, split into what is this command's and what is ios-run.sh's.
#[derive(Debug, Default)]
pub struct Call {
    pub items: Vec<String>,
    pub title: String,
    pub checklist: String,
    pub launch_env_file: String,
    pub checking: bool,
    pub probe: bool,
    pub no_wait: bool,
    pub force: bool,
    pub queue_command: String,
    pub queue_argument: String,
    pub note: String,
    pub only: Vec<String>,
    /// For ios-run.sh, in the order given: (option or project, its value or None).
    pub passed: Vec<(String, Option<String>)>,
    pub app_arguments: Vec<String>,
    pub has_separator: bool,
}

impl Call {
    pub fn parse(argv: &[String]) -> Result<Call, Stop> {
        let mut call = Call::default();
        let mut index = 0;
        // What follows the option at `index`, which has to be there.
        let value = |index: &mut usize, requirement: &str| -> Result<String, Stop> {
            if *index + 1 >= argv.len() {
                return die(format!("{} requires {requirement}.", argv[*index]));
            }
            *index += 1;
            Ok(argv[*index].clone())
        };
        while index < argv.len() {
            let argument = argv[index].as_str();
            if argument == "--" {
                call.app_arguments = argv[index + 1..].to_vec();
                call.has_separator = true;
                break;
            }
            match argument {
                "-h" | "--help" => return Err(Stop::Help),
                "--check" => {
                    let text = value(&mut index, "the text of a checklist item")?;
                    if hitl_host_core::text_links(&text).iter().any(|(_, target)| is_address(target)) {
                        return die("a --check has a link written into its text, [слова](адрес). The checklist on the phone shows an \
                                    item's text as it is, brackets and all: say the thing in words. Such a link is for the checks \
                                    of the window, `hitl check`.");
                    }
                    call.items.push(text);
                    call.checking = true;
                }
                "--check-title" => call.title = value(&mut index, "a value")?,
                "--check-setup" | "--check-button" | "--check-link" => {
                    let field = value(&mut index, "a value")?;
                    let Some(last) = call.items.last_mut() else {
                        return die(format!("{argument} belongs to the --check before it: put it after one."));
                    };
                    let name = ITEM_FIELDS.iter().find(|(option, _)| *option == argument).map(|(_, name)| *name).unwrap_or("");
                    // Joined to the item's text with a unit separator; the queue splits it.
                    last.push_str(&format!("{FIELD_SEPARATOR}{name}={field}"));
                }
                "--launch-env-file" => call.launch_env_file = value(&mut index, "a private JSON file")?,
                "--checklist" => {
                    call.checklist = value(&mut index, "a file")?;
                    call.checking = true;
                }
                "--probe" => call.probe = true,
                "--no-wait" => call.no_wait = true,
                "--force" => call.force = true,
                "--queue" => call.queue_command = "status".into(),
                "--queue-release" | "--queue-pause" | "--queue-resume" => {
                    call.queue_command = argument["--queue-".len()..].to_string();
                }
                "--queue-wait" | "--queue-drop" => {
                    call.queue_command = argument["--queue-".len()..].to_string();
                    if index + 1 < argv.len() && !argv[index + 1].starts_with('-') {
                        index += 1;
                        call.queue_argument = argv[index].clone();
                    }
                }
                "--queue-first" => {
                    call.queue_command = "first".into();
                    call.queue_argument = value(&mut index, "a check: its chat tag or ticket")?;
                }
                "--only" => {
                    let tag = value(&mut index, "a chat tag")?;
                    call.only.push(tag);
                }
                "--note" => call.note = value(&mut index, "a value")?,
                other if IOS_RUN_VALUE_OPTIONS.contains(&other) => {
                    let given = value(&mut index, "a value")?;
                    call.passed.push((other.to_string(), Some(given)));
                }
                other => call.passed.push((other.to_string(), None)),
            }
            index += 1;
        }
        Ok(call)
    }

    pub fn has(&self, options: &[&str]) -> bool {
        self.passed.iter().any(|(option, _)| options.contains(&option.as_str()))
    }

    /// The last value given to an option, "" when it was not given.
    pub fn last(&self, option: &str) -> String {
        self.passed.iter().rev().find(|(name, _)| name == option).and_then(|(_, value)| value.clone()).unwrap_or_default()
    }

    /// What goes to ios-run.sh, in the order given, without the options named.
    pub fn for_ios_run(&self, without: &[&str]) -> Vec<String> {
        let mut arguments = Vec::new();
        for (option, value) in &self.passed {
            if without.contains(&option.as_str()) {
                continue;
            }
            arguments.push(option.clone());
            if let Some(value) = value {
                arguments.push(value.clone());
            }
        }
        arguments
    }
}

/// The path this command was called by (its link among the shared scripts: `HITL_COMMAND`, which the script
/// exports), else the path of this program.
fn called_by() -> Option<PathBuf> {
    let named = hitl_host_core::var("CONVEYOR_CALLED_AS").filter(|value| value.contains('/'))
        .or_else(|| hitl_host_core::var("HITL_COMMAND").filter(|value| value.contains('/')))
        .or_else(|| std::env::args().next().filter(|value| value.contains('/')))?;
    let path = PathBuf::from(named);
    Some(if path.is_absolute() { path } else { std::env::current_dir().ok()?.join(path) })
}

/// The plain builder: named by `IOS_RUN_COMMAND`, else the ios-run.sh beside the path this command was called by,
/// else the one at the project's root, else the one among the shared scripts.
pub fn ios_run_command() -> Result<String, Stop> {
    if let Some(named) = hitl_host_core::var("IOS_RUN_COMMAND").filter(|value| !value.is_empty()) {
        return Ok(named);
    }
    let mut candidates = Vec::new();
    if let Some(parent) = called_by().and_then(|path| path.parent().map(Path::to_path_buf)) {
        candidates.push(parent.join("ios-run.sh"));
    }
    candidates.push(hitl_host_core::root().join("ios-run.sh"));
    candidates.push(hitl_host_core::paths::expand("~/Code/scripts/ios-run.sh"));
    match candidates.into_iter().find(|candidate| candidate.is_file()) {
        Some(found) => Ok(found.display().to_string()),
        None => die("ios-run.sh, which builds for this command, was not found beside it; name it with IOS_RUN_COMMAND."),
    }
}

/// What is added to the environment ios-run.sh runs in: the chat's name for the tag the app shows, and what the gate
/// reads before ios-run.sh touches the phone.
pub fn environment_for_ios_run(call: &Call, probe: bool) -> Vec<(String, String)> {
    let mut additions = Vec::new();
    let chat = hitl_host_core::chat_tag();
    if !chat.is_empty() {
        // ios-run.sh puts it ahead of the Git branch in the tag the app shows.
        additions.push(("IOS_RUN_TAG_PREFIX".to_string(), chat));
    }
    if probe {
        additions.push(("CONVEYOR_PROBE".to_string(), "1".to_string()));
    }
    if call.force {
        additions.push(("CONVEYOR_FORCE".to_string(), "1".to_string()));
    }
    additions
}

fn options_with(purpose: &str, environment: &[(String, String)]) -> Options {
    let mut options = Options::new(purpose);
    for (name, _) in std::env::vars_os().filter(|(n,_)| n.to_string_lossy().starts_with("DEVICECTL_CHILD_")) {
        options = options.env_remove(&name.to_string_lossy());
    }
    for (name, value) in environment {
        options = options.env(name, value);
    }
    options
}

/// The words of `queues.main` for a `--queue-…` command (the first is the command).
pub fn queue_arguments(call: &Call) -> Result<Vec<String>, Stop> {
    let mut arguments = vec![call.queue_command.clone()];
    let device = match call.last("--device-id") {
        found if !found.is_empty() => found,
        _ => call.last("--device-name"),
    };
    if !device.is_empty() {
        arguments.push("--udid".into());
        arguments.push(device);
    }
    if !call.note.is_empty() {
        if !["release", "drop"].contains(&call.queue_command.as_str()) {
            return die("--note goes with --queue-release or --queue-drop.");
        }
        arguments.push(format!("--note={}", call.note));
    }
    if call.force {
        if !FORCED_QUEUE_COMMANDS.contains(&call.queue_command.as_str()) {
            return die("--force goes with --queue-release, --queue-drop, --queue-first, --queue-pause or --queue-resume.");
        }
        arguments.push("--force".into());
    }
    if !call.only.is_empty() {
        if call.queue_command != "pause" {
            return die("--only goes with --queue-pause.");
        }
        arguments.extend(call.only.iter().map(|tag| format!("--only={tag}")));
    }
    if !call.queue_argument.is_empty() {
        arguments.push(call.queue_argument.clone());
    }
    Ok(arguments)
}

/// `hitl queue` as a child with this command's own streams: one listing for the phone and the window on the Mac
/// (checks.py `show`).
fn show_queue() -> i32 {
    let program = hitl_host_core::command(&[]);
    match process::run(program.clone(), &["queue".to_string()], &Options::new("iphone.queue").inherit()) {
        Ok(done) => done.code.unwrap_or_else(|| 128 + done.signal.unwrap_or(0)),
        Err(error) => {
            say(&format!("Error: {program} queue did not start: {error}"));
            1
        }
    }
}

fn queue_command(call: &Call) -> Result<i32, Stop> {
    let arguments = queue_arguments(call)?;
    if call.queue_command == "status" {
        return Ok(show_queue());
    }
    Ok(crate::commands::run(&arguments))
}

/// A field of the product ios-run.sh printed, as Python would write it into an f-string.
fn field(product: &Value, key: &str) -> Option<String> {
    Some(match product.get(key)? {
        Value::String(text) => text.clone(),
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        other => other.to_string(),
    })
}

/// The words of `queues.main` for a check, from what ios-run.sh --build-only printed and what the call asked.
pub fn check_arguments(call: &Call, product: &Value, chat: &str) -> Result<Vec<String>, Stop> {
    let said = "ios-run.sh --build-only did not say what it built: it has to be the version that prints the product.";
    let get = |key: &str| field(product, key).ok_or_else(|| Stop::Die(said.to_string()));
    if !product.is_object() {
        return die(said);
    }
    let mut arguments: Vec<String> = vec![
        "check".into(), "--udid".into(), get("deviceId")?, format!("--name={}", get("deviceName")?),
        "--app".into(), get("app")?, "--bundle-id".into(), get("bundleId")?, format!("--branch={}", get("branch")?),
        "--project".into(), get("project")?, format!("--launch-tag={}", get("tag")?),
    ];
    if product.get("target").and_then(Value::as_str) != Some("device") {
        return die("--check and --probe are for a physical device.");
    }
    if !call.launch_env_file.is_empty() {
        arguments.extend(["--launch-env-file".to_string(), call.launch_env_file.clone()]);
    }
    if let Some(core) = product.get("coreDeviceId").and_then(Value::as_str).filter(|core| !core.is_empty()) {
        arguments.push("--core-id".into());
        arguments.push(core.to_string());
    }
    if !chat.is_empty() {
        arguments.push(format!("--tag={chat}"));
    }
    if !call.title.is_empty() {
        arguments.push(format!("--title={}", call.title));
    }
    if !call.checklist.is_empty() {
        arguments.push("--checklist".into());
        arguments.push(call.checklist.clone());
    }
    let url = call.last("--url");
    if !url.is_empty() {
        arguments.push(format!("--url={url}"));
    }
    if call.no_wait {
        arguments.push("--no-wait".into());
    }
    if call.force {
        arguments.push("--force".into());
    }
    if let Some(commit) = product.get("commit").and_then(Value::as_str).filter(|commit| !commit.is_empty()) {
        arguments.push("--commit".into());
        arguments.push(commit.to_string());
    }
    arguments.extend(call.items.iter().map(|item| format!("--item={item}")));
    arguments.push("--".into());
    arguments.extend(call.app_arguments.iter().cloned());
    Ok(arguments)
}

/// What ios-run.sh --build-only is called with.
pub fn build_command(call: &Call) -> Result<Vec<String>, Stop> {
    let mut command = vec![ios_run_command()?];
    command.extend(call.for_ios_run(&["--url", "--build-only"]));
    command.push("--build-only".into());
    Ok(command)
}

/// Python's `shlex.quote`.
pub fn shell_quote(word: &str) -> String {
    if word.is_empty() {
        return "''".into();
    }
    if word.chars().all(|c| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c)) {
        return word.to_string();
    }
    format!("'{}'", word.replace('\'', "'\"'\"'"))
}

/// Runs ios-run.sh with stdout taken and stderr shown as it comes; the stdout and the code.
fn build_output(command: &[String], environment: &[(String, String)]) -> Result<(Option<i32>, String), Stop> {
    let mut options = options_with("iphone.build", environment);
    options.capture = true;
    let mut child = match process::spawn(command[0].clone(), &command[1..], &options) {
        Ok(child) => child,
        Err(error) => return die(format!("{} did not start: {error}", command[0])),
    };
    let errors = child.stderr.take().map(|pipe| {
        std::thread::spawn(move || {
            let mut lines = BufReader::new(pipe);
            let mut line = Vec::new();
            let mut stderr = std::io::stderr();
            while matches!(lines.read_until(b'\n', &mut line), Ok(count) if count > 0) {
                let _ = stderr.write_all(&line);
                let _ = stderr.flush();
                line.clear();
            }
        })
    });
    let mut bytes = Vec::new();
    if let Some(mut pipe) = child.stdout.take() {
        let _ = pipe.read_to_end(&mut bytes);
    }
    let status = child.wait();
    if let Some(reading) = errors {
        let _ = reading.join();
    }
    let code = match status {
        Ok(status) => {
            use std::os::unix::process::ExitStatusExt;
            status.code().or_else(|| status.signal().map(|signal| 128 + signal))
        }
        Err(_) => None,
    };
    Ok((code, String::from_utf8_lossy(&bytes).to_string()))
}

/// The last line of what the build printed, as the product it says it built.
pub fn product_of(output: &str) -> Result<Value, Stop> {
    let said = "ios-run.sh --build-only did not say what it built: it has to be the version that prints the product.";
    let last = output.lines().filter(|line| !line.trim().is_empty()).last();
    match last.and_then(|line| serde_json::from_str::<Value>(line).ok()) {
        Some(product) => Ok(product),
        None => die(said),
    }
}

/// What a check does before the queue: has ios-run.sh build, and makes the arguments for the queue.
pub fn check_plan(call: &Call) -> Result<Vec<String>, Stop> {
    if !call.launch_env_file.is_empty() {
        crate::private_launch_env::validate_source(Path::new(&call.launch_env_file)).map_err(|e| Stop::Die(e.to_string()))?;
    }
    let command = build_command(call)?;
    let (code, output) = build_output(&command, &environment_for_ios_run(call, false))?;
    if code != Some(0) {
        return Err(Stop::Exit(code.unwrap_or(1)));
    }
    let product = product_of(&output)?;
    check_arguments(call, &product, &hitl_host_core::chat_tag())
}

/// Has ios-run.sh build, then hands the build and its checklist to the queue, which installs it in its turn and waits
/// for the verdict.
fn check(call: &Call) -> Result<i32, Stop> {
    let arguments = check_plan(call)?;
    let app = arguments.iter().position(|word| word == "--app").map(|at| arguments[at + 1].clone()).unwrap_or_default();
    let name = Path::new(&app).file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default();
    say(&format!("==> Queueing {name} for a hand check"));
    if call.has(&["--dry-run"]) {
        let quoted: Vec<String> = arguments.iter().map(|word| shell_quote(word)).collect();
        say(&format!("+ hitl iphone, to the queue: {}", quoted.join(" ")));
        return Ok(0);
    }
    Ok(crate::commands::run(&arguments))
}

/// What the run hands to ios-run.sh: the command first.
pub fn hand_over_command(call: &Call) -> Result<Vec<String>, Stop> {
    let mut command = vec![ios_run_command()?];
    command.extend(call.for_ios_run(&[]));
    if call.no_wait {
        command.push("--no-wait".into());
    }
    if call.has_separator {
        command.push("--".into());
        command.extend(call.app_arguments.iter().cloned());
    }
    Ok(command)
}

/// The run is ios-run.sh's from here; its gate takes the phone's turn right before the phone is used, not for the
/// time of the build. Only returns when ios-run.sh could not be started.
fn hand_to_ios_run(call: &Call, probe: bool) -> Result<i32, Stop> {
    let command = hand_over_command(call)?;
    let options = options_with("iphone.ios_run", &environment_for_ios_run(call, probe));
    let error = process::exec(command[0].clone(), &command[1..], &options);
    die(format!("{} did not start: {error}", command[0]))
}

/// What the call comes to, after the checks of its options.
#[derive(Debug, PartialEq)]
pub enum Action {
    Queue,
    Check,
    /// A build on the phone without a mode: the text that says which two there are.
    NeedsMode,
    HandOver { probe: bool },
}

/// The order of the refusals is the order of the words a chat reads first.
pub fn decide(call: &Call) -> Result<Action, Stop> {
    if !call.launch_env_file.is_empty() && (!call.checking || call.probe || !call.queue_command.is_empty()) {
        return die("--launch-env-file belongs only to --check or --checklist.");
    }
    if !call.queue_command.is_empty() {
        return Ok(Action::Queue);
    }
    if !call.note.is_empty() {
        return die("--note goes with --queue-release or --queue-drop.");
    }
    if !call.only.is_empty() {
        return die("--only goes with --queue-pause.");
    }
    let off_the_phone = call.has(&["--mac", "--sim"]);
    let tests = call.has(&TEST_OPTIONS);
    if !call.title.is_empty() && !call.checking {
        return die("--check-title names a check: add --check or --checklist.");
    }
    if call.force && off_the_phone {
        return die("--force takes the physical device out of turn: it does not combine with --mac or --sim.");
    }
    if call.checking || call.probe {
        if off_the_phone {
            return die("--check and --probe are for a physical device: they do not combine with --mac or --sim.");
        }
        if call.checking && call.probe {
            return die("--check and --probe are two different runs: pick one.");
        }
    }
    if call.checking {
        if tests || call.has(&["--launch-only", "--no-build", "--console"]) {
            return die("--check queues a fresh build for a hand check: it does not combine with --test, --launch-only or --console.");
        }
        if !call.checklist.is_empty() && !Path::new(&call.checklist).is_file() {
            return die(format!("Checklist file not found: '{}'.", call.checklist));
        }
        return Ok(Action::Check);
    }
    // A build on the shared phone has to say what it is there for.
    if !(off_the_phone || call.probe || tests || call.has(&ASIDE_OPTIONS)) {
        return Ok(Action::NeedsMode);
    }
    Ok(Action::HandOver { probe: call.probe })
}

fn run(argv: &[String]) -> Result<i32, Stop> {
    let call = Call::parse(argv)?;
    match decide(&call)? {
        Action::Queue => queue_command(&call),
        Action::Check => check(&call),
        Action::NeedsMode => {
            say(&device_modes_text());
            Ok(2)
        }
        Action::HandOver { probe } => hand_to_ios_run(&call, probe),
    }
}

/// `hitl iphone`.
pub fn main(argv: &[String]) -> i32 {
    match run(argv) {
        Ok(code) => code,
        Err(Stop::Help) => {
            print!("{USAGE}");
            let _ = std::io::stdout().flush();
            0
        }
        Err(Stop::Die(message)) => {
            say(&format!("Error: {message}"));
            1
        }
        Err(Stop::Exit(code)) => code,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::testing::Sandbox;
    use serde_json::json;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(String::from).collect()
    }

    fn call(arguments: &[&str]) -> Call {
        Call::parse(&arguments.iter().map(|word| word.to_string()).collect::<Vec<_>>()).unwrap_or_else(|stop| panic!("{stop:?}"))
    }

    fn refusal(arguments: &[&str]) -> String {
        let parsed = Call::parse(&arguments.iter().map(|word| word.to_string()).collect::<Vec<_>>());
        match parsed.and_then(|call| decide(&call).map(|_| ())) {
            Err(Stop::Die(message)) => message,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    fn write_script(folder: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = folder.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn an_option_without_its_value_is_refused_with_its_name() {
        for (arguments, message) in [
            ("--check", "--check requires the text of a checklist item."),
            ("--check-title", "--check-title requires a value."),
            ("--checklist", "--checklist requires a file."),
            ("--queue-first", "--queue-first requires a check: its chat tag or ticket."),
            ("--only", "--only requires a chat tag."),
            ("--note", "--note requires a value."),
            ("--device-id", "--device-id requires a value."),
        ] {
            assert_eq!(Call::parse(&words(arguments)).err(), Some(Stop::Die(message.into())), "{arguments}");
        }
    }

    #[test]
    fn options_it_does_not_know_go_to_the_builder_in_order() {
        let given = call(&["/path/Project", "--device-id", "X", "--dry-run", "--tag", "--check", "--mac"]);
        assert!(!given.checking, "a value that looks like an option of this command is the builder's value");
        assert_eq!(given.passed.len(), 5);
        assert_eq!(given.for_ios_run(&[]), words("/path/Project --device-id X --dry-run --tag --check --mac"));
        assert_eq!(given.for_ios_run(&["--tag", "--dry-run"]), words("/path/Project --device-id X --mac"));
        assert_eq!(given.last("--device-id"), "X");
        assert_eq!(given.last("--scheme"), "");
        assert!(given.has(&["--mac", "--sim"]) && !given.has(&["--sim"]));
    }

    #[test]
    fn what_follows_the_separator_is_the_apps() {
        let given = call(&["--probe", "--", "--flag", "--check", "x"]);
        assert!(given.probe && given.has_separator && !given.checking);
        assert_eq!(given.app_arguments, words("--flag --check x"));
        assert!(given.passed.is_empty());
        assert!(!call(&["--probe"]).has_separator);
        assert!(call(&["--probe", "--"]).has_separator);
    }

    #[test]
    fn the_fields_of_an_item_are_joined_to_it_with_the_separator() {
        let given = call(&["--check", "1. Open settings", "--check-setup", "open-settings,dock-fold", "--check-button", "dock-unfold",
                           "--check", "2. Open the main screen", "--check-link", "demo://screen/main"]);
        assert_eq!(given.items, vec![
            "1. Open settings\u{1f}setup=open-settings,dock-fold\u{1f}buttons=dock-unfold".to_string(),
            "2. Open the main screen\u{1f}link=demo://screen/main".to_string(),
        ]);
        let early = Call::parse(&words("--check-setup open-settings --check x"));
        assert_eq!(early.err(), Some(Stop::Die("--check-setup belongs to the --check before it: put it after one.".into())));
    }

    #[test]
    fn a_link_written_into_an_item_is_refused_and_a_plain_bracket_is_not() {
        let message = refusal(&["--check", "1. Open [the task](https://example.test/task/320)"]);
        assert!(message.contains("shows an item's text as it is"), "{message}");
        assert!(Call::parse(&["--check".to_string(), "1. a [b](c d) and [e]".to_string()]).is_ok());
    }

    #[test]
    fn the_queue_commands_take_their_optional_argument() {
        let given = call(&["--queue-drop", "r/38"]);
        assert_eq!((given.queue_command.as_str(), given.queue_argument.as_str()), ("drop", "r/38"));
        let given = call(&["--queue-drop", "--force"]);
        assert_eq!((given.queue_command.as_str(), given.queue_argument.as_str(), given.force), ("drop", "", true));
        let given = call(&["--queue-first", "r/40"]);
        assert_eq!((given.queue_command.as_str(), given.queue_argument.as_str()), ("first", "r/40"));
        assert_eq!(call(&["--queue"]).queue_command, "status");
        assert_eq!(call(&["--queue-release"]).queue_command, "release");
        assert_eq!(call(&["--queue-pause", "--only", "r/1", "--only", "r/2"]).only, words("r/1 r/2"));
    }

    #[test]
    fn the_arguments_for_the_queue() {
        let arguments = |line: &[&str]| queue_arguments(&call(line)).map_err(|stop| format!("{stop:?}"));
        assert_eq!(arguments(&["--queue-release", "--device-id", "U1", "--device-name", "N", "--note", "слово"]).unwrap(),
                   words("release --udid U1 --note=слово"));
        assert_eq!(arguments(&["--queue-release", "--device-name", "N"]).unwrap(), words("release --udid N"));
        assert_eq!(arguments(&["--queue-first", "r/40", "--force"]).unwrap(), words("first --force r/40"));
        assert_eq!(arguments(&["--queue-pause", "--only", "r/1", "--only", "r/2"]).unwrap(), words("pause --only=r/1 --only=r/2"));
        assert_eq!(arguments(&["--queue-drop", "r/38"]).unwrap(), words("drop r/38"));
        assert_eq!(arguments(&["--queue"]).unwrap(), words("status"));
    }

    #[test]
    fn the_refusals_of_the_queue_commands() {
        let message = |line: &[&str]| match queue_arguments(&call(line)) {
            Err(Stop::Die(message)) => message,
            other => panic!("{other:?}"),
        };
        assert_eq!(message(&["--queue", "--note", "слово"]), "--note goes with --queue-release or --queue-drop.");
        assert_eq!(message(&["--queue-pause", "--note", "x"]), "--note goes with --queue-release or --queue-drop.");
        assert!(message(&["--queue", "--force"]).starts_with("--force goes with --queue-release, --queue-drop"));
        assert_eq!(message(&["--queue-release", "--only", "r/1"]), "--only goes with --queue-pause.");
        // `decide` sends every call with a queue command to the queue; the refusals are the queue's own.
        assert_eq!(decide(&call(&["--queue"])), Ok(Action::Queue));
    }

    #[test]
    fn the_refusals_before_a_run_are_in_the_order_the_python_has_them() {
        assert_eq!(refusal(&["--note", "x", "--check", "1."]), "--note goes with --queue-release or --queue-drop.");
        assert_eq!(refusal(&["--only", "r/1"]), "--only goes with --queue-pause.");
        assert_eq!(refusal(&["--check-title", "Тема", "--probe"]), "--check-title names a check: add --check or --checklist.");
        assert_eq!(refusal(&["--mac", "--force"]), "--force takes the physical device out of turn: it does not combine with --mac or --sim.");
        assert_eq!(refusal(&["--sim", "Test Phone", "--check", "1. Что-то"]),
                   "--check and --probe are for a physical device: they do not combine with --mac or --sim.");
        assert_eq!(refusal(&["--probe", "--mac"]),
                   "--check and --probe are for a physical device: they do not combine with --mac or --sim.");
        assert_eq!(refusal(&["--check", "1. Что-то", "--probe"]), "--check and --probe are two different runs: pick one.");
        for option in ["--launch-only", "--test", "--console", "--no-build", "--only-testing"] {
            let arguments: Vec<&str> = if option == "--only-testing" { vec!["--check", "1.", option, "T"] } else { vec!["--check", "1.", option] };
            assert!(refusal(&arguments).starts_with("--check queues a fresh build for a hand check"), "{option}");
        }
        assert!(refusal(&["--check", "1. Что-то", "--launch-only"]).ends_with("does not combine with --test, --launch-only or --console."));
        assert_eq!(refusal(&["--checklist", "/no/such/file"]), "Checklist file not found: '/no/such/file'.");
    }

    #[test]
    fn what_a_call_comes_to() {
        assert_eq!(decide(&call(&["--device-id", "X", "--dry-run"])), Ok(Action::NeedsMode));
        assert_eq!(decide(&call(&["--device-id", "X", "--check", "1. a", "--dry-run"])), Ok(Action::Check));
        assert_eq!(decide(&call(&["--device-id", "X", "--probe"])), Ok(Action::HandOver { probe: true }));
        assert_eq!(decide(&call(&["--device-id", "X", "--test-without-building"])), Ok(Action::HandOver { probe: false }));
        assert_eq!(decide(&call(&["--mac", "--test"])), Ok(Action::HandOver { probe: false }));
        assert_eq!(decide(&call(&["--list-devices"])), Ok(Action::HandOver { probe: false }));
        assert_eq!(decide(&call(&["--sim", "X"])), Ok(Action::HandOver { probe: false }));
        // A checklist from a file that is there is a check without any --check.
        let folder = std::env::temp_dir().join(format!("iphone-rs-list-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let list = folder.join("list.txt");
        std::fs::write(&list, "Title\n1. a\n").unwrap();
        assert_eq!(decide(&call(&["--checklist", list.to_str().unwrap()])), Ok(Action::Check));
        std::fs::remove_dir_all(&folder).unwrap();
    }

    fn product() -> Value {
        json!({"deviceId": "UDID-IPHONE", "deviceName": "Test iPhone", "app": "/products/Demo.app", "bundleId": "dev.example.demo",
               "branch": "", "project": "/p/Demo Project", "tag": "r/38", "target": "device", "coreDeviceId": "CORE-IPHONE"})
    }

    #[test]
    fn a_check_carries_the_force_the_wait_the_commit_and_the_checklist() {
        let given = call(&["--check", "1. Hotfix", "--force", "--no-wait", "--checklist", "/tmp/list"]);
        let mut with_commit = product();
        with_commit["commit"] = json!("abc123");
        with_commit["branch"] = json!("main");
        with_commit["coreDeviceId"] = json!("");
        let arguments = check_arguments(&given, &with_commit, "").unwrap();
        let text = arguments.join(" ");
        assert!(text.contains("--branch=main"), "{text}");
        assert!(!text.contains("--core-id") && !text.contains("--tag=") && !text.contains("--title="), "{text}");
        assert!(text.ends_with("--checklist /tmp/list --no-wait --force --commit abc123 --item=1. Hotfix --"), "{text}");
    }

    #[test]
    fn a_build_that_did_not_say_what_it_built_is_refused() {
        let given = call(&["--check", "1."]);
        let said = "ios-run.sh --build-only did not say what it built: it has to be the version that prints the product.";
        for broken in [json!({}), json!([1]), json!("x"), {
            let mut missing = product();
            missing.as_object_mut().unwrap().remove("bundleId");
            missing
        }] {
            assert_eq!(check_arguments(&given, &broken, "").err(), Some(Stop::Die(said.into())), "{broken}");
        }
        let mut simulator = product();
        simulator["target"] = json!("simulator");
        assert_eq!(check_arguments(&given, &simulator, "").err(), Some(Stop::Die("--check and --probe are for a physical device.".into())));
        assert_eq!(product_of("noise\n\n{\"a\": 1}\n  \n").unwrap(), json!({"a": 1}));
        assert_eq!(product_of("not json").err(), Some(Stop::Die(said.into())));
        assert_eq!(product_of("").err(), Some(Stop::Die(said.into())));
    }

    #[test]
    fn the_builder_is_found_by_name_then_beside_the_command_then_at_the_root() {
        let mut sandbox = Sandbox::new("ios-run-command");
        let folder = sandbox.folder.clone();
        let beside = folder.join("scripts");
        let root = folder.join("root");
        let home = folder.join("home");
        for place in [&beside, &root, &home.join("Code/scripts")] {
            std::fs::create_dir_all(place).unwrap();
        }
        sandbox.set("HOME", home.to_str().unwrap());
        sandbox.set("HITL_ROOT", root.to_str().unwrap());
        sandbox.set("HITL_COMMAND", beside.join("hitl").to_str().unwrap());
        sandbox.set("IOS_RUN_COMMAND", "/named/by/the/chat");
        assert_eq!(ios_run_command().unwrap(), "/named/by/the/chat");
        sandbox.unset("IOS_RUN_COMMAND");
        // Not found anywhere (the test binary's own path has no ios-run.sh beside it either).
        sandbox.unset("HITL_COMMAND");
        let nowhere = ios_run_command();
        if nowhere.is_ok() {
            // Only when the test binary stands beside a real ios-run.sh; not the case in a plain cargo tree.
            return;
        }
        assert_eq!(nowhere.err(), Some(Stop::Die("ios-run.sh, which builds for this command, was not found beside it; name it with IOS_RUN_COMMAND.".into())));
        write_script(&home.join("Code/scripts"), "ios-run.sh", "true");
        assert_eq!(ios_run_command().unwrap(), home.join("Code/scripts/ios-run.sh").display().to_string());
        write_script(&root, "ios-run.sh", "true");
        assert_eq!(ios_run_command().unwrap(), root.join("ios-run.sh").display().to_string());
        write_script(&beside, "ios-run.sh", "true");
        sandbox.set("HITL_COMMAND", beside.join("hitl").to_str().unwrap());
        assert_eq!(ios_run_command().unwrap(), beside.join("ios-run.sh").display().to_string());
    }

    #[test]
    fn the_environment_of_the_builder() {
        let mut sandbox = Sandbox::new("ios-run-environment");
        let plain = call(&["--probe"]);
        assert!(environment_for_ios_run(&plain, false).is_empty());
        assert_eq!(environment_for_ios_run(&plain, true), vec![("CONVEYOR_PROBE".to_string(), "1".to_string())]);
        sandbox.set("CONVEYOR_CHAT", "r/38 · Бары внизу");
        let forced = call(&["--probe", "--force"]);
        assert_eq!(environment_for_ios_run(&forced, true), vec![
            ("IOS_RUN_TAG_PREFIX".to_string(), "r/38".to_string()),
            ("CONVEYOR_PROBE".to_string(), "1".to_string()),
            ("CONVEYOR_FORCE".to_string(), "1".to_string()),
        ]);
    }

    #[test]
    fn what_is_handed_to_the_builder_keeps_the_order_and_the_separator() {
        let mut sandbox = Sandbox::new("hand-over");
        sandbox.set("IOS_RUN_COMMAND", "/made/up/ios-run.sh");
        let given = call(&["/p/Project", "--device-id", "X", "--probe", "--no-wait", "--tag", "t", "--", "--flag"]);
        assert_eq!(hand_over_command(&given).unwrap(), words("/made/up/ios-run.sh /p/Project --device-id X --tag t --no-wait -- --flag"));
        // No separator, no `--`; a separator with nothing after it still goes.
        assert_eq!(hand_over_command(&call(&["--mac", "--test"])).unwrap(), words("/made/up/ios-run.sh --mac --test"));
        assert_eq!(hand_over_command(&call(&["--probe", "--"])).unwrap(), words("/made/up/ios-run.sh --"));
        // The build for a check drops the url and says --build-only last.
        let building = call(&["/p", "--device-id", "X", "--url", "demo://x", "--check", "1.", "--build-only"]);
        assert_eq!(build_command(&building).unwrap(), words("/made/up/ios-run.sh /p --device-id X --build-only"));
    }

    const BUILD: &str = r#"
echo "$@" > "$ARGS_FILE"
echo "env:$IOS_RUN_TAG_PREFIX:${CONVEYOR_PROBE-unset}:${CONVEYOR_FORCE-unset}" >> "$ARGS_FILE"
echo "log line of the build" >&2
echo "a noisy line of the build"
echo '{"deviceId":"UDID-IPHONE","deviceName":"Test iPhone","app":"/products/Demo.app","bundleId":"dev.example.demo","branch":"main","project":"/p","tag":"r/38 · main","target":"device","coreDeviceId":"CORE-IPHONE","commit":"deadbeef"}'
"#;

    #[test]
    fn a_check_builds_with_the_made_up_builder_and_makes_the_queue_arguments() {
        let mut sandbox = Sandbox::new("check-plan");
        let record = sandbox.folder.join("args.txt");
        let builder = write_script(&sandbox.folder, "ios-run.sh", BUILD);
        sandbox.set("IOS_RUN_COMMAND", builder.to_str().unwrap());
        sandbox.set("ARGS_FILE", record.to_str().unwrap());
        sandbox.set("CONVEYOR_JOURNAL", "0");
        sandbox.set("CONVEYOR_CHAT", "r/38 · Бары внизу");
        let given = call(&["/p/Project", "--device-id", "CORE-IPHONE", "--url", "demo://debug", "--check-title", "Dock bars",
                           "--check", "1. Swipe", "--dry-run", "--", "--flag"]);
        let arguments = check_plan(&given).unwrap();
        // The builder got the call without --url and with --build-only; it saw the chat's name for the tag.
        assert_eq!(std::fs::read_to_string(&record).unwrap(),
                   "/p/Project --device-id CORE-IPHONE --dry-run --build-only\nenv:r/38:unset:unset\n");
        let text = arguments.join(" ");
        assert!(text.starts_with("check --udid UDID-IPHONE --name=Test iPhone --app /products/Demo.app --bundle-id dev.example.demo --branch=main \
                                  --project /p --launch-tag=r/38 · main --core-id CORE-IPHONE --tag=r/38 --title=Dock bars --url=demo://debug \
                                  --commit deadbeef --item=1. Swipe -- --flag"), "{text}");
    }

    #[test]
    fn a_build_that_fails_ends_the_check_with_its_code() {
        let mut sandbox = Sandbox::new("check-fails");
        let builder = write_script(&sandbox.folder, "ios-run.sh", "echo '{\"a\":1}'; exit 7");
        sandbox.set("IOS_RUN_COMMAND", builder.to_str().unwrap());
        sandbox.set("CONVEYOR_JOURNAL", "0");
        assert_eq!(check_plan(&call(&["--check", "1."])).err(), Some(Stop::Exit(7)));
        let silent = write_script(&sandbox.folder, "silent.sh", "echo 'building' >&2");
        sandbox.set("IOS_RUN_COMMAND", silent.to_str().unwrap());
        assert!(matches!(check_plan(&call(&["--check", "1."])), Err(Stop::Die(message)) if message.contains("did not say what it built")));
        sandbox.set("IOS_RUN_COMMAND", "/no/such/ios-run.sh");
        assert!(matches!(check_plan(&call(&["--check", "1."])), Err(Stop::Die(message)) if message.contains("did not start")));
    }

    /// The hand-over replaces the process, so a test runs it in a child: this test binary again, told by the
    /// environment to do the hand-over as its only work.
    #[test]
    fn the_hand_over_replaces_the_process_with_the_builder() {
        if std::env::var("IPHONE_RS_HAND_OVER").is_ok() {
            let given = call(&["/p/Project", "--probe", "--no-wait", "--tag", "t", "--", "--flag"]);
            let _ = hand_to_ios_run(&given, true);
            std::process::exit(99); // only when the exec failed
        }
        let sandbox = Sandbox::new("hand-over-exec");
        let record = sandbox.folder.join("handed.txt");
        let builder = write_script(
            &sandbox.folder,
            "ios-run.sh",
            "echo \"$@\" > \"$HANDED_FILE\"; echo \"probe=${CONVEYOR_PROBE-unset} force=${CONVEYOR_FORCE-unset} tag=${IOS_RUN_TAG_PREFIX-unset}\" >> \"$HANDED_FILE\"; exit 5",
        );
        let done = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "iphone::tests::the_hand_over_replaces_the_process_with_the_builder", "--nocapture", "--test-threads=1"])
            .env("IPHONE_RS_HAND_OVER", "1")
            .env("IOS_RUN_COMMAND", &builder)
            .env("HANDED_FILE", &record)
            .env("CONVEYOR_JOURNAL", "0")
            .env("CONVEYOR_CHAT", "r/38 · Бары внизу")
            .output()
            .unwrap();
        assert_eq!(done.status.code(), Some(5), "{}", String::from_utf8_lossy(&done.stderr));
        assert_eq!(std::fs::read_to_string(&record).unwrap(),
                   "/p/Project --tag t --no-wait -- --flag\nprobe=1 force=unset tag=r/38\n");
    }

    #[test]
    fn main_prints_help_refusals_and_the_modes_text_with_their_codes() {
        let _sandbox = Sandbox::new("main-codes");
        assert_eq!(main(&words("--help")), 0);
        assert_eq!(main(&words("--check")), 1);
        assert_eq!(main(&words("--device-id X --dry-run")), 2);
        assert_eq!(main(&words("--check-setup x")), 1);
    }
    #[test]
    fn private_launch_input_never_reaches_the_builder_and_invalid_input_prevents_build() {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        use std::io::Write;
        let mut sandbox = Sandbox::new("private-builder");
        let folder=sandbox.folder.join("private");std::fs::DirBuilder::new().mode(0o700).create(&folder).unwrap();
        let input=folder.join("input.json");
        let mut file=std::fs::OpenOptions::new().create_new(true).write(true).mode(0o600).open(&input).unwrap();
        file.write_all(br#"{"version":1,"bundleID":"dev.example.demo","udid":"UDID-IPHONE","variables":{"REALIZE_PASSWORD":"private-build-sentinel"}}"#).unwrap();
        let record=sandbox.folder.join("args.txt");
        let script=BUILD.replace("echo \"env:","echo \"env:");
        let script=format!("{}\necho \"private-env:${{DEVICECTL_CHILD_REALIZE_PASSWORD-unset}}\" >> \"$ARGS_FILE\"\n",script);
        let builder=write_script(&sandbox.folder,"builder",&script);
        sandbox.set("IOS_RUN_COMMAND",builder.to_str().unwrap());sandbox.set("ARGS_FILE",record.to_str().unwrap());
        sandbox.set("DEVICECTL_CHILD_REALIZE_PASSWORD","ambient-build-sentinel");
        let given=call(&["/p","--check","1. Look","--launch-env-file",input.to_str().unwrap()]);
        assert!(!build_command(&given).unwrap().join(" ").contains("launch-env"));
        assert!(environment_for_ios_run(&given,false).iter().all(|(_,v)|!v.contains("sentinel")));
        let arguments=check_plan(&given).unwrap();assert!(arguments.contains(&"--launch-env-file".into()));
        let text=std::fs::read_to_string(&record).unwrap();assert!(text.contains("private-env:unset"));assert!(!text.contains("sentinel"));
        std::fs::remove_file(&record).unwrap();std::fs::remove_file(input).unwrap();assert!(check_plan(&given).is_err());assert!(!record.exists());
        for argv in [vec!["--probe","--launch-env-file","x"],vec!["--mac","--launch-env-file","x"],vec!["--queue","--launch-env-file","x"]] { assert!(decide(&call(&argv)).is_err()); }
    }

}
