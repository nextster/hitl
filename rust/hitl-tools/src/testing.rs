//! `hitl test`: every check of this project, all at the same time, with one line about each.
//!
//! Moved from `testing.py` (see `docs/rust-move.md`). It runs for the crates of `rust/` their own
//! `cargo test --offline --locked`: those whose files changed (and the crates that depend on them), all of them on
//! `--all`; and the Swift and shell suites (`package`, `builder`, `iphone`). There is no Python in it.

#[path = "testing_line.rs"]
mod line;
#[path = "testing_table.rs"]
mod table;

use hitl_host_core::process::{Options, run};
use hitl_host_core::{EXIT_REFUSED, chat_title, command, env, say, var, which};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};

const USAGE: &str = "Runs every check of this project and says in one line how each went.

Usage:
  hitl test            The suites the branch's changes touch, against main.
  hitl test --all      Every suite.
  hitl test NAME...    The suites named.

The suites: cargo-NAME for every crate of rust/ (below), iphone (the command with a made-up Xcode and
phone, on top of the real ios-run.sh), package (the Swift package for apps) and builder (ios-run.sh's own suite,
when it is beside this project or in ~/Code/scripts).

cargo-NAME is the tests of the crate rust/NAME: with no NAME those of the crates
whose files changed and the crates that depend on them, on --all every crate.
They are built together, by one cargo for the workspace of rust/, into a folder
of cargo that is this copy's of the project alone.

builder, package and iphone run only when their own files change. --all is every other
suite, one run at a time on this Mac. `hitl test` runs the suites of the
branch's changes.

The suites run at the same time: none shares anything with another. A crate's
test programs are built once, and then each of its tests runs as a process of its
own, in one of the Mac's places for tests: as many as it has cores, shared by every
run on this Mac, so several chats testing at once do not load it many times over.
The longest test (by the last run) goes first; a test that runs past ten minutes
has hung and fails its suite. Builds of tests take turns on the Mac.
CONVEYOR_TEST_WORKERS=N limits concurrent commands, including parts, in this
invocation. N must be a positive integer. Unset or empty keeps the original
parallelism; the selected suites, full-run queue and timeouts stay the same.
With no NAME the run is the smallest that covers the change: the suites of the
files changed since the branch left main, committed or not. A file of code
every suite leans on, or one no suite is known for, takes every suite of --all; a
document takes none. His words of 2026-10-15, to a tree of chats that ran the
whole set 26 times in three hours: «надо сделать вместо этого минимальный набор
тестов». A run of every suite (--all) takes its turn on this Mac, one at a time
(CONVEYOR_TEST_SLOTS), the others wait and are shown in `hitl queue`.

No window is put on the screen and no phone is touched: the window is driven by
a script, the phone and Xcode are made up. Exit code 0 when everything passed.
";

/// Suites a run takes only when their own files change: the iOS builder, the panel of the phone, the queue of the phone.
const OWN_CHANGES_ONLY: [&str; 3] = ["builder", "package", "iphone"];

/// A suite: its name, the commands that run it between them, the folder they run in, and what is set for them alone.
#[derive(Clone, Debug)]
pub struct Suite {
    pub label: String,
    pub commands: Vec<Vec<String>>,
    pub folder: PathBuf,
    pub env: Vec<(String, String)>,
    /// The crate's folder for a suite of a crate: its test programs are built once and each test runs as a process
    /// of its own, instead of `commands`.
    pub cargo: Option<PathBuf>,
}

impl Suite {
    fn name(&self) -> &str {
        self.label.split(' ').next().unwrap_or("")
    }
}

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

/// The project this run is of: the folder with the file `hitl`. In the crate's own tests it is the one the crate
/// stands in, whatever other tests of the crate do to the working folder.
fn project() -> PathBuf {
    #[cfg(test)]
    {
        std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).expect("the project")
    }
    #[cfg(not(test))]
    {
        project_of(&std::env::current_dir().unwrap_or_default(), hitl_host_core::root())
    }
}

/// The checkout the run is called from. A chat calls the command by its link among the shared scripts, and the link
/// leads to the live folder: taken as the project, that folder was what `--all` tested and what `git diff` was asked
/// of, while the chat's own copy stood unchecked beside it. The folder the call is made in wins; `fallback` (the
/// script's own root) is for a call from anywhere else.
fn project_of(here: &Path, fallback: PathBuf) -> PathBuf {
    let found = here.ancestors().find(|folder| folder.join("hitl").is_file() && folder.join("rust").is_dir());
    found.and_then(|folder| std::fs::canonicalize(folder).ok()).unwrap_or(fallback)
}

/// The folder every suite of a run shares and later runs keep: the programs they build, the program of hitl.
fn shared_folder() -> PathBuf {
    std::fs::canonicalize(std::env::temp_dir()).unwrap_or_else(|_| std::env::temp_dir()).join("conveyor-tests")
}

/// The crates tested: every crate of the workspace in `rust/`.
fn testable_crates(project: &Path) -> Vec<String> {
    all_crates(project)
}

/// The name cargo knows a crate by: the first `name` of its Cargo.toml, that of its `[package]`.
fn package_name(crate_folder: &Path) -> String {
    let text = std::fs::read_to_string(crate_folder.join("Cargo.toml")).unwrap_or_default();
    text.lines()
        .find_map(|line| line.trim().strip_prefix("name = \"").and_then(|rest| rest.split('"').next()).map(str::to_string))
        .unwrap_or_else(|| crate_folder.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default())
}

/// The folder of cargo of this copy of the project, its own. In one workspace cargo names what it built by the paths
/// inside the workspace and tells what is fresh by the times of the files, so a folder shared by two copies could
/// hand one of them the other's build. It holds the path of its copy in `ROOT`; the folders of copies that are gone
/// go, and so does the folder every copy shared before, once nothing has built into it for two days.
fn cargo_target(project: &Path) -> PathBuf {
    let text = project.display().to_string();
    let key = text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3));
    shared_folder().join("targets").join(format!("{key:016x}"))
}

