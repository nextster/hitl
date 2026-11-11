//! `hitl update`: brings the folder every chat runs these tools from to what is merged into main.
//!
//! Moved from `update.py` (see `docs/rust-move.md`). A change of the tools is merged on GitHub, and the chats go on
//! running the files that lie in the project's folder until those are brought up to it. There is one version, that
//! folder («всегда будет одна актуальная версия которая лежит в репозитории»): this command only moves it forward,
//! in a way no chat sees half of.

use hitl_host_core::process::{Options, Output, run};
use hitl_host_core::{EXIT_BUSY, EXIT_REFUSED, chat_tag, home, plural, read_json, root, say, var};
use serde_json::Value;
use std::collections::BTreeSet;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::Duration;

const USAGE: &str = "Brings the folder every chat runs hitl from to what is merged into main.

Usage:
  hitl update [--dry-run] [--no-wait]

Run it after a change of these tools is merged. It fetches main and puts its
files into the project's folder so that no chat sees a half of the change:

  - The window's sources go in as one swap of the folder, and the window is
    built from them beforehand: a waiting `hitl check` finds the new
    window ready and never builds a mix of two versions.
  - The program's sources (`rust/`) go in the same way: main's program is built
    beforehand, so that the first call after this finds it ready, and the folder
    is swapped at once, so that no call builds a mix of two versions.
  - Every other file goes in by a rename.
  - Work that lies there uncommitted is another chat's and is not touched.
    When main changes a file that has such work in it, or the window's folder
    has any, the command refuses and names the files.
  - A new window takes the place of the one on the screen. So while the window
    shows another chat's check, the command waits for him to end that check and
    goes on between it and the next one, which comes up in the new window. It
    does not wait for the caller's own check, nor for the window to be empty.

  --dry-run   Say what would change, change nothing.
  --no-wait   Exit with 75 instead of waiting for the window.

Exit codes: 0 the folder is at main; 10 refused, with the reason; 64 a wrong
call; 75 the window is busy and --no-wait was given.
";

/// Refused, with the reason: 10, not the 64 of a wrong call.
const REFUSED: i32 = 10;
const WINDOW: &str = "window/";
const PROGRAM: &str = "rust/";

/// Why the folder was not brought to main, and the exit code that says so.
#[derive(Debug)]
pub struct Stop {
    pub code: i32,
    pub message: String,
}

fn stop(code: i32, message: impl Into<String>) -> Stop {
    Stop { code, message: message.into() }
}

fn poll_seconds() -> f64 {
    var("CONVEYOR_UPDATE_POLL_SECONDS").and_then(|value| value.parse::<f64>().ok()).filter(|value| *value != 0.0).unwrap_or(2.0)
}

/// The folder the chats run the tools from: where the link among the shared scripts leads.
pub fn live_folder() -> PathBuf {
    if let Some(named) = var("CONVEYOR_LIVE").filter(|value| !value.is_empty()) {
        return PathBuf::from(named);
    }
    let link = hitl_host_core::paths::expand("~/Code/scripts/hitl");
    match std::fs::canonicalize(&link) {
        Ok(real) => real.parent().map(Path::to_path_buf).unwrap_or_else(root),
        Err(_) => root(),
    }
}

fn tail(text: &str, count: usize) -> String {
    let chars: Vec<char> = text.trim().chars().collect();
    chars[chars.len().saturating_sub(count)..].iter().collect()
}

pub struct Update {
    pub folder: PathBuf,
    pub head: String,
    pub target: String,
    /// What main changes: (how, path), as git names it: M, A or D.
    pub changed: Vec<(String, String)>,
}

impl Update {
    pub fn new(folder: &Path) -> Update {
        Update { folder: folder.to_path_buf(), head: String::new(), target: String::new(), changed: Vec::new() }
    }

