//! Run a manual Mac check in the same slot as Mac test installs.
use hitl::{common, queue, submit, tell, verdict, window};
use hitl_queues::{iphone, lines};
use std::process::{Command, Stdio};
use std::time::Duration;

const USAGE: &str = "hitl mac [PROJECT] [--scheme NAME] [--configuration NAME] -- --title TEXT --check TEXT ...
Launches via ios-run --mac --run. Holds the shared Mac slot until the check ends.
Keep this command alive, or launch it as a background job with a log.";

fn runner_args(args: &[String]) -> Result<Vec<String>, &'static str> {
    let mut out = vec!["--mac".into(), "--run".into()];
    let mut args = args.iter();
    let mut project = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scheme" | "--configuration" => {
                let value = args.next().filter(|value| !value.starts_with('-')).ok_or("Missing build option value.")?;
                out.extend([arg.clone(), value.clone()]);
            }
            _ if !project && !arg.starts_with('-') => {
                out.push(arg.clone());
                project = true;
            }
            _ => return Err("Use only a project path, --scheme and --configuration before the separator."),
        }
    }
    Ok(out)
}

pub fn run(args: &[String]) -> i32 {
    if args.is_empty() || args == ["--help"] || args == ["-h"] {
        println!("{USAGE}");
        return 0;
    }
    let Some(index) = args.iter().position(|arg| arg == "--") else { return submit::refuse(USAGE) };
    let build = match runner_args(&args[..index]) {
        Ok(build) => build,
        Err(error) => return submit::refuse(error),
    };
    let parsed = match submit::parse_submission(&args[index + 1..]) {
        Ok(parsed) if parsed["wait"] != false => parsed,
        Ok(_) => return submit::refuse("Run the whole Mac check as a background job instead of using --no-wait."),
        Err(error) => return submit::refuse(&error),
    };
    let Ok(program) = iphone::ios_run_command() else { return submit::refuse("ios-run.sh was not found.") };
    let environment = common::env();
    let tag = common::text_of(&parsed, "tag");
    let chat = common::chat_title(&environment);
    let title = common::text_of(&parsed, "title");
    let pid = i64::from(std::process::id());
    let line = lines::mac_tests();
    let turn = line.wait_turn(pid, if tag.is_empty() { &chat } else { tag }, title, false,
        "Ручная проверка и тесты используют один слот на Маке.", "");
    if turn != 0 { return turn }
    struct Lease(lines::Line, i64);
    impl Drop for Lease {
        fn drop(&mut self) { self.0.leave(self.1); }
    }
    let lease = Lease(line, pid);
    let mut child = match Command::new(program).args(build).stdin(Stdio::null()).spawn() {
        Ok(child) => child,
        Err(error) => return submit::refuse(&format!("ios-run: {error}")),
    };
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(status)) => return status.code().unwrap_or(1),
            Ok(None) => lease.0.beat(pid),
            Err(error) => return submit::refuse(&format!("ios-run: {error}")),
        }
        std::thread::sleep(Duration::from_secs(1));
    }

    queue::prepare();
    let ticket = submit::submit(parsed);
    let id = common::text_of(&ticket, "id");
    let addressed = tell::addressee(&environment).filter(|who| tell::start(&ticket, who));
    println!("{id}");
    if let Some(who) = &addressed { common::say(&tell::note(who, id)); }
    window::ensure_panel();
    let filename = format!("{id}.json");
    let done = window::done_folder().join(&filename);
    let queued = window::queue_folder().join(&filename);
    loop {
        if let Some(result) = common::read_json(&done).filter(serde_json::Value::is_object) {
            if addressed.is_none() { print!("{}", verdict::verdict_text(&result, false)); }
            return if result["how"] == "dropped" { 1 } else { 0 }
        }
        if !queued.exists() { return submit::refuse("Проверка исчезла из очереди; результат не найден.") }
        lease.0.beat(pid);
        std::thread::sleep(Duration::from_secs(1));
    }
}