fn keep_cargo_targets(project: &Path) {
    let target = cargo_target(project);
    let _ = std::fs::create_dir_all(&target);
    let _ = std::fs::write(target.join("ROOT"), format!("{}\n", project.display()));
    for entry in std::fs::read_dir(shared_folder().join("targets")).into_iter().flatten().flatten() {
        let root = std::fs::read_to_string(entry.path().join("ROOT")).unwrap_or_default();
        if !root.trim().is_empty() && !Path::new(root.trim()).is_dir() {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
    let old = shared_folder().join("cargo");
    let stale = std::fs::metadata(old.join("debug/deps")).and_then(|meta| meta.modified()).ok().and_then(|when| when.elapsed().ok());
    if stale.is_some_and(|age| age > std::time::Duration::from_secs(2 * 24 * 3600)) {
        let _ = std::fs::remove_dir_all(&old);
    }
}

/// The crates of `rust/`: every one with a Cargo.toml, by name.
fn all_crates(project: &Path) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(project.join("rust"))
        .map(|entries| {
            entries.flatten().filter(|entry| entry.path().join("Cargo.toml").is_file()).map(|entry| entry.file_name().to_string_lossy().to_string()).collect()
        })
        .unwrap_or_default();
    found.sort();
    found
}

/// The crates a crate reaches by `path = "../name"`.
fn path_dependencies(project: &Path, name: &str) -> Vec<String> {
    let text = std::fs::read_to_string(project.join("rust").join(name).join("Cargo.toml")).unwrap_or_default();
    let mut found = Vec::new();
    for piece in text.split("path = \"../").skip(1) {
        if let Some(dependency) = piece.split('"').next().filter(|dependency| !dependency.contains('/')) {
            found.push(dependency.to_string());
        }
    }
    found
}

/// The crates whose files changed, the crates the table names for a changed file (`cargo-NAME`: the Python a crate
/// took the place of), and every crate that depends on one of them.
fn cargo_touched(paths: &[String], crates_here: &[String], dependencies: &BTreeMap<String, Vec<String>>) -> BTreeSet<String> {
    let named = paths.iter().filter_map(|path| table::SUITES_OF.iter().find(|(pattern, _)| fnmatch(path, pattern))).flat_map(|(_, suites)| {
        suites.split_whitespace().filter_map(|suite| suite.strip_prefix("cargo-"))
    });
    let mut chosen: BTreeSet<String> = paths
        .iter()
        .filter_map(|path| path.strip_prefix("rust/").and_then(|rest| rest.split('/').next()))
        .chain(named)
        .filter(|name| crates_here.iter().any(|known| known == name))
        .map(str::to_string)
        .collect();
    loop {
        let more: Vec<String> = crates_here
            .iter()
            .filter(|name| !chosen.contains(*name))
            .filter(|name| dependencies.get(*name).is_some_and(|reached| reached.iter().any(|one| chosen.contains(one))))
            .cloned()
            .collect();
        if more.is_empty() {
            return chosen;
        }
        chosen.extend(more);
    }
}

fn dependency_map(project: &Path, crates_here: &[String]) -> BTreeMap<String, Vec<String>> {
    crates_here.iter().map(|name| (name.clone(), path_dependencies(project, name))).collect()
}

/// Every suite: its name, the commands that run it between them, the folder they run in. A suite of Python is there
/// only while its file `tests/NAME-tests.py` is: the parts of the move delete theirs as they go, and a missing file is
/// no error. When none is left, Python is not asked for at all.
pub fn suites() -> Vec<Suite> {
    let project = project();
    let tests = project.join("tests");
    let mut found = Vec::new();
    let plain = |label: &str, words: Vec<String>, folder: &Path| Suite { label: label.into(), commands: vec![words], folder: folder.to_path_buf(), env: Vec::new(), cargo: None };
    if tests.join("iphone-tests.sh").is_file() {
        found.push(plain("iphone", vec!["bash".into(), tests.join("iphone-tests.sh").display().to_string()], &project));
    }
    if project.join("ConveyorCheck").is_dir() {
        found.push(plain(
            "package",
            strings(&["swift", "test", "--package-path"]).into_iter().chain([project.join("ConveyorCheck").display().to_string()]).collect(),
            &project,
        ));
    }
    let builder = [tests.join("ios-run-tests.sh"), hitl_host_core::paths::expand("~/Code/scripts/tests/ios-run-tests.sh")].into_iter().find(|path| path.is_file());
    if let Some(builder) = builder {
        let folder = builder.parent().and_then(Path::parent).unwrap_or(&project).to_path_buf();
        found.push(plain("builder", vec!["bash".into(), builder.display().to_string()], &folder));
    }
    let target = cargo_target(&project);
    for name in testable_crates(&project) {
        let manifest = project.join("rust").join(&name).join("Cargo.toml").display().to_string();
        found.push(Suite {
            label: format!("cargo-{name}"),
            commands: vec![strings(&["cargo", "test", "--offline", "--locked", "--manifest-path"]).into_iter().chain([manifest]).collect()],
            folder: project.clone(),
            // Line tables are enough for a failure's place; full debug info only made the build and its link slower.
            env: vec![("CARGO_TARGET_DIR".into(), target.display().to_string()), ("CARGO_PROFILE_DEV_DEBUG".into(), "line-tables-only".into())],
            cargo: Some(project.join("rust").join(&name)),
        });
    }
    found
}

/// Whether a text matches a pattern of `fnmatch`: `*` any run, `?` one character, `[abc]` or `[!abc]` one of them.
fn fnmatch(text: &str, pattern: &str) -> bool {
    fn matches(text: &[char], pattern: &[char]) -> bool {
        match pattern.first() {
            None => text.is_empty(),
            Some('*') => (0..=text.len()).any(|skip| matches(&text[skip..], &pattern[1..])),
            Some('?') => !text.is_empty() && matches(&text[1..], &pattern[1..]),
            Some('[') => match pattern.iter().position(|c| *c == ']').filter(|end| *end > 1) {
                Some(end) if !text.is_empty() => {
                    let (negate, set) = match pattern[1] {
                        '!' => (true, &pattern[2..end]),
                        _ => (false, &pattern[1..end]),
                    };
                    let mut hit = false;
                    let mut at = 0;
                    while at < set.len() {
                        if at + 2 < set.len() && set[at + 1] == '-' {
                            hit |= set[at] <= text[0] && text[0] <= set[at + 2];
                            at += 3;
                        } else {
                            hit |= set[at] == text[0];
                            at += 1;
                        }
                    }
                    hit != negate && matches(&text[1..], &pattern[end + 1..])
                }
                Some(_) => false,
                None => !text.is_empty() && text[0] == '[' && matches(&text[1..], &pattern[1..]),
            },
            Some(c) => !text.is_empty() && text[0] == *c && matches(&text[1..], &pattern[1..]),
        }
    }
    matches(&text.chars().collect::<Vec<_>>(), &pattern.chars().collect::<Vec<_>>())
}

/// The suites that check these files: a file no pattern knows takes every suite of the core, as does one of the
/// shared code. A suite run only by its name is never taken.
pub fn touched(paths: &[String], names: &[String]) -> BTreeSet<String> {
    let core: BTreeSet<&String> = names.iter().filter(|name| !OWN_CHANGES_ONLY.contains(&name.as_str())).collect();
    let mut chosen: BTreeSet<String> = BTreeSet::new();
    for path in paths {
        let found = table::SUITES_OF.iter().find(|(pattern, _)| fnmatch(path, pattern)).map(|(_, suites)| *suites).unwrap_or("*");
        if found == "*" {
            chosen.extend(core.iter().map(|name| name.to_string()));
        } else {
            chosen.extend(found.split_whitespace().filter(|name| names.iter().any(|known| known == name)).map(str::to_string));
        }
    }
    chosen.into_iter().collect()
}

/// The files changed since this tree left main, committed or not; None when git cannot say.
pub fn changed_paths() -> Option<Vec<String>> {
    let project = project();
    let git = |words: &[&str]| -> Option<String> {
        let done = run("git".to_string(), &words.iter().map(|word| word.to_string()).collect::<Vec<_>>(), &Options::new("testing.git").cwd(&project)).ok()?;
        done.success().then(|| done.out())
    };
    let base = ["origin/main", "main"].iter().find_map(|reference| git(&["merge-base", "HEAD", reference]).map(|found| found.trim().to_string()).filter(|found| !found.is_empty()))?;
    let (changed, untracked) = (git(&["diff", "--name-only", &base])?, git(&["ls-files", "--others", "--exclude-standard"])?);
    let all: BTreeSet<String> = (changed + &untracked).lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_string).collect();
    Some(all.into_iter().collect())
}

fn digits_at(text: &str, start: usize) -> Option<(i64, usize)> {
    let digits: String = text[start..].chars().take_while(char::is_ascii_digit).collect();
    digits.parse::<i64>().ok().map(|number| (number, start + digits.len()))
}