    fn git(&self, arguments: &[&str], input: Option<&[u8]>) -> Result<Output, Stop> {
        let folder = self.folder.display().to_string();
        let mut all = vec!["-C", folder.as_str(), "-c", "core.quotePath=false"];
        all.extend_from_slice(arguments);
        let mut options = Options::new("update.git");
        if let Some(bytes) = input {
            options = options.input(bytes);
        }
        run("git", &all, &options).map_err(|error| stop(REFUSED, format!("git не запустился: {error}")))
    }

    /// git, and a refusal with what it said when it failed.
    fn checked(&self, arguments: &[&str]) -> Result<Output, Stop> {
        let done = self.git(arguments, None)?;
        if !done.success() {
            let first = arguments.iter().take(2).copied().collect::<Vec<_>>().join(" ");
            return Err(stop(REFUSED, format!("git {first} не прошёл: {}", tail(&done.err(), 300))));
        }
        Ok(done)
    }

    fn lines(&self, arguments: &[&str]) -> Result<Vec<String>, Stop> {
        Ok(self.checked(arguments)?.out().lines().filter(|line| !line.is_empty()).map(str::to_string).collect())
    }

    fn first(&self, arguments: &[&str]) -> Result<String, Stop> {
        let lines = self.lines(arguments)?;
        lines.into_iter().next().ok_or_else(|| stop(REFUSED, format!("git {} ничего не сказал", arguments.join(" "))))
    }

    fn paths(&self) -> Vec<&str> {
        self.changed.iter().map(|(_, path)| path.as_str()).collect()
    }

    fn touches_window(&self) -> bool {
        self.paths().iter().any(|path| path.starts_with(WINDOW))
    }

    fn touches_program(&self) -> bool {
        self.paths().iter().any(|path| path.starts_with(PROGRAM))
    }

    /// What main has that the folder has not. False when there is nothing to bring.
    pub fn look(&mut self) -> Result<bool, Stop> {
        if !self.folder.join(".git").exists() {
            return Err(stop(REFUSED, format!("{} не репозиторий: обновлять нечего.", self.folder.display())));
        }
        if !self.git(&["fetch", "--quiet", "origin", "main"], None)?.success() {
            return Err(stop(REFUSED, "не получилось забрать main с GitHub."));
        }
        let branch = self.first(&["rev-parse", "--abbrev-ref", "HEAD"])?;
        if branch != "main" {
            return Err(stop(
                REFUSED,
                format!("рабочая папка {} стоит на ветке {branch}, а не на main: её кто-то переключил.", self.folder.display()),
            ));
        }
        self.head = self.first(&["rev-parse", "HEAD"])?;
        self.target = self.first(&["rev-parse", "origin/main"])?;
        if self.head == self.target {
            return Ok(false);
        }
        if !self.git(&["merge-base", "--is-ancestor", &self.head, &self.target], None)?.success() {
            return Err(stop(REFUSED, "в рабочей папке есть коммиты, которых нет в main на GitHub: их сначала надо влить."));
        }
        let listing = self.lines(&["diff", "--name-status", "--no-renames", &self.head, &self.target])?;
        self.changed = listing
            .iter()
            .filter_map(|line| line.split_once('\t').map(|(how, path)| (how.to_string(), path.to_string())))
            .collect();
        Ok(true)
    }