/// How many checks a command ran, in whichever way it says so; the largest number when it says it more than once.
/// A crate's tests are said once for each of its test programs: those are added up.
pub fn count(output: &str) -> Option<i64> {
    let mut found: Vec<i64> = Vec::new();
    let after = |prefix: &str, rest_starts: &[&str], found: &mut Vec<i64>| {
        for (at, _) in output.match_indices(prefix) {
            if let Some((number, end)) = digits_at(output, at + prefix.len()) {
                let rest = &output[end..];
                let rest = rest.strip_prefix(" test").map(|tail| tail.strip_prefix('s').unwrap_or(tail));
                if let Some(rest) = rest {
                    if rest_starts.iter().any(|start| rest.starts_with(start)) {
                        found.push(number);
                    }
                }
            }
        }
    };
    after("Ran ", &[" in"], &mut found);
    after("Executed ", &[", with 0 failures"], &mut found);
    for line in output.lines() {
        if let Some(number) = line.strip_prefix("1..").and_then(|rest| rest.parse::<i64>().ok()) {
            found.push(number);
        }
    }
    // "N tests in M suites passed"
    let mut start = 0;
    while start < output.len() {
        let Some(offset) = output[start..].find(|c: char| c.is_ascii_digit()) else { break };
        let at = start + offset;
        let Some((number, end)) = digits_at(output, at) else { break };
        let rest = output[end..].strip_prefix(" test").map(|tail| tail.strip_prefix('s').unwrap_or(tail));
        if let Some(rest) = rest.and_then(|rest| rest.strip_prefix(" in ")) {
            if let Some((_, after_suites)) = digits_at(rest, 0) {
                let tail = rest[after_suites..].strip_prefix(" suite").map(|tail| tail.strip_prefix('s').unwrap_or(tail));
                if tail.is_some_and(|tail| tail.starts_with(" passed")) {
                    found.push(number);
                }
            }
        }
        start = end;
    }
    let cargo: i64 = output
        .lines()
        .filter_map(|line| line.strip_prefix("test result: ok. "))
        .filter_map(|rest| digits_at(rest, 0).filter(|(_, end)| rest[*end..].starts_with(" passed")).map(|(number, _)| number))
        .sum();
    if cargo > 0 {
        found.push(cargo);
    }
    found.into_iter().max()
}

fn last_chars(text: &str, count: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    chars[chars.len().saturating_sub(count)..].iter().collect()
}

/// What a failed command says of why. The end of its output alone is not that: the output is what the command printed
/// and then what it said on stderr, and cargo's last lines ("Running ...", "error: test failed") come after the text
/// of the failure, which a long run of several test programs pushed out of the last 4000 characters. So the lines of
/// a failure come first (the failed tests, their panics, a signal that ended a program, an error of the build), each
/// with the lines after it, and the end of the output after them.
fn failure_excerpt(output: &str, count: usize) -> String {
    let lines: Vec<&str> = output.lines().collect();
    let mut picked = vec![false; lines.len()];
    for (at, line) in lines.iter().enumerate() {
        let failure = (line.starts_with("---- ") && line.ends_with(" ----"))
            || (line.starts_with("test ") && line.ends_with("FAILED"))
            || line.contains("panicked at")
            || line.contains("didn't exit successfully")
            || line.starts_with("error:")
            || line.starts_with("error[");
        if failure {
            picked[at..(at + 12).min(lines.len())].iter_mut().for_each(|flag| *flag = true);
        }
    }
    let mut found = String::new();
    let mut gap = false;
    for (at, line) in lines.iter().enumerate() {
        if picked[at] {
            if gap && !found.is_empty() {
                found.push_str("…\n");
            }
            gap = false;
            found.push_str(line);
            found.push('\n');
        } else {
            gap = true;
        }
    }
    if found.is_empty() {
        return last_chars(output, count);
    }
    format!("{}\n--- конец вывода ---\n{}", last_chars(&found, count * 3 / 4), last_chars(output.trim_end(), count / 4))
}

/// How many commands run at once: all of them, or the budget `CONVEYOR_TEST_WORKERS` gives. Err on one that is
/// not a positive integer; an empty one is none.
fn budget(value: Option<&str>) -> Result<Option<usize>, ()> {
    let Some(text) = value.filter(|text| !text.is_empty()) else { return Ok(None) };
    let digits = text.trim().strip_prefix('+').unwrap_or(text.trim()).replace('_', "");
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err(());
    }
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return Err(());
    }
    Ok(Some(if digits.len() > 18 { usize::MAX } else { digits.parse().map_err(|_| ())? }))
}

/// The made-up phone and chats of the suites must not be the real ones of whoever runs this.
fn suite_environment(parent: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut environment: BTreeMap<String, String> = parent
        .iter()
        .filter(|(key, _)| !["CONVEYOR_", "CLAUDE", "CODEX_", "OPENCODE"].iter().any(|prefix| key.starts_with(prefix)) && key.as_str() != "AI_AGENT")
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    environment.insert("CONVEYOR_JOURNAL".into(), "0".into()); // fixture suites opt in explicitly; never journal into a user's home
    // Keep the already installed build tools/cache, not their HOME-dependent rustup fallback. This selects no new
    // toolchain and changes no config.
    let real = parent.get("HOME").cloned().unwrap_or_default();
    for (key, child) in [("RUSTUP_HOME", ".rustup"), ("CARGO_HOME", ".cargo")] {
        environment.entry(key.into()).or_insert_with(|| format!("{real}/{child}"));
    }
    environment
}

type Done = (bool, String);

/// One command in a private home of its own, which goes when it ends. A forgotten override (including a child that
/// drops CONVEYOR_HOME) must fall back to a private home. The program of hitl, which the suites that run the
/// command build once per version, is the one every command shares.
fn run_command(command: &[String], folder: &Path, environment: &BTreeMap<String, String>, extra: &[(String, String)], shared: &Path) -> Done {
    run_command_for(command, folder, environment, extra, shared, None)
}

/// `run_command`, ended at `timeout` seconds when one is given.
fn run_command_for(command: &[String], folder: &Path, environment: &BTreeMap<String, String>, extra: &[(String, String)], shared: &Path, timeout: Option<f64>) -> Done {
    let made = (|| -> std::io::Result<PathBuf> {
        static NUMBER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let base = std::fs::canonicalize(std::env::temp_dir())?;
        loop {
            let number = NUMBER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let folder = base.join(format!("conveyor-suite-{}-{number}", std::process::id()));
            match std::fs::create_dir(&folder) {
                Ok(()) => return Ok(folder),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let _ = std::fs::remove_dir_all(&folder);
                }
                Err(error) => return Err(error),
            }
        }
    })();
    let home = match made {
        Ok(home) => home,
        Err(error) => return (false, format!("не получилось сделать закрытый дом для набора: {error}")),
    };
    let cache = home.join("cache");
    let _ = std::fs::create_dir_all(&cache);
    let _ = std::fs::create_dir_all(shared);
    let _ = std::os::unix::fs::symlink(shared, cache.join("hitl-host"));
    // The program of hitl too, built once per version of its source: with a cache of its own, every test that makes
    // the archive built it from nothing, a minute each.
    if let Some(hitl) = shared.parent().map(|parent| parent.join("hitl-distribution")) {
        let _ = std::fs::create_dir_all(&hitl);
        let _ = std::os::unix::fs::symlink(&hitl, cache.join("hitl-distribution"));
    }
    let mut options = Options::new("testing.run").cwd(folder);
    if let Some(seconds) = timeout {
        options = options.timeout(seconds);
    }
    options.clear_env = true;
    let mut all = environment.clone();
    let put = |all: &mut BTreeMap<String, String>, key: &str, value: String| {
        all.insert(key.to_string(), value);
    };
    put(&mut all, "HOME", home.display().to_string());
    put(&mut all, "CFFIXED_USER_HOME", home.display().to_string());
    put(&mut all, "XDG_CONFIG_HOME", home.join("config").display().to_string());
    put(&mut all, "XDG_DATA_HOME", home.join("data").display().to_string());
    put(&mut all, "XDG_CACHE_HOME", cache.display().to_string());
    all.retain(|name, _| name != "HITL_HOME");
    put(&mut all, "CONVEYOR_HOME", home.join(".conveyor").display().to_string());
    put(&mut all, "CONVEYOR_WINDOW", String::new());
    put(&mut all, "CONVEYOR_NOTIFY", "0".into());
    for (key, value) in extra {
        put(&mut all, key, value.clone());
    }
    for (key, value) in &all {
        options = options.env(key, value);
    }
    let done = run(command[0].clone(), &command[1..], &options);
    let _ = std::fs::remove_dir_all(&home);
    match done {
        Ok(done) if done.timed_out => (false, format!("{}{}\nЗавис: остановлен через {:.0} с.\n", done.out(), done.err(), timeout.unwrap_or(0.0))),
        Ok(done) => (done.success(), done.out() + &done.err()),
        Err(error) => (false, format!("{} не запустилась: {error}", command[0])),
    }
}