    /// Refuses over work that lies in the folder uncommitted, where main changes the same files.
    pub fn others_work(&self) -> Result<(), Stop> {
        let touched: BTreeSet<String> = self.paths().iter().map(|path| path.to_string()).collect();
        let set = |arguments: &[&str]| -> Result<BTreeSet<String>, Stop> { Ok(self.lines(arguments)?.into_iter().collect()) };
        let staged = set(&["diff", "--cached", "--name-only"])?;
        let unstaged = set(&["diff", "--name-only"])?;
        let untracked = set(&["ls-files", "--others", "--exclude-standard"])?;
        let mut theirs: Vec<String> = staged.union(&unstaged).filter(|path| touched.contains(*path)).cloned().collect();
        // A file main adds, where one lies already that git does not know.
        let mut added: Vec<String> =
            self.changed.iter().filter(|(how, path)| how == "A" && untracked.contains(path)).map(|(_, path)| path.clone()).collect();
        added.sort();
        theirs.extend(added);
        let (window, program) = (self.touches_window(), self.touches_program());
        if window || program {
            // The window and the program go in as whole folders: whatever lies in them uncommitted would be gone.
            let mut inside: Vec<String> = staged
                .iter()
                .chain(unstaged.iter())
                .chain(untracked.iter())
                .filter(|path| ((window && path.starts_with(WINDOW)) || (program && path.starts_with(PROGRAM))) && !theirs.contains(*path))
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            inside.sort();
            theirs.extend(inside);
        }
        if !theirs.is_empty() {
            return Err(stop(
                REFUSED,
                format!(
                    "в рабочей папке лежит незакоммиченная работа в файлах, которые меняет main: {}. Она чужая, и эта команда её не трогает. \
                     Дождись, пока её закоммитят и вольют, и повтори; срочно — внеси свою правку в эти файлы руками, не затирая чужую.",
                    theirs.join(" ")
                ),
            ));
        }
        Ok(())
    }

    /// The check the window shows now, when it shows one.
    fn shown(&self) -> Option<Value> {
        let progress = read_json(&home().join("window").join("progress.json"))?;
        let id = progress.get("id").filter(|id| hitl_host_core::truthy(Some(id)))?;
        let name = id.as_str().map(str::to_string).unwrap_or_else(|| id.to_string());
        read_json(&home().join("queue").join(format!("{name}.json"))).filter(Value::is_object)
    }

    /// A new window takes the place of the one on the screen: not under a check of another chat that he may be
    /// answering. The command waits for that check to end and goes on between it and the next one, which then comes
    /// up in the new window. It does not wait for the window to be empty: with checks in a row that would be never
    /// («а если за выполненной проверкой будет очередь ещё то что произойдёт?»).
    fn wait_for_the_window(&self, wait: bool) -> Result<(), Stop> {
        let own = chat_tag();
        let Some(first) = self.shown() else { return Ok(()) };
        let text = |key: &str| first.get(key).and_then(Value::as_str).unwrap_or("").to_string();
        if !own.is_empty() && text("tag") == own {
            return Ok(());
        }
        let name = [text("tag"), format!("«{}»", text("title"))]
            .into_iter()
            .filter(|part| !part.trim_matches(|c| c == '«' || c == '»').is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if !wait {
            return Err(stop(
                EXIT_BUSY,
                format!(
                    "в окне проверок сейчас проверка другого чата ({name}): новое окно перезапустило бы её перед Проверяющим. \
                     Повтори, когда он её закончит, или без --no-wait."
                ),
            ));
        }
        say(&format!(
            "==> В окне проверок сейчас проверка другого чата ({name}). Жду, пока Проверяющий её закончит: окно не должно \
             перезапуститься у него под руками. Следующая проверка откроется уже в новом окне."
        ));
        loop {
            match self.shown() {
                Some(ticket) if ticket.get("id") == first.get("id") => std::thread::sleep(Duration::from_secs_f64(poll_seconds())),
                _ => return Ok(()),
            }
        }
    }

    fn content(&self, path: &str) -> Result<Vec<u8>, Stop> {
        Ok(self.checked(&["show", &format!("{}:{path}", self.target)])?.stdout)
    }

    /// The window's sources as main has them: built, then put in place as one swap of the folder.
    fn place_window(&self, work: &Path) -> Result<(), Stop> {
        let archive = self.checked(&["archive", &self.target, "window"])?.stdout;
        let untar = run("tar", &["-x", "-C", &work.display().to_string()], &Options::new("update.tar").input(&archive))
            .map_err(|error| stop(REFUSED, format!("tar не запустился: {error}")))?;
        if !untar.success() {
            return Err(stop(REFUSED, format!("tar не прошёл: {}", tail(&untar.err(), 300))));
        }
        let new = work.join("window");
        if var("CONVEYOR_WINDOW").is_none() {
            let mut sources: Vec<PathBuf> = std::fs::read_dir(&new)
                .map_err(|error| stop(REFUSED, format!("{}: {error}", new.display())))?
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|extension| extension == "swift"))
                .collect();
            sources.sort();
            if let Err(said) = hitl::window::built_panel(&sources, false) {
                say(&said);
                return Err(stop(REFUSED, "окно проверок из main не собралось: рабочая папка не тронута."));
            }
        }
        let old = self.folder.join("window");
        let failed = |error: std::io::Error| stop(REFUSED, format!("не получилось поставить окно на место: {error}"));
        if !old.exists() {
            return std::fs::rename(&new, &old).map_err(failed);
        }
        if swap(&new, &old) {
            return Ok(());
        }
        // No swap on this system: two renames, with a moment of no folder between them.
        std::fs::rename(&old, work.join("window-before")).map_err(failed)?;
        std::fs::rename(&new, &old).map_err(failed)
    }

    /// The program's sources as main has them: built, then put in place as one swap of the folder. The script of the
    /// entry names a build by the contents of its sources, so what it builds here is what the folder finds after the
    /// swap, and no chat waits for a build. (Python's `place_core` did this for `rust/hitl`; now it is all of `rust/`.)
    fn place_program(&self, work: &Path) -> Result<(), Stop> {
        let tree = work.join("program");
        std::fs::create_dir(&tree).map_err(|error| stop(REFUSED, format!("{}: {error}", tree.display())))?;
        let build = var("CONVEYOR_WINDOW").is_none();
        let mut words = vec!["archive", &self.target, "rust"];
        if build {
            words.push("hitl");
        }
        let archive = self.checked(&words)?.stdout;
        let untar = run("tar", &["-x", "-C", &tree.display().to_string()], &Options::new("update.tar").input(&archive))
            .map_err(|error| stop(REFUSED, format!("tar не запустился: {error}")))?;
        if !untar.success() {
            return Err(stop(REFUSED, format!("tar не прошёл: {}", tail(&untar.err(), 300))));
        }
        if build {
            let entry = tree.join("hitl").display().to_string();
            let built = run(entry.as_str(), &["--help"], &Options::new("update.build"))
                .map_err(|error| stop(REFUSED, format!("программа из main не запустилась: {error}")))?;
            if !built.success() {
                say(&tail(&built.err(), 2000));
                return Err(stop(REFUSED, "программа hitl из main не собралась: рабочая папка не тронута."));
            }
        }
        let (new, old) = (tree.join("rust"), self.folder.join("rust"));
        let failed = |error: std::io::Error| stop(REFUSED, format!("не получилось поставить программу на место: {error}"));
        if !old.exists() {
            return std::fs::rename(&new, &old).map_err(failed);
        }
        if swap(&new, &old) {
            return Ok(());
        }
        // No swap on this system: two renames, with a moment of no folder between them.
        std::fs::rename(&old, work.join("rust-before")).map_err(failed)?;
        std::fs::rename(&new, &old).map_err(failed)
    }

    pub fn place(&self) -> Result<(), Stop> {
        // The folder is where it was looked at: another update that moved it meanwhile has put its own files there,
        // and those of an older look would go over them (the audit of 2026-10-16, H05).
        if self.first(&["rev-parse", "HEAD"])? != self.head {
            return Err(stop(REFUSED, "рабочую папку hitl только что обновили из другого места: повтори hitl update."));
        }
        let tree = self.lines(&["ls-tree", "-r", &self.target])?;
        let modes: std::collections::HashMap<String, String> = tree
            .iter()
            .filter_map(|line| line.split_once('\t').map(|(head, path)| (path.to_string(), head.split_whitespace().next().unwrap_or("").to_string())))
            .collect();
        let work = self.folder.join(".git").join(format!("conveyor-update-{}", std::process::id()));
        std::fs::create_dir(&work).map_err(|error| stop(REFUSED, format!("{}: {error}", work.display())))?;
        let placed = self.put_files(&work, &modes);
        let _ = std::fs::remove_dir_all(&work);
        placed?;
        // HEAD goes to main, and the index only for what main changed: what another chat has staged stays staged.
        self.checked(&["update-ref", "refs/heads/main", &self.target, &self.head])?;
        let paths = self.paths();
        for chunk in paths.chunks(100) {
            let mut arguments = vec!["reset", "--quiet", "--"];
            arguments.extend(chunk.iter().copied());
            self.checked(&arguments)?;
        }
        Ok(())
    }

    fn put_files(&self, work: &Path, modes: &std::collections::HashMap<String, String>) -> Result<(), Stop> {
        use std::os::unix::fs::PermissionsExt;
        if self.touches_window() {
            self.place_window(work)?;
        }
        if self.touches_program() {
            self.place_program(work)?;
        }
        for (how, path) in &self.changed {
            if path.starts_with(WINDOW) || path.starts_with(PROGRAM) {
                continue;
            }
            let file = self.folder.join(path);
            if how == "D" {
                let _ = std::fs::remove_file(&file);
                continue;
            }
            let failed = |error: std::io::Error| stop(REFUSED, format!("{}: {error}", file.display()));
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent).map_err(failed)?;
            }
            let name = file.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default();
            let temporary = file.with_file_name(format!(".{name}.update-{}", std::process::id()));
            std::fs::write(&temporary, self.content(path)?).map_err(failed)?;
            let mode = if modes.get(path).map(String::as_str) == Some("100755") { 0o755 } else { 0o644 };
            std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(mode)).map_err(failed)?;
            std::fs::rename(&temporary, &file).map_err(failed)?;
        }
        Ok(())
    }

    /// One update of the folder at a time: two at once could leave the files of one under the HEAD of the other.
    pub fn run(&mut self, dry: bool, wait: bool) -> Result<(), Stop> {
        if !self.folder.join(".git").is_dir() {
            return self.update(dry, wait);
        }
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.folder.join(".git").join("conveyor-update.lock"))
            .map_err(|error| stop(REFUSED, format!("замок обновления не открылся: {error}")))?;
        let flags = libc::LOCK_EX | if wait { 0 } else { libc::LOCK_NB };
        // SAFETY: a plain flock on a file this process holds open until the update ends.
        if unsafe { libc::flock(lock.as_raw_fd(), flags) } != 0 {
            return Err(stop(EXIT_BUSY, "рабочую папку hitl сейчас обновляет другой hitl update."));
        }
        let done = self.update(dry, wait);
        drop(lock);
        done
    }

    fn title(&self) -> Result<String, Stop> {
        self.first(&["log", "-1", "--format=%s", &self.target])
    }

    fn files(&self) -> String {
        let count = self.changed.len() as i64;
        format!("{count} {}", plural(count, "файл", "файла", "файлов"))
    }

    fn update(&mut self, dry: bool, wait: bool) -> Result<(), Stop> {
        if !self.look()? {
            say(&format!("Обновлять нечего: рабочая папка hitl уже на влитом main ({}).", short(&self.head)));
            return Ok(());
        }
        self.others_work()?;
        let mut title = self.title()?;
        let mut files = self.files();
        if dry {
            for (how, path) in &self.changed {
                say(&format!("  {how} {path}"));
            }
            let rebuilt = match (self.touches_window(), self.touches_program()) {
                (true, true) => ", окно проверок и программа пересоберутся",
                (true, false) => ", окно проверок пересоберётся",
                (false, true) => ", программа пересоберётся",
                (false, false) => "",
            };
            say(&format!(
                "Сухой прогон: рабочая папка hitl обновилась бы до {} («{title}»), {files}{rebuilt}.",
                short(&self.target)
            ));
            return Ok(());
        }
        if self.touches_window() {
            self.wait_for_the_window(wait)?;
            // The wait may have been long: main and the folder are looked at again.
            if !self.look()? {
                say(&format!(
                    "Обновлять нечего: пока окно было занято, рабочую папку hitl уже обновили ({}).",
                    short(&self.head)
                ));
                return Ok(());
            }
            self.others_work()?;
            title = self.title()?;
            files = self.files();
        }
        self.place()?;
        let built = match (self.touches_window(), self.touches_program()) {
            (true, true) => ", новые окно проверок и программа собраны заранее",
            (true, false) => ", новое окно проверок собрано заранее",
            (false, true) => ", новая программа собрана заранее",
            (false, false) => "",
        };
        say(&format!("Рабочая папка hitl обновлена до {} («{title}»): {files}{built}.", short(&self.target)));
        Ok(())
    }
}