fn tool_missing(tool: &str) -> bool {
    which(tool).is_none() && !Path::new(tool).exists()
}

/// Runs the suites chosen by name (an empty set is every suite), up to the budget of commands at a time, and says one
/// line about each, in order, to `say_line`. 0 when everything passed.
pub fn run_suites_in(parent: &BTreeMap<String, String>, known: &[Suite], wanted: &BTreeSet<String>, say_line: &mut dyn FnMut(String)) -> i32 {
    let chosen: Vec<&Suite> = known.iter().filter(|suite| wanted.is_empty() || wanted.contains(suite.name())).collect();
    // Every command of a suite at once, as before; and the tests of the crates as many at a time as the Mac has cores.
    let cores = std::thread::available_parallelism().map(usize::from).unwrap_or(8);
    let mut workers = chosen.iter().map(|suite| if suite.cargo.is_some() { 1 } else { suite.commands.len() }).sum::<usize>().max(cores).max(1);
    match budget(parent.get("CONVEYOR_TEST_WORKERS").map(String::as_str)) {
        Err(()) => {
            say("hitl test: CONVEYOR_TEST_WORKERS must be a positive integer");
            return EXIT_REFUSED;
        }
        Ok(Some(limit)) => workers = workers.min(limit),
        Ok(None) => {}
    }
    let environment = suite_environment(parent);
    let shared = shared_folder().join("xdg").join("hitl-host-core");
    let mut times = Times::load();
    // Every command of every suite that has its tool, and the build of every crate's test programs, queued; a build
    // that is done queues the tests it made, one each.
    let mut queue: Vec<Job> = Vec::new();
    let mut pending: Vec<usize> = vec![0; chosen.len()];
    let mut skipped: Vec<bool> = vec![false; chosen.len()];
    // The crates are built together, by one cargo for the workspace: each library once, every core busy.
    let mut crates: Vec<(PathBuf, usize)> = Vec::new();
    let mut crates_env: Vec<(String, String)> = Vec::new();
    for (index, suite) in chosen.iter().enumerate() {
        if let Some(crate_folder) = &suite.cargo {
            if tool_missing("cargo") {
                skipped[index] = true;
                continue;
            }
            pending[index] = 1;
            crates.push((crate_folder.clone(), index));
            crates_env = suite.env.clone();
            continue;
        }
        if suite.commands.first().map(|command| tool_missing(&command[0])).unwrap_or(true) {
            skipped[index] = true;
            continue;
        }
        for command in &suite.commands {
            pending[index] += 1;
            queue.push(Job { suite: index, work: Work::Command(command.clone()), folder: suite.folder.clone(), extra: suite.env.clone(), weight: f64::MAX / 2.0, order: queue.len() });
        }
    }
    if let Some((first, index)) = crates.first().cloned() {
        let folder = first.parent().map(Path::to_path_buf).unwrap_or(first);
        queue.push(Job { suite: index, work: Work::Build(crates), folder, extra: crates_env, weight: f64::MAX, order: queue.len() });
    }
    let file = parent.get(hitl_host_core::progress::FILE_SETTING).filter(|file| !file.is_empty()).map(PathBuf::from);
    let counted = Count { file, ..Count::default() };
    let board = Arc::new((Mutex::new(Board { queue, running: 0, pending, outputs: vec![Vec::new(); chosen.len()], took: Vec::new(), known: times.clone(), count: counted }), Condvar::new()));
    let environment = Arc::new(environment);
    let handles: Vec<_> = (0..workers)
        .map(|_| {
            let (board, environment, shared) = (board.clone(), environment.clone(), shared.clone());
            std::thread::spawn(move || work(&board, &environment, &shared))
        })
        .collect();
    let mut failed = 0;
    for (index, suite) in chosen.iter().enumerate() {
        if skipped[index] {
            let tool = if suite.cargo.is_some() { "cargo" } else { suite.commands.first().map(|command| command[0].as_str()).unwrap_or("") };
            say_line(format!("пропущен  {}: нет команды {tool}", suite.label));
            continue;
        }
        let done: Vec<(Done, bool)> = {
            let (lock, signal) = &*board;
            let mut held = lock.lock().unwrap();
            while held.pending[index] > 0 {
                held = signal.wait(held).unwrap();
            }
            std::mem::take(&mut held.outputs[index])
        };
        if done.iter().all(|((passed, _), _)| *passed) {
            let counts: Vec<Option<i64>> = done.iter().filter(|(_, counted)| *counted).map(|((_, output), _)| count(output)).collect();
            let total = if counts.contains(&None) { "?".to_string() } else { counts.iter().flatten().sum::<i64>().to_string() };
            say_line(format!("ок        {}: {total}", suite.label));
        } else {
            failed += 1;
            let output = done.iter().filter(|((passed, _), _)| !passed).map(|((_, output), _)| output.as_str()).collect::<Vec<_>>().join("\n");
            say_line(format!("НЕ ПРОШЁЛ {}\n{}", suite.label, failure_excerpt(&output, 4000)));
        }
    }
    for handle in handles {
        let _ = handle.join();
    }
    board.0.lock().unwrap().count.tell(true);
    let took = std::mem::take(&mut board.0.lock().unwrap().took);
    times.update(took);
    times.save();
    if failed > 0 { 1 } else { 0 }
}

/// The places for tests on this Mac, which every run shares: a test runs in one of them, as many as the Mac has cores,
/// whichever runs started them. Each run took every core before, and three chats testing at once loaded the Mac five
/// times over («я по 20 минут их жду с очень горячим маком»). Set by `main` only: the crate's own tests of the runner
/// run without them.
static PLACES: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// A file held locked for as long as it is kept: the lock goes with the file, and with the process that dies.
struct Held(#[allow(dead_code)] std::fs::File);

fn locked(path: &Path, wait: bool) -> Option<Held> {
    let file = std::fs::OpenOptions::new().create(true).append(true).open(path).ok()?;
    let how = if wait { libc::LOCK_EX } else { libc::LOCK_EX | libc::LOCK_NB };
    // SAFETY: an advisory lock on a file this function holds open; it goes with the file.
    (unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&file), how) } == 0).then_some(Held(file))
}

/// One of the places, when the run has them: the first free one, waited for.
fn take_place() -> Option<Held> {
    let folder = PLACES.get()?;
    let _ = std::fs::create_dir_all(folder);
    let cores = std::thread::available_parallelism().map(usize::from).unwrap_or(8);
    loop {
        if let Some(held) = (0..cores).find_map(|place| locked(&folder.join(place.to_string()), false)) {
            return Some(held);
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// The turn of a build of tests: one at a time on this Mac, as cargo takes every core for it.
fn turn_to_build() -> Option<Held> {
    let folder = PLACES.get()?;
    let _ = std::fs::create_dir_all(folder);
    locked(&folder.join("build"), true)
}

/// Test programs whose tests share a slow start made once in their process: run whole, one process for all their tests.
/// The tests of hitl's archive build the archive once for the file.
const WHOLE: [(&str, &str); 1] = [("hitl-distribution", "hitl")];

/// A test that has not ended after this long has hung: it is stopped and its suite fails, the run goes on.
const TEST_TIMEOUT: f64 = 600.0;

#[derive(Clone, Debug)]
enum Work {
    /// A command of a suite, run whole.
    Command(Vec<String>),
    /// The test programs of these crates (their folders, and the suite of each), built by one cargo in the workspace
    /// whose folder is the job's; their tests then queued one each.
    Build(Vec<(PathBuf, usize)>),
    /// One test of a test program, or every test of it when `test` is None; `key` names it in the times kept.
    Test { program: String, test: Option<String>, key: String },
}

struct Job {
    suite: usize,
    work: Work,
    folder: PathBuf,
    extra: Vec<(String, String)>,
    /// The longest go first, so that the run does not end waiting for one long test started last.
    weight: f64,
    order: usize,
}

struct Board {
    queue: Vec<Job>,
    running: usize,
    /// The jobs of each suite not done yet, queued or running.
    pending: Vec<usize>,
    /// What each job of a suite said, and whether its number of tests counts (a build's does not).
    outputs: Vec<Vec<(Done, bool)>>,
    took: Vec<(String, f64)>,
    known: Times,
    /// The tests queued, run and failed so far, and the file they are told to (`hitl queue` shows them).
    count: Count,
}

#[derive(Default)]
struct Count {
    total: u64,
    done: u64,
    failed: u64,
    file: Option<PathBuf>,
    written: Option<std::time::Instant>,
}

impl Count {
    /// Tells the file how far the run is: not more than twice a second, and always at the end.
    fn tell(&mut self, last: bool) {
        let Some(file) = &self.file else { return };
        if !last && self.written.is_some_and(|at| at.elapsed().as_secs_f64() < 0.5) {
            return;
        }
        hitl_host_core::progress::count(file, std::process::id() as i64, self.done, self.total, self.failed);
        self.written = Some(std::time::Instant::now());
    }
}

/// A worker: takes the heaviest job queued until none is queued and none runs that could queue more.
fn work(board: &Arc<(Mutex<Board>, Condvar)>, environment: &BTreeMap<String, String>, shared: &Path) {
    let (lock, signal) = &**board;
    loop {
        let job = {
            let mut held = lock.lock().unwrap();
            loop {
                if !held.queue.is_empty() {
                    break;
                }
                if held.running == 0 {
                    signal.notify_all();
                    return;
                }
                held = signal.wait(held).unwrap();
            }
            let at = (0..held.queue.len())
                .max_by(|one, other| {
                    let (one, other) = (&held.queue[*one], &held.queue[*other]);
                    one.weight.total_cmp(&other.weight).then(other.order.cmp(&one.order))
                })
                .unwrap_or(0);
            held.running += 1;
            held.queue.swap_remove(at)
        };
        let _place = match job.work {
            Work::Build(_) => None,
            _ => take_place(),
        };
        let started = std::time::Instant::now();
        let (done, more) = match &job.work {
            Work::Command(command) => (run_command(command, &job.folder, environment, &job.extra, shared), Vec::new()),
            Work::Build(crates) => {
                let known = lock.lock().unwrap().known.clone();
                let _turn = turn_to_build();
                build(&job, crates, environment, shared, &known)
            }
            Work::Test { program, test, .. } => {
                let mut command = vec![program.clone()];
                if let Some(test) = test {
                    command.extend([test.clone(), "--exact".to_string()]);
                }
                let mut extra = job.extra.clone();
                extra.push(("CARGO_MANIFEST_DIR".into(), job.folder.display().to_string()));
                (run_command_for(&command, &job.folder, environment, &extra, shared, Some(TEST_TIMEOUT)), Vec::new())
            }
        };
        let mut held = lock.lock().unwrap();
        if let Work::Test { key, .. } = &job.work {
            if done.0 {
                held.took.push((key.clone(), started.elapsed().as_secs_f64()));
            }
            held.count.done += 1;
            held.count.failed += u64::from(!done.0);
        }
        held.count.total += more.iter().filter(|queued| matches!(queued.work, Work::Test { .. })).count() as u64;
        held.count.tell(false);
        // A build is said only when it failed, under the first of its crates (the others point there): one that passed
        // is said by its tests.
        let suites: Vec<usize> = match &job.work {
            Work::Build(crates) => crates.iter().map(|(_, suite)| *suite).collect(),
            _ => vec![job.suite],
        };
        let counted = !matches!(job.work, Work::Build(_));
        if counted || !done.0 {
            held.outputs[job.suite].push((done.clone(), counted));
            for other in suites.iter().filter(|other| **other != job.suite) {
                held.outputs[*other].push(((false, "сборка тестов не прошла: её вывод выше, под первым крейтом\n".into()), false));
            }
        }
        for queued in &more {
            held.pending[queued.suite] += 1;
        }
        for mut queued in more {
            queued.order = held.queue.len() + held.running;
            held.queue.push(queued);
        }
        for suite in suites {
            held.pending[suite] -= 1;
        }
        held.running -= 1;
        signal.notify_all();
    }
}

/// Builds the test programs of the crates in one go and lists their tests: the jobs that run them, longest first by the
/// times kept from earlier runs. A failed build or listing is the suites' failure.
fn build(job: &Job, crates: &[(PathBuf, usize)], environment: &BTreeMap<String, String>, shared: &Path, known: &Times) -> (Done, Vec<Job>) {
    let manifest = job.folder.join("Cargo.toml").display().to_string();
    let mut command: Vec<String> =
        strings(&["cargo", "test", "--offline", "--locked", "--no-run", "--message-format=json-render-diagnostics", "--manifest-path"]).into_iter().chain([manifest]).collect();
    for (folder, _) in crates {
        command.extend(["-p".to_string(), package_name(folder)]);
    }
    let (built, output) = run_command(&command, &job.folder, environment, &job.extra, shared);
    let said: String = output.lines().filter(|line| !line.starts_with('{')).map(|line| format!("{line}\n")).collect();
    if !built {
        return ((false, said), Vec::new());
    }
    let mut jobs = Vec::new();
    for line in output.lines().filter(|line| line.starts_with('{')) {
        let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        if message["reason"] != "compiler-artifact" || message["profile"]["test"] != true {
            continue;
        }
        // Whose test program it is: the crate whose Cargo.toml cargo names.
        let manifest = PathBuf::from(message["manifest_path"].as_str().unwrap_or(""));
        let Some((crate_folder, suite)) = crates.iter().find(|(folder, _)| manifest.parent() == Some(folder.as_path())).cloned() else { continue };
        let crate_name = crate_folder.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default();
        let Some(program) = message["executable"].as_str() else { continue };
        let target = message["target"]["name"].as_str().unwrap_or("").to_string();
        let base = format!("{crate_name}/{target}");
        if WHOLE.iter().any(|(name, whole)| *name == crate_name && *whole == target) {
            let weight = known.get(&base).unwrap_or(60.0);
            jobs.push(Job { suite, work: Work::Test { program: program.into(), test: None, key: base }, folder: crate_folder.clone(), extra: job.extra.clone(), weight, order: 0 });
            continue;
        }
        let listing = [program.to_string(), "--list".into(), "--format".into(), "terse".into()];
        let (listed, names) = run_command(&listing, &crate_folder, environment, &job.extra, shared);
        if !listed {
            return ((false, format!("{program} --list:\n{names}")), Vec::new());
        }
        // A test marked #[ignore] is not run by `cargo test`: no process is started for it either.
        let ignored = [program.to_string(), "--list".into(), "--format".into(), "terse".into(), "--ignored".into()];
        let (_, ignored) = run_command(&ignored, &crate_folder, environment, &job.extra, shared);
        let ignored: BTreeSet<&str> = ignored.lines().filter_map(|line| line.strip_suffix(": test")).collect();
        for name in names.lines().filter_map(|line| line.strip_suffix(": test")).filter(|name| !ignored.contains(name)) {
            let key = format!("{base}/{name}");
            let weight = known.get(&key).unwrap_or(1.0);
            jobs.push(Job { suite, work: Work::Test { program: program.into(), test: Some(name.to_string()), key }, folder: crate_folder.clone(), extra: job.extra.clone(), weight, order: 0 });
        }
    }
    ((true, said), jobs)
}

/// How long each test took when it last passed, kept between runs in the shared folder: the order of the next run.
#[derive(Clone, Debug, Default)]
struct Times(BTreeMap<String, f64>);

impl Times {
    fn path() -> PathBuf {
        shared_folder().join("times.json")
    }

    fn load() -> Times {
        let text = std::fs::read_to_string(Times::path()).unwrap_or_default();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        Times(value.as_object().map(|all| all.iter().filter_map(|(key, value)| value.as_f64().map(|seconds| (key.clone(), seconds))).collect()).unwrap_or_default())
    }

    fn get(&self, key: &str) -> Option<f64> {
        self.0.get(key).copied()
    }

    fn update(&mut self, took: Vec<(String, f64)>) {
        self.0.extend(took);
    }

    /// Written whole into a file beside it, then put in its place: a run that reads it meanwhile reads one or the other.
    fn save(&self) {
        let path = Times::path();
        let _ = std::fs::create_dir_all(shared_folder());
        let text = serde_json::to_string(&self.0.iter().map(|(key, seconds)| (key.clone(), serde_json::json!((seconds * 100.0).round() / 100.0))).collect::<serde_json::Map<_, _>>()).unwrap_or_default();
        let made = path.with_extension(format!("json.{}", std::process::id()));
        if std::fs::write(&made, text).is_ok() {
            let _ = std::fs::rename(&made, &path);
        }
    }
}

fn run_suites(known: &[Suite], wanted: &BTreeSet<String>) -> i32 {
    run_suites_in(&env(), known, wanted, &mut |line| {
        use std::io::Write;
        println!("{line}");
        let _ = std::io::stdout().flush();
    })
}

/// A full run takes its turn on this Mac, one at a time; the others wait and are shown in `hitl queue`.
fn run_all(known: &[Suite], wanted: &BTreeSet<String>) -> i32 {
    let line = line::Line::conveyor_tests();
    let folder = project().file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default();
    let title = Some(chat_title(&env())).filter(|title| !title.is_empty()).unwrap_or_else(|| folder.clone());
    let pid = std::process::id();
    let taken = line.wait_turn(pid, &title, &folder, false, "Полный прогон тестов hitl ждёт своей очереди на этом Маке.", "");
    if taken != 0 {
        return taken;
    }
    // The run tells the line how far it is: `hitl queue` shows a bar of its tests.
    let file = hitl_host_core::progress::file(&line.folder());
    hitl_host_core::progress::step(&file, pid as i64, "тесты");
    let mut parent = env();
    parent.insert(hitl_host_core::progress::FILE_SETTING.into(), file.display().to_string());
    let done = run_suites_in(&parent, known, wanted, &mut |line| {
        use std::io::Write;
        println!("{line}");
        let _ = std::io::stdout().flush();
    });
    line.leave(pid);
    let _ = std::fs::remove_file(&file);
    done
}

/// Builds the program of hitl once into a folder every suite of this run shares and later runs keep, as the
/// window's program is. With a private cache each, every command of a run built it of its own, all at once, and under
/// that load a build outran its 50 seconds (his «да» of 2026-10-28, check 1028-182539-169).
fn share_rust() {
    // The real path: /var is a link to /private/var, and the suites' fences and their reads of a running program's
    // image name the real one.
    if var("CARGO_TARGET_DIR").is_none() {
        // SAFETY: nothing else is running yet; the suites are started after this.
        unsafe { std::env::set_var("CARGO_TARGET_DIR", shared_folder().join("rust")) };
    }
    let project = project();
    let xdg = shared_folder().join("xdg");
    let _ = std::fs::create_dir_all(&xdg);
    let options = Options::new("testing.share_conveyor").env("XDG_CACHE_HOME", &xdg.display().to_string()).timeout(900.0);
    let _ = run(project.join("hitl").display().to_string(), &strings(&["update", "--help"]), &options);
}

pub fn main(arguments: &[String]) -> i32 {
    if matches!(arguments.first().map(String::as_str), Some("-h" | "--help" | "help")) {
        print!("{USAGE}");
        return 0;
    }
    let _ = PLACES.set(shared_folder().join("places"));
    keep_cargo_targets(&project());
    let known = suites();
    let mut names: Vec<String> = Vec::new();
    for suite in &known {
        if !names.iter().any(|name| name == suite.name()) {
            names.push(suite.name().to_string());
        }
    }
    let project = project();
    let crates_here = all_crates(&project);
    let cargo_names: BTreeSet<String> = testable_crates(&project).iter().map(|name| format!("cargo-{name}")).collect();
    let core: BTreeSet<String> = names
        .iter()
        .filter(|name| !OWN_CHANGES_ONLY.contains(&name.as_str()) && !cargo_names.contains(*name))
        .cloned()
        .collect();
    let everything: BTreeSet<String> = core.union(&cargo_names).cloned().collect();
    let mut arguments: Vec<String> = arguments.to_vec();
    if arguments.is_empty() {
        let changed = changed_paths();
        let (chosen, cargo) = match &changed {
            Some(paths) => {
                let cargo = cargo_touched(paths, &crates_here, &dependency_map(&project, &crates_here));
                (touched(paths, &names), cargo.into_iter().map(|name| format!("cargo-{name}")).filter(|name| cargo_names.contains(name)).collect::<BTreeSet<_>>())
            }
            None => (core.clone(), BTreeSet::new()),
        };
        let count_of_changed = changed.as_ref().map(Vec::len).unwrap_or(0);
        if changed.is_none() {
            println!("Изменения против main не прочитать: все наборы.");
        } else if chosen.is_empty() && cargo.is_empty() {
            println!(
                "Изменено файлов против main: {count_of_changed}, ни один набор их не проверяет. Все наборы: {}",
                command(&["test", "--all"])
            );
            return 0;
        }
        if chosen != core {
            let listed: Vec<&str> = names.iter().filter(|name| chosen.contains(*name) || cargo.contains(*name)).map(String::as_str).collect();
            println!(
                "Наборы по изменениям против main ({count_of_changed} файлов): {}. Все наборы: {}",
                listed.join(", "),
                command(&["test", "--all"])
            );
            share_rust();
            return run_suites(&known, &chosen.union(&cargo).cloned().collect());
        }
        arguments = vec!["--all".into()];
    }
    if arguments == ["--all"] {
        share_rust();
        return run_all(&known, &everything);
    }
    let wanted: BTreeSet<String> = arguments.iter().cloned().collect();
    let unknown: Vec<&String> = wanted.iter().filter(|name| !names.contains(*name)).collect();
    if !unknown.is_empty() {
        say(&format!(
            "hitl test: нет набора «{}». Есть: {}",
            unknown.iter().map(|name| name.as_str()).collect::<Vec<_>>().join(", "),
            names.join(", ")
        ));
        return EXIT_REFUSED;
    }
    share_rust();
    run_suites(&known, &wanted)
}

/// Answers `command` with its arguments, or None when this module does not have it (yet).
pub fn dispatch(command: &str, rest: &[String]) -> Option<i32> {
    (command == "test").then(|| main(rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<String> {
        ["iphone", "package", "builder", "cargo-hitl", "cargo-hitl-tools", "cargo-hitl-phone", "cargo-hitl-claude"]
            .iter()
            .map(|name| name.to_string())
            .collect()
    }

    fn chosen(paths: &[&str]) -> BTreeSet<String> {
        touched(&paths.iter().map(|path| path.to_string()).collect::<Vec<_>>(), &names())
    }

    fn set(words: &[&str]) -> BTreeSet<String> {
        words.iter().map(|word| word.to_string()).collect()
    }

    // ---- the suites of a change: «надо сделать вместо этого минимальный набор тестов»

    #[test]
    fn the_project_is_the_checkout_the_call_is_made_in_and_else_the_scripts_own_root() {
        let top = std::fs::canonicalize(std::env::temp_dir()).unwrap().join(format!("conveyor-project-of-{}", std::process::id()));
        let inner = top.join("copy/rust/crate");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(top.join("copy/hitl"), "#!/bin/sh\n").unwrap();
        let live = PathBuf::from("/live/folder");
        assert_eq!(project_of(&inner, live.clone()), top.join("copy"), "from a folder inside the copy");
        assert_eq!(project_of(&top, live.clone()), live, "from a folder that is no checkout");
        let _ = std::fs::remove_dir_all(&top);
    }

    #[test]
    fn a_failure_is_told_by_its_own_lines_and_not_by_the_end_of_a_long_output() {
        let mut output = String::from("running 3 tests\ntest a ... ok\ntest b ... FAILED\n\nfailures:\n\n---- b stdout ----\n");
        output.push_str("thread 'b' panicked at tests/x.rs:9:5:\nthe reason\n");
        output.push_str(&"a line of what a test printed after\n".repeat(400));
        output.push_str("   Running tests/install.rs\nerror: test failed, to rerun pass `--test install`\n");
        let told = failure_excerpt(&output, 4000);
        assert!(told.contains("---- b stdout ----") && told.contains("the reason") && told.contains("test b ... FAILED"), "{told}");
        assert!(told.contains("error: test failed"), "{told}");
        assert!(told.chars().count() <= 4100, "{}", told.chars().count());
        assert_eq!(failure_excerpt("nothing like a failure\nat all\n", 4000), "nothing like a failure\nat all\n");
    }

    #[test]
    fn a_file_takes_its_own_suites() {
        assert_eq!(chosen(&["skills/hitl/SKILL.md"]), set(&["cargo-hitl-tools"]));
        assert_eq!(chosen(&["ConveyorCheck/Sources/ConveyorCheck/Store.swift"]), set(&["package"]));
        assert_eq!(chosen(&["tests/iphone-tests.sh", "ios-run.sh"]), set(&["iphone", "builder"]));
    }

    #[test]
    fn documents_take_none_and_the_entry_takes_every_crate() {
        assert_eq!(chosen(&["docs/decisions.md", "AGENTS.md", ".gitattributes", "rust/hitl-tools/src/update.rs"]), set(&[]));
        let all = chosen(&["hitl"]);
        assert!(all.contains("cargo-hitl") && all.iter().all(|name| name.starts_with("cargo-")));
        assert_eq!(chosen(&["somewhere/new.txt"]), all, "a file nobody knows takes every crate");
    }

    #[test]
    fn crate_files_use_cargo_dependencies_instead_of_the_static_table() {
        // A crate's files take no suite by the table: `cargo test` of the crate (and of what depends on it) is what a
        // change there runs.
        let names: Vec<String> = ["iphone", "cargo-hitl-host-core"].iter().map(|name| name.to_string()).collect();
        let paths = vec!["rust/hitl-host-core/src/lib.rs".to_string()];
        assert_eq!(touched(&paths, &names), set(&[]));
        let here: Vec<String> = ["hitl-host-core", "hitl-tools", "hitl-host"].iter().map(|name| name.to_string()).collect();
        let dependencies: BTreeMap<String, Vec<String>> =
            [("hitl-host-core", vec![]), ("hitl-tools", vec!["hitl-host-core"]), ("hitl-host", vec!["hitl-tools"])]
                .iter()
                .map(|(name, reached)| (name.to_string(), reached.iter().map(|one| one.to_string()).collect()))
                .collect();
        assert_eq!(cargo_touched(&paths, &here, &dependencies), set(&["hitl-host-core", "hitl-tools", "hitl-host"]));
    }

    #[test]
    fn the_crates_that_depend_on_a_crate_run_with_it() {
        let here: Vec<String> = ["hitl-host-core", "hitl-tools", "hitl-host", "hitl"].iter().map(|name| name.to_string()).collect();
        let dependencies: BTreeMap<String, Vec<String>> = [
            ("hitl-host-core", vec!["hitl"]),
            ("hitl-tools", vec!["hitl-host-core"]),
            ("hitl-host", vec!["hitl-tools", "hitl"]),
            ("hitl", vec![]),
        ]
        .iter()
        .map(|(name, reached)| (name.to_string(), reached.iter().map(|one| one.to_string()).collect()))
        .collect();
        let paths = |words: &[&str]| words.iter().map(|word| word.to_string()).collect::<Vec<_>>();
        assert_eq!(cargo_touched(&paths(&["rust/hitl-host-core/src/git.rs"]), &here, &dependencies), set(&["hitl-host-core", "hitl-tools", "hitl-host"]));
        assert_eq!(cargo_touched(&paths(&["rust/hitl-host/src/main.rs", "docs/x.md"]), &here, &dependencies), set(&["hitl-host"]));
        assert_eq!(cargo_touched(&paths(&["docs/decisions.md"]), &here, &dependencies), set(&[]));
    }

    #[test]
    fn hitls_build_and_release_take_their_crates_suite() {
        let here: Vec<String> = ["hitl-distribution", "hitl-tools", "hitl-host"].iter().map(|name| name.to_string()).collect();
        let dependencies: BTreeMap<String, Vec<String>> = [
            ("hitl-distribution", vec![]),
            ("hitl-tools", vec!["hitl-distribution"]),
            ("hitl-host", vec!["hitl-tools", "hitl-distribution"]),
        ]
        .iter()
        .map(|(name, reached)| (name.to_string(), reached.iter().map(|one| one.to_string()).collect()))
        .collect();
        for path in ["rust/hitl-distribution/src/release.rs", "rust/hitl-distribution/tests/hitl.rs"] {
            let paths = vec![path.to_string()];
            assert_eq!(cargo_touched(&paths, &here, &dependencies), set(&["hitl-distribution", "hitl-tools", "hitl-host"]), "{path}");
            assert_eq!(chosen(&[path]), set(&[]), "{path}");
        }
    }

    #[test]
    fn the_patterns_are_those_of_fnmatch() {
        assert!(fnmatch("tests/far-phone.swift", "tests/far*"));
        assert!(fnmatch("a/b/c", "a/*"));
        assert!(fnmatch("ab", "a?") && !fnmatch("abc", "a?"));
        assert!(fnmatch("b", "[abc]") && !fnmatch("d", "[abc]") && fnmatch("d", "[!abc]"));
        assert!(!fnmatch("hitl/x", "hitl"));
    }

    // ---- how a run says how many it ran

    #[test]
    fn the_count_is_read_in_whichever_way_a_command_says_it() {
        assert_eq!(count("..\nRan 12 tests in 0.5s\n\nOK"), Some(12));
        assert_eq!(count("1..7\nok 1\n"), Some(7));
        assert_eq!(count("Executed 5 tests, with 0 failures (0 unexpected)"), Some(5));
        assert_eq!(count("3 tests in 2 suites passed"), Some(3));
        assert_eq!(count("nothing said"), None);
        assert_eq!(count("running 3 tests\ntest result: ok. 3 passed; 0 failed\ntest result: ok. 0 passed; 0 failed\nRan 1 test in 0.001s"), Some(3));
        assert_eq!(count("test result: ok. 4 passed; 0\ntest result: ok. 2 passed; 0\n"), Some(6), "a crate's test programs are added up");
    }

    #[test]
    fn a_budget_is_a_positive_integer() {
        assert_eq!(budget(None), Ok(None));
        assert_eq!(budget(Some("")), Ok(None));
        assert_eq!(budget(Some("2")), Ok(Some(2)));
        assert_eq!(budget(Some("999999999999999999999999999999999999999")), Ok(Some(usize::MAX)));
        for bad in ["0", "-1", "-0", "text", "1.5", " ", "nan"] {
            assert_eq!(budget(Some(bad)), Err(()), "{bad}");
        }
    }

    // ---- the run itself, with made-up suites

    struct Folder(PathBuf);

    impl Folder {
        fn new(label: &str) -> Folder {
            let path = std::fs::canonicalize(std::env::temp_dir()).unwrap().join(format!("conveyor-testing-{label}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Folder(path)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// An inert command: it notes its private home, how many run at the same moment, and says it ran one test.
    fn inert(folder: &Path, id: usize, code: i32) -> Vec<String> {
        let script = format!(
            "mkdir -p \"{dir}/running\"; touch \"{dir}/running/{id}\"; sleep 0.4; ls \"{dir}/running\" | wc -l > \"{dir}/seen-{id}\"; \
             echo \"$HOME|$CONVEYOR_HOME|$CFFIXED_USER_HOME|$CONVEYOR_JOURNAL|${{CONVEYOR_TEST_WORKERS-unset}}\" > \"{dir}/env-{id}\"; \
             rm \"{dir}/running/{id}\"; echo 'Ran 1 test in 0.001s'; exit {code}",
            dir = folder.display()
        );
        vec!["/bin/sh".into(), "-c".into(), script]
    }

    fn suites_of_six(folder: &Path, failing: bool) -> Vec<Suite> {
        let commands: Vec<Vec<String>> = (0..6).map(|id| inert(folder, id, if failing && id == 1 { 9 } else { 0 })).collect();
        vec![
            Suite { label: "split".into(), commands: commands[..4].to_vec(), folder: folder.to_path_buf(), env: vec![], cargo: None },
            Suite { label: "last".into(), commands: commands[4..].to_vec(), folder: folder.to_path_buf(), env: vec![], cargo: None },
        ]
    }

    fn peak_of(folder: &Path) -> usize {
        (0..6).filter_map(|id| std::fs::read_to_string(folder.join(format!("seen-{id}"))).ok()).filter_map(|text| text.trim().parse::<usize>().ok()).max().unwrap_or(0)
    }

    fn run_six(label: &str, workers: Option<&str>, failing: bool) -> (usize, i32, Vec<String>, Folder) {
        let folder = Folder::new(label);
        let mut parent: BTreeMap<String, String> = std::env::vars().collect();
        parent.remove("CONVEYOR_TEST_WORKERS");
        parent.insert("HOME".into(), "/nonexistent-operator-home".into());
        if let Some(workers) = workers {
            parent.insert("CONVEYOR_TEST_WORKERS".into(), workers.into());
        }
        let mut said = Vec::new();
        let code = run_suites_in(&parent, &suites_of_six(&folder.0, failing), &set(&["split", "last"]), &mut |line| said.push(line));
        let peak = peak_of(&folder.0);
        (peak, code, said, folder)
    }

    fn assert_private_homes(folder: &Folder) {
        let mut homes = BTreeSet::new();
        for id in 0..6 {
            let text = std::fs::read_to_string(folder.0.join(format!("env-{id}"))).unwrap();
            let parts: Vec<&str> = text.trim().split('|').collect();
            let home = PathBuf::from(parts[0]);
            assert!(!home.exists(), "private home removed only after its command finished");
            assert!(parts[1].starts_with(parts[0]), "the state is inside the private home: {text}");
            assert_eq!(parts[2], parts[0]);
            assert_eq!(parts[3], "0");
            assert_eq!(parts[4], "unset", "the parent's budget must not reach nested fixtures");
            homes.insert(home);
        }
        assert_eq!(homes.len(), 6);
    }

    #[test]
    fn an_explicit_budget_limits_commands_without_dropping_split_parts() {
        let (peak, code, said, folder) = run_six("budget2", Some("2"), false);
        assert_eq!((peak, code), (2, 0));
        assert_eq!(said, ["ок        split: 4", "ок        last: 2"]);
        assert_private_homes(&folder);
    }

    #[test]
    fn unset_empty_and_larger_budgets_retain_all_parallel_commands() {
        for (label, budget) in [("unset", None)] {
            let (peak, code, said, folder) = run_six(label, budget, false);
            assert_eq!((peak, code), (6, 0), "{label}");
            assert_eq!(said, ["ок        split: 4", "ок        last: 2"]);
            assert_private_homes(&folder);
        }
    }

    #[test]
    fn a_failure_still_finishes_the_remaining_commands_and_cleans_the_homes() {
        let (peak, code, said, folder) = run_six("failing", Some("2"), true);
        assert_eq!((peak, code), (2, 1));
        assert!(said[0].starts_with("НЕ ПРОШЁЛ split\n"), "{said:?}");
        assert_eq!(said[1], "ок        last: 2");
        assert_private_homes(&folder);
    }

    #[test]
    fn an_invalid_budget_refuses_before_any_command_runs() {
        for bad in ["0", "-1", "-0", "text", "1.5", " ", "nan"] {
            let folder = Folder::new("invalid");
            let mut parent: BTreeMap<String, String> = std::env::vars().collect();
            parent.insert("CONVEYOR_TEST_WORKERS".into(), bad.into());
            let mut said = Vec::new();
            let code = run_suites_in(&parent, &suites_of_six(&folder.0, false), &set(&["split", "last"]), &mut |line| said.push(line));
            assert_eq!(code, EXIT_REFUSED, "{bad}");
            assert!(said.is_empty());
            assert!(!folder.0.join("running").exists(), "no command ran for {bad}");
        }
    }

    #[test]
    fn a_suite_whose_tool_is_missing_is_skipped_with_a_line() {
        let suite = Suite { label: "lost".into(), commands: vec![vec!["/nonexistent/tool".into()]], folder: PathBuf::from("/"), env: vec![], cargo: None };
        let mut said = Vec::new();
        let code = run_suites_in(&BTreeMap::new(), &[suite], &set(&[]), &mut |line| said.push(line));
        assert_eq!((code, said), (0, vec!["пропущен  lost: нет команды /nonexistent/tool".to_string()]));
    }

    #[test]
    fn the_suites_are_the_crates_and_the_swift_and_shell_ones_that_are_there() {
        let known = suites();
        assert!(known.iter().any(|suite| suite.label == "cargo-hitl-tools" && suite.commands[0][1] == "test"));
        assert!(known.iter().all(|suite| suite.label.starts_with("cargo-") || ["iphone", "package", "builder"].contains(&suite.label.as_str())));
    }
}