fn short(sha: &str) -> String {
    sha.chars().take(7).collect()
}

/// An atomic exchange of two folders: the new window goes live in one step, and the old one is left where the
/// new one was, to be removed with the work folder.
#[cfg(target_os = "macos")]
fn swap(new: &Path, old: &Path) -> bool {
    unsafe extern "C" {
        fn renamex_np(from: *const libc::c_char, to: *const libc::c_char, flags: libc::c_uint) -> libc::c_int;
    }
    const RENAME_SWAP: libc::c_uint = 0x2;
    let c = |path: &Path| std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).ok();
    let (Some(from), Some(to)) = (c(new), c(old)) else { return false };
    // SAFETY: two valid, NUL-terminated paths that outlive the call.
    unsafe { renamex_np(from.as_ptr(), to.as_ptr(), RENAME_SWAP) == 0 }
}

#[cfg(not(target_os = "macos"))]
fn swap(_new: &Path, _old: &Path) -> bool {
    false
}

/// The command: `hitl update [--dry-run] [--no-wait]`, in the folder the chats run the tools from.
pub fn main(arguments: &[String]) -> i32 {
    run_in(&live_folder(), arguments)
}

/// The command in a given folder (the tests use their own).
pub fn run_in(folder: &Path, arguments: &[String]) -> i32 {
    if matches!(arguments.first().map(String::as_str), Some("-h" | "--help" | "help")) {
        print!("{USAGE}");
        return 0;
    }
    if let Some(unknown) = arguments.iter().find(|argument| !matches!(argument.as_str(), "--dry-run" | "--no-wait")) {
        say(&format!("hitl update: не знаю «{unknown}». hitl update --help"));
        return EXIT_REFUSED;
    }
    let dry = arguments.iter().any(|argument| argument == "--dry-run");
    let wait = !arguments.iter().any(|argument| argument == "--no-wait");
    match Update::new(folder).run(dry, wait) {
        Ok(()) => 0,
        Err(stopped) => {
            say(&format!("Не обновлено: {}", stopped.message));
            stopped.code
        }
    }
}

/// Answers `command` with its arguments, or None when this module does not have it (yet).
pub fn dispatch(command: &str, rest: &[String]) -> Option<i32> {
    (command == "update").then(|| main(rest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::sync::atomic::{AtomicU32, Ordering};

    static NUMBER: AtomicU32 = AtomicU32::new(0);

    /// A made-up GitHub, a clone that lands changes into it, and the folder of the tools.
    struct Made {
        top: PathBuf,
        work: PathBuf,
        live: PathBuf,
    }

    impl Drop for Made {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.top);
        }
    }

    fn git(folder: &Path, arguments: &[&str]) -> String {
        let done = Command::new("git")
            .arg("-C")
            .arg(folder)
            .args(["-c", "user.name=test", "-c", "user.email=test@example.test"])
            .args(arguments)
            .output()
            .expect("git runs");
        assert!(done.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&done.stderr));
        String::from_utf8_lossy(&done.stdout).trim().to_string()
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    impl Made {
        fn new() -> Made {
            let top = std::fs::canonicalize(std::env::temp_dir())
                .unwrap()
                .join(format!("conveyor-update-rust-{}-{}", std::process::id(), NUMBER.fetch_add(1, Ordering::SeqCst)));
            let _ = std::fs::remove_dir_all(&top);
            std::fs::create_dir_all(&top).unwrap();
            let (origin, work, live) = (top.join("origin.git"), top.join("work"), top.join("live"));
            git(&top, &["init", "--quiet", "--bare", "--initial-branch=main", origin.to_str().unwrap()]);
            git(&top, &["clone", "--quiet", origin.to_str().unwrap(), work.to_str().unwrap()]);
            for (path, text) in [
                ("hitl", "#!/bin/sh\n"),
                ("src/tool.py", "first = 1\n"),
                ("docs/guide.md", "the guide\n"),
                ("window/main.swift", "print(\"first\")\n"),
                ("window/Views.swift", "// views\n"),
            ] {
                write(&work.join(path), text);
            }
            let made = Made { top: top.clone(), work, live: live.clone() };
            made.land("The tools as they were");
            git(&top, &["clone", "--quiet", origin.to_str().unwrap(), live.to_str().unwrap()]);
            made
        }

        /// Whatever lies in the other clone goes into main on the made-up GitHub.
        fn land(&self, title: &str) -> String {
            git(&self.work, &["add", "--all"]);
            git(&self.work, &["commit", "--quiet", "-m", title]);
            git(&self.work, &["push", "--quiet", "origin", "HEAD:main"]);
            git(&self.work, &["rev-parse", "HEAD"])
        }
    }

    #[test]
    fn two_updates_at_once_do_not_leave_one_versions_files_under_the_others_head() {
        // The audit of 2026-10-16, H05: an update that had looked at T1 put T1's files in after another had brought
        // the folder to T2, and the folder was T2 by git with T1's file in it.
        let made = Made::new();
        write(&made.work.join("src/tool.py"), "T1\n");
        made.land("T1");
        let mut first = Update::new(&made.live);
        assert!(first.look().unwrap());
        write(&made.work.join("src/tool.py"), "T2\n");
        let t2 = made.land("T2");
        let mut second = Update::new(&made.live);
        assert!(second.look().unwrap());
        second.place().unwrap();
        let refused = first.place().unwrap_err();
        assert!(refused.message.contains("только что обновили из другого места"), "{}", refused.message);
        assert_eq!(git(&made.live, &["rev-parse", "HEAD"]), t2);
        assert_eq!(std::fs::read_to_string(made.live.join("src/tool.py")).unwrap(), "T2\n");
        assert_eq!(git(&made.live, &["status", "--porcelain"]), "");
        // Two run at once: the second waits for the first, or with --no-wait says so, and nothing is mixed.
        write(&made.work.join("src/tool.py"), "T3\n");
        let t3 = made.land("T3");
        let held = std::fs::OpenOptions::new().create(true).append(true).open(made.live.join(".git/conveyor-update.lock")).unwrap();
        assert_eq!(unsafe { libc::flock(held.as_raw_fd(), libc::LOCK_EX) }, 0);
        let busy = Update::new(&made.live).run(false, false).unwrap_err();
        assert_eq!(busy.code, 75, "{}", busy.message);
        assert!(busy.message.contains("обновляет другой hitl update"), "{}", busy.message);
        drop(held);
        Update::new(&made.live).run(false, true).unwrap();
        assert_eq!(git(&made.live, &["rev-parse", "HEAD"]), t3);
    }
}
