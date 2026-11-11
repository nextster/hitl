//! `hitl package build|release`: hitl, the check window for his team, built out of this project and published.
//!
//! His words of 2026-10-28: «пусть будет в одном репозитории hitl но hitl пусть можно будет выкладывать отдельно в
//! публичный releases и устанавливать через brew», «как в tolk», then «хотя бы ту часть чтоб можно было дистрибьютить
//! hitl без питона совсем». So the source stays here, and a release is an archive with no Python and nothing to
//! compile: the program of rust/hitl, the window built from window/*.swift, and the skill without his own parts, in
//! the public repository nextster/hitl-releases, with a formula in his tap: `brew install nextster/tap/hitl`. Built
//! for Macs with Apple's chips and macOS 14 or later: the window needs no older one, and this Mac's Rust has no other
//! target. A release is published on his word in the chat only; `build` changes nothing outside the folder it is
//! given (and the cache the program of rust/hitl is built into once per version).

use hitl_host_core::process::{self, Options};
use hitl_host_core::{EXIT_REFUSED, say, var};
use sha2::{Digest, Sha256};
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

const USAGE: &str = "Usage:
  hitl package build [--version V] [--out DIR]
                      Build the archive hitl-V.tar.gz into DIR (a folder of its own when none is
                      given) and print its path. Changes nothing else.
  hitl package release V
                      On his word in the chat only: build hitl V from this checkout, which has to be
                      main as merged, publish it in nextster/hitl-releases and point the formula in
                      nextster/homebrew-tap at it. Prints what it did.
";

pub const RELEASES: &str = "nextster/hitl-releases";
pub const TAP: &str = "nextster/homebrew-tap";
/// The parts of the skill that are his: the phone, and the rules of his own (`local.md`).
pub const HIS_PARTS: [&str; 2] = ["phone.md", "local.md"];

/// The oldest macOS the window is built for: it uses what macOS 14 brought.
pub const MACOS: &str = "14.0";

pub const README: &str = "# hitl

Агент ставит человеку в одно окно на Маке то, без чего не идёт дальше: план перед большой работой, вопрос с вариантами,
результат перед вливанием, действие, которое может сделать только человек, объяснение. Человек отвечает клавишей,
агент получает ответ по номерам пунктов и продолжает. Работает с чатами Claude Code и Codex.

## Установка

```sh
brew install nextster/tap/hitl
hitl setup --name Никита
```

`hitl setup` запоминает имя для итогов проверок (`--she` — для женского рода) и кладёт навык `hitl` туда, где его
находят Claude Code (`~/.claude/skills`) и Codex (`~/.agents/skills`). Нужен Мак на Apple Silicon с macOS 14 или
новее; ни Python, ни инструменты Xcode не нужны. Всё, что hitl хранит, лежит в `~/.hitl`.

## Как пользоваться

Скажи агенту работать через hitl, или он сам возьмёт навык, когда ему нужно твоё решение. Окно поднимается само, пока
есть проверки, и уходит, когда очередь пуста. `hitl --help` и `hitl check --help` говорят остальное.
";

/// The formula of hitl `version` for his tap: the archive of the release, run from where Homebrew keeps it.
pub fn formula(version: &str, sha256: &str) -> String {
    format!(
        r##"class Hitl < Formula
  desc "Agent plans, questions and results, answered by a person in one window on the Mac"
  homepage "https://github.com/{RELEASES}"
  url "https://github.com/{RELEASES}/releases/download/v{version}/hitl-{version}.tar.gz"
  sha256 "{sha256}"

  depends_on arch: :arm64
  depends_on macos: :sonoma

  def install
    libexec.install Dir["*"]
    (bin/"hitl").write <<~SH
      #!/bin/sh
      export HITL_ROOT="#{{opt_libexec}}"
      exec "#{{opt_libexec}}/hitl" "$@"
    SH
  end

  def caveats
    <<~EOS
      Once, with the name the verdicts call you by (--she for the feminine forms):
        hitl setup --name Никита
      It puts the skill hitl where Claude Code and Codex find it. Run it again after an upgrade.
    EOS
  end

  test do
    assert_match "hitl {version}", shell_output("#{{bin}}/hitl --version")
  end
end
"##
    )
}

// ------------------------------------------------------------------------------------------------ folders

/// A folder of its own in the temporary one, `PREFIX…`.
fn temporary(prefix: &str) -> Result<PathBuf, String> {
    static NUMBER: AtomicUsize = AtomicUsize::new(0);
    loop {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
        let folder = std::env::temp_dir().join(format!("{prefix}{}-{stamp:x}{}", std::process::id(), NUMBER.fetch_add(1, Ordering::Relaxed)));
        match std::fs::create_dir(&folder) {
            Ok(()) => return Ok(folder),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("{}: {error}", folder.display())),
        }
    }
}

/// A temporary folder that goes when it goes out of scope.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Python's `repr` of a text, as the refusal of a version quotes it.
fn quoted(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') { '"' } else { '\'' };
    let mut said = String::from(quote);
    for c in text.chars() {
        match c {
            '\\' => said.push_str("\\\\"),
            '\n' => said.push_str("\\n"),
            '\r' => said.push_str("\\r"),
            '\t' => said.push_str("\\t"),
            c if c == quote => {
                said.push('\\');
                said.push(c);
            }
            c => said.push(c),
        }
    }
    said.push(quote);
    said
}

/// A version as a release names it: `0.1.0`, `1.2.3-rc.1`.
fn valid_version(version: &str) -> bool {
    let (core, suffix) = match version.split_once('-') {
        Some((core, suffix)) => (core, Some(suffix)),
        None => (version, None),
    };
    let numbers: Vec<&str> = core.split('.').collect();
    numbers.len() == 3
        && numbers.iter().all(|number| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
        && suffix.is_none_or(|suffix| !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.'))
}

// ------------------------------------------------------------------------------------------------ the program of rust/hitl

/// The user's own home, whatever `HOME` says: a suite's made-up home has neither Rust nor its crates.
fn own_home() -> PathBuf {
    // SAFETY: getpwuid returns a pointer into static storage or null; it is read at once.
    let entry = unsafe { libc::getpwuid(libc::getuid()) };
    if !entry.is_null() && !unsafe { (*entry).pw_dir }.is_null() {
        let folder = unsafe { std::ffi::CStr::from_ptr((*entry).pw_dir) };
        return PathBuf::from(std::ffi::OsStr::from_bytes(folder.to_bytes()));
    }
    PathBuf::from(var("HOME").unwrap_or_else(|| "/".into()))
}

/// A real cargo and rustc, never a rustup shim (which may set out to install something): the one on PATH, else a
/// toolchain of rustup's, stable first, in `RUSTUP_HOME` or the user's own home, even under a made-up `HOME`.
fn toolchain() -> Result<(PathBuf, PathBuf), String> {
    let mut folders: Vec<PathBuf> = hitl_host_core::which("cargo")
        .and_then(|cargo| cargo.canonicalize().ok())
        .and_then(|cargo| cargo.parent().map(Path::to_path_buf))
        .into_iter()
        .collect();
    let mut homes: Vec<PathBuf> = var("RUSTUP_HOME").filter(|value| !value.is_empty()).map(PathBuf::from).into_iter().collect();
    homes.push(own_home().join(".rustup"));
    for home in homes {
        let mut installed: Vec<PathBuf> = std::fs::read_dir(home.join("toolchains"))
            .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
            .unwrap_or_default();
        installed.sort_by_key(|path| (!path.file_name().is_some_and(|name| name.to_string_lossy().starts_with("stable")), path.clone()));
        folders.extend(installed.into_iter().map(|path| path.join("bin")));
    }
    for folder in folders {
        let real = |name: &str| -> Option<PathBuf> {
            let path = folder.join(name).canonicalize().ok()?;
            let runs = std::fs::metadata(&path).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0);
            (runs && path.file_name().is_some_and(|name| name != "rustup")).then_some(path)
        };
        if let (Some(cargo), Some(rustc)) = (real("cargo"), real("rustc")) {
            return Ok((cargo, rustc));
        }
    }
    Err("для окна проверок нужен установленный Rust (cargo и rustc)".into())
}

/// Every source of the crate, in the order Python sorted its paths: by their parts.
fn sources(folder: &Path, crate_folder: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if entry.file_name() != "target" {
                sources(&path, crate_folder, found);
            }
        } else if path.extension().is_some_and(|extension| extension == "rs")
            || (folder == crate_folder && matches!(entry.file_name().to_str(), Some("Cargo.toml" | "Cargo.lock")))
        {
            found.push(path);
        }
    }
}

/// Where the program of this version of the crate is built: a folder per digest of its source and of the compiler,
/// in the cache (`XDG_CACHE_HOME`, else `~/Library/Caches`), the one `hitlcore.py` built it into.
fn target(crate_folder: &Path, cargo: &Path, rustc: &Path) -> PathBuf {
    let mut found = Vec::new();
    sources(crate_folder, crate_folder, &mut found);
    found.sort_by(|one, other| one.components().cmp(other.components()));
    let mut digest = Sha256::new();
    for path in &found {
        let relative = path.strip_prefix(crate_folder).unwrap_or(path);
        digest.update(format!("rust/hitl/{}", relative.display()).as_bytes());
        digest.update(b"\0");
        digest.update(std::fs::read(path).unwrap_or_default());
        digest.update(b"\0");
    }
    // The versions of its libraries are the workspace's, in the one Cargo.lock beside the crates.
    if let Some(workspace) = crate_folder.parent() {
        for name in ["Cargo.toml", "Cargo.lock"] {
            if let Ok(text) = std::fs::read(workspace.join(name)) {
                digest.update(format!("rust/{name}").as_bytes());
                digest.update(b"\0");
                digest.update(text);
                digest.update(b"\0");
            }
        }
    }
    for path in [cargo, rustc] {
        if let Ok(meta) = std::fs::metadata(path) {
            let nanoseconds = meta.mtime() as i128 * 1_000_000_000 + meta.mtime_nsec() as i128;
            digest.update(format!("{}:{}:{nanoseconds}", path.display(), meta.size()).as_bytes());
        }
    }
    let hex: String = digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    let cache = var("XDG_CACHE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(var("HOME").unwrap_or_else(|| "/".into())).join("Library/Caches"));
    cache.join("hitl-distribution").join(&hex[..24])
}

/// The last `count` characters of a text.
fn tail(text: &str, count: usize) -> String {
    let all: Vec<char> = text.chars().collect();
    all[all.len().saturating_sub(count)..].iter().collect()
}

/// The folder of cargo of this copy of the project, in `cache`, its own: in one workspace cargo names what it built by
/// the paths inside it and tells what is fresh by the times of the files, so a folder shared by two copies could hand
/// one the other's build. It holds the crate's path in `ROOT`; the folders of copies that are gone go, and so does the
/// folder every copy shared before, once nothing has built into it for two days.
fn own_target(cache: &Path, crate_folder: &Path) -> PathBuf {
    let text = crate_folder.display().to_string();
    let key = text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3));
    let targets = cache.join("targets");
    for entry in std::fs::read_dir(&targets).into_iter().flatten().flatten() {
        let root = std::fs::read_to_string(entry.path().join("ROOT")).unwrap_or_default();
        if !root.trim().is_empty() && !Path::new(root.trim()).is_dir() {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
    let old = cache.join("target");
    let stale = std::fs::metadata(old.join("release/hitl")).and_then(|meta| meta.modified()).ok().and_then(|when| when.elapsed().ok());
    if stale.is_some_and(|age| age > std::time::Duration::from_secs(2 * 24 * 3600)) {
        let _ = std::fs::remove_dir_all(&old);
    }
    let own = targets.join(format!("{key:016x}"));
    let _ = std::fs::create_dir_all(&own);
    let _ = std::fs::write(own.join("ROOT"), format!("{text}\n"));
    own
}

/// The release build of the program `hitl` of the crate (rust/hitl), built offline when this version has none yet;
/// one build at a time, whoever asks.
pub fn hitl_program(crate_folder: &Path) -> Result<PathBuf, String> {
    let (cargo, rustc) = toolchain()?;
    let folder = target(crate_folder, &cargo, &rustc);
    let program = folder.join("release/hitl");
    if program.is_file() {
        return Ok(program);
    }
    std::fs::create_dir_all(&folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    let shared = own_target(folder.parent().unwrap_or(&folder), crate_folder);
    let lock_path = folder.parent().unwrap_or(&folder).join(".build.lock");
    let lock = std::fs::OpenOptions::new().create(true).append(true).open(&lock_path).map_err(|error| format!("{}: {error}", lock_path.display()))?;
    // SAFETY: an advisory lock on a file this function holds open; it goes with the file.
    unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&lock), libc::LOCK_EX) };
    if program.is_file() {
        return Ok(program);
    }
    say("==> Собираю программу окна проверок (один раз на версию).");
    let manifest = crate_folder.join("Cargo.toml").display().to_string();
    let cargo_home = var("CARGO_HOME").filter(|value| !value.is_empty()).unwrap_or_else(|| own_home().join(".cargo").display().to_string());
    let options = Options::new("hitl.build_program")
        .env("RUSTC", &rustc.display().to_string())
        // One folder of cargo for every version of this copy of the project, the program copied out: a folder each built
        // every crate from nothing.
        .env("CARGO_TARGET_DIR", &shared.display().to_string())
        .env("CARGO_NET_OFFLINE", "true")
        .env("CARGO_HOME", &cargo_home)
        .timeout(600.0);
    let words = ["build", "--release", "--locked", "--offline", "--quiet", "--manifest-path", manifest.as_str()].map(String::from);
    let built = process::run(cargo.display().to_string(), &words, &options).map_err(|error| format!("{}: {error}", cargo.display()))?;
    let made = shared.join("release/hitl");
    if !built.success() || !made.is_file() {
        let said = if built.err().trim().is_empty() { built.out() } else { built.err() };
        return Err(format!("программа окна проверок не собралась: {}", tail(said.trim(), 1500)));
    }
    let _ = std::fs::create_dir_all(folder.join("release"));
    let copy = folder.join(format!("release/.hitl.{}", std::process::id()));
    std::fs::copy(&made, &copy).and_then(|_| std::fs::rename(&copy, &program)).map_err(|error| format!("{}: {error}", program.display()))?;
    Ok(program)
}

/// The window of this version of its source, built with `swiftc -O` once and kept in the cache beside the program of
/// hitl: built for every archive, it was most of the time of every test that makes one.
fn window_program(swift: &[PathBuf]) -> Result<PathBuf, String> {
    let mut digest = Sha256::new();
    digest.update(format!("macos{MACOS}").as_bytes());
    for path in swift {
        digest.update(path.file_name().map(|name| name.as_bytes().to_vec()).unwrap_or_default());
        digest.update(b"\0");
        digest.update(std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?);
        digest.update(b"\0");
    }
    // The compiler too: another Xcode builds another window.
    if let Ok(found) = process::run("xcrun".to_string(), &["--find".to_string(), "swiftc".to_string()], &Options::new("hitl.find_swiftc")) {
        let compiler = found.out().trim().to_string();
        if let Ok(meta) = std::fs::metadata(&compiler) {
            digest.update(format!("{compiler}:{}:{}:{}", meta.size(), meta.mtime(), meta.mtime_nsec()).as_bytes());
        }
    }
    let hex: String = digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    let cache = var("XDG_CACHE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(var("HOME").unwrap_or_else(|| "/".into())).join("Library/Caches"));
    let folder = cache.join("hitl-distribution").join(format!("window-{}", &hex[..24]));
    let program = folder.join("CheckPanel");
    if program.is_file() {
        return Ok(program);
    }
    std::fs::create_dir_all(&folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    let lock_path = folder.join(".build.lock");
    let lock = std::fs::OpenOptions::new().create(true).append(true).open(&lock_path).map_err(|error| format!("{}: {error}", lock_path.display()))?;
    // SAFETY: an advisory lock on a file this function holds open; it goes with the file.
    unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&lock), libc::LOCK_EX) };
    if program.is_file() {
        return Ok(program);
    }
    let made = folder.join(format!(".CheckPanel.{}", std::process::id()));
    let mut words: Vec<String> = vec!["-O".into(), "-target".into(), format!("arm64-apple-macos{MACOS}")];
    words.extend(swift.iter().map(|path| path.display().to_string()));
    words.extend(["-o".into(), made.display().to_string()]);
    let built = process::run("swiftc".to_string(), &words, &Options::new("hitl.build_window").timeout(900.0)).map_err(|error| format!("swiftc: {error}"))?;
    if !built.success() {
        let _ = std::fs::remove_file(&made);
        return Err(format!("окно не собралось: {}", tail(&built.err(), 1500)));
    }
    std::fs::rename(&made, &program).map_err(|error| format!("{}: {error}", program.display()))?;
    Ok(program)
}

// ------------------------------------------------------------------------------------------------ the archive

/// Copies a folder of the skill, without his parts and Finder's files, at every level.
fn copy_skill(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|error| format!("{}: {error}", to.display()))?;
    let entries = std::fs::read_dir(from).map_err(|error| format!("{}: {error}", from.display()))?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name == ".DS_Store" || HIS_PARTS.iter().any(|his| name == *his) {
            continue;
        }
        let (source, copy) = (entry.path(), to.join(&name));
        if source.is_dir() {
            copy_skill(&source, &copy)?;
        } else {
            std::fs::copy(&source, &copy).map_err(|error| format!("{}: {error}", source.display()))?;
        }
    }
    Ok(())
}

/// What the archive holds, in the order Python's tarfile added it: a folder, then what is in it, sorted by name.
fn entries(base: &Path, relative: &Path, found: &mut Vec<PathBuf>) -> Result<(), String> {
    found.push(relative.to_path_buf());
    let folder = base.join(relative);
    if !folder.is_dir() {
        return Ok(());
    }
    let mut names: Vec<_> = std::fs::read_dir(&folder).map_err(|error| format!("{}: {error}", folder.display()))?.flatten().map(|entry| entry.file_name()).collect();
    names.sort_by(|one, other| one.as_bytes().cmp(other.as_bytes()));
    for name in names {
        entries(base, &relative.join(name), found)?;
    }
    Ok(())
}

/// Every file and folder of the tree dated the first second of 1970, as tarfile wrote them: the same tree, the same
/// archive.
fn date_to_zero(path: &Path) -> Result<(), String> {
    let name = CString::new(path.as_os_str().as_bytes()).map_err(|error| error.to_string())?;
    let times = [libc::timeval { tv_sec: 0, tv_usec: 0 }; 2];
    // SAFETY: a path and two times this function owns, for the duration of the call.
    if unsafe { libc::lutimes(name.as_ptr(), times.as_ptr()) } != 0 {
        return Err(format!("{}: {}", path.display(), std::io::Error::last_os_error()));
    }
    Ok(())
}

/// The archive of hitl `version` in `out`: `hitl-V.tar.gz`, one folder `hitl-V` with the program `hitl`, the window
/// `window/CheckPanel`, the skill `skills/hitl` without his parts, `EDITION` and `README.md`; owned by 0, with no
/// names of owners and no dates, so that the same tree packs into the same bytes.
pub fn build(version: &str, out: &Path) -> Result<PathBuf, String> {
    if !valid_version(version) {
        return Err(format!("версия {}: нужна вида 0.1.0", quoted(version)));
    }
    let root = hitl_host_core::root();
    let scratch = Scratch(temporary("hitl-build-")?);
    let name = format!("hitl-{version}");
    let tree = scratch.0.join(&name);
    let made = |error: std::io::Error| error.to_string();
    std::fs::create_dir_all(tree.join("window")).map_err(made)?;
    let program = hitl_program(&root.join("rust/hitl"))?;
    std::fs::copy(&program, tree.join("hitl")).map_err(|error| format!("{}: {error}", program.display()))?;
    let mut swift: Vec<PathBuf> = std::fs::read_dir(root.join("window"))
        .map_err(|error| format!("{}: {error}", root.join("window").display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "swift"))
        .collect();
    swift.sort();
    let window = window_program(&swift)?;
    std::fs::copy(&window, tree.join("window/CheckPanel")).map_err(|error| format!("{}: {error}", window.display()))?;
    copy_skill(&root.join("skills/hitl"), &tree.join("skills/hitl"))?;
    std::fs::write(tree.join("EDITION"), format!("hitl {version}\n")).map_err(made)?;
    std::fs::write(tree.join("README.md"), README).map_err(made)?;
    let mut packed = Vec::new();
    entries(&scratch.0, Path::new(&name), &mut packed)?;
    for path in &packed {
        date_to_zero(&scratch.0.join(path))?;
    }
    std::fs::create_dir_all(out).map_err(|error| format!("{}: {error}", out.display()))?;
    let archive = out.join(format!("{name}.tar.gz"));
    let absolute = std::path::absolute(&archive).map_err(made)?;
    let mut words: Vec<String> = ["-c", "-z", "-n", "--uid", "0", "--gid", "0", "--numeric-owner", "--no-mac-metadata", "--no-xattrs", "--no-acls",
                                  "--no-fflags", "--options", "gzip:!timestamp", "-f"]
        .map(String::from)
        .to_vec();
    words.push(absolute.display().to_string());
    words.push("-C".into());
    words.push(scratch.0.display().to_string());
    words.extend(packed.iter().map(|path| path.display().to_string()));
    let options = Options::new("hitl.pack").env("COPYFILE_DISABLE", "1");
    let done = process::run("/usr/bin/tar".to_string(), &words, &options).map_err(|error| format!("/usr/bin/tar: {error}"))?;
    if !done.success() {
        return Err(format!("архив не собрался: {}", tail(done.err().trim(), 1500)));
    }
    Ok(archive)
}

// ------------------------------------------------------------------------------------------------ the release

/// Runs a program and gives what it printed, or what it said when it failed, after the words that ran it.
fn run(words: &[&str], cwd: Option<&Path>) -> Result<String, String> {
    let mut options = Options::new("hitl.release");
    if let Some(folder) = cwd {
        options = options.cwd(folder);
    }
    let arguments: Vec<String> = words[1..].iter().map(|word| word.to_string()).collect();
    let done = process::run(words[0].to_string(), &arguments, &options).map_err(|error| format!("{}: {error}", words.join(" ")))?;
    if !done.success() {
        let said = if done.err().trim().is_empty() { done.out() } else { done.err() };
        return Err(format!("{}: {}", words.join(" "), said.trim()));
    }
    Ok(done.out().trim().to_string())
}

/// Publishes hitl `version`: the archive in the public repository, the formula in the tap.
fn release(version: &str) -> Result<i32, String> {
    let root = hitl_host_core::root();
    if !run(&["git", "status", "--porcelain"], Some(&root))?.is_empty() {
        say("hitl package release: в папке есть невлитые изменения; релиз собирается из влитого main.");
        return Ok(EXIT_REFUSED);
    }
    run(&["git", "fetch", "-q", "origin", "main"], Some(&root))?;
    if run(&["git", "rev-parse", "HEAD"], Some(&root))? != run(&["git", "rev-parse", "origin/main"], Some(&root))? {
        say("hitl package release: эта папка не на влитом main; релиз собирается из него.");
        return Ok(EXIT_REFUSED);
    }
    let digest = {
        let scratch = Scratch(temporary("hitl-release-")?);
        let archive = build(version, &scratch.0)?;
        let bytes = std::fs::read(&archive).map_err(|error| format!("{}: {error}", archive.display()))?;
        let digest: String = Sha256::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect();
        let known = process::run("gh".to_string(), &["repo", "view", RELEASES].map(String::from), &Options::new("hitl.release"))
            .map_err(|error| format!("gh: {error}"))?;
        if !known.success() {
            run(&["gh", "repo", "create", RELEASES, "--public", "--description", "hitl: releases for brew install nextster/tap/hitl"], None)?;
            println!("Заведён публичный репозиторий {RELEASES}.");
        }
        let short = run(&["git", "rev-parse", "--short", "HEAD"], Some(&root))?;
        let notes = format!("brew install nextster/tap/hitl\n\nСобран из hitl {short}.");
        let (tag, title, path) = (format!("v{version}"), format!("hitl {version}"), archive.display().to_string());
        run(&["gh", "release", "create", &tag, &path, "-R", RELEASES, "--title", &title, "--notes", &notes], None)?;
        println!("Релиз: https://github.com/{RELEASES}/releases/tag/v{version}");
        digest
    };
    let tap = PathBuf::from(run(&["brew", "--repository", "nextster/tap"], None)?);
    run(&["git", "pull", "-q", "--ff-only"], Some(&tap))?;
    let file = tap.join("Formula/hitl.rb");
    std::fs::write(&file, formula(version, &digest)).map_err(|error| format!("{}: {error}", file.display()))?;
    run(&["git", "add", "Formula/hitl.rb"], Some(&tap))?;
    run(&["git", "commit", "-q", "-m", &format!("hitl {version}")], Some(&tap))?;
    run(&["git", "push", "-q"], Some(&tap))?;
    println!("Формула: {TAP}/Formula/hitl.rb → {version}. Ставится: brew install nextster/tap/hitl");
    Ok(0)
}

/// `hitl package ARGS`.
pub fn main(arguments: &[String]) -> i32 {
    let Some(command) = arguments.first().map(String::as_str).filter(|command| !matches!(*command, "-h" | "--help" | "help")) else {
        print!("{USAGE}");
        return 0;
    };
    let rest = &arguments[1..];
    let done = match command {
        "build" => {
            let (mut version, mut out) = ("0.0.0".to_string(), None::<PathBuf>);
            let mut position = 0;
            while position < rest.len() {
                match rest[position].as_str() {
                    "--version" if position + 1 < rest.len() => version = rest[position + 1].clone(),
                    "--out" if position + 1 < rest.len() => out = Some(PathBuf::from(&rest[position + 1])),
                    _ => {
                        eprint!("{USAGE}");
                        return EXIT_REFUSED;
                    }
                }
                position += 2;
            }
            let out = match out {
                Some(out) => Ok(out),
                None => temporary("hitl-"),
            };
            out.and_then(|out| build(&version, &out)).map(|archive| {
                println!("{}", archive.display());
                0
            })
        }
        "release" if rest.len() == 1 => release(&rest[0]),
        _ => {
            eprint!("{USAGE}");
            return EXIT_REFUSED;
        }
    };
    done.unwrap_or_else(|error| {
        say(&format!("hitl package {command}: {error}"));
        1
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_is_three_numbers_and_maybe_a_mark() {
        for good in ["0.1.0", "1.2.3", "10.20.30-rc.1", "1.0.0-beta2"] {
            assert!(valid_version(good), "{good}");
        }
        for bad in ["1.2", "1.2.3.4", "v1.2.3", "1.2.3-", "1.2.3-rc-1", "1..3", "1.2.x", ""] {
            assert!(!valid_version(bad), "{bad}");
        }
    }

    #[test]
    fn the_formula_runs_the_archive_by_the_path_that_outlives_an_upgrade() {
        let text = formula("1.2.3", &"ab".repeat(32));
        assert!(text.contains("url \"https://github.com/nextster/hitl-releases/releases/download/v1.2.3/hitl-1.2.3.tar.gz\""));
        assert!(text.contains(&format!("sha256 \"{}\"", "ab".repeat(32))));
        assert!(text.contains("export HITL_ROOT=\"#{opt_libexec}\""));
        assert!(text.contains("exec \"#{opt_libexec}/hitl\" \"$@\""));
        assert!(text.contains("depends_on arch: :arm64"));
        assert!(text.contains("assert_match \"hitl 1.2.3\", shell_output(\"#{bin}/hitl --version\")"));
        assert!(!text.to_lowercase().contains("python"));
    }

    #[test]
    fn the_command_says_how_it_is_called() {
        assert_eq!(main(&[]), 0);
        assert_eq!(main(&["--help".into()]), 0);
        assert_eq!(main(&["release".into()]), EXIT_REFUSED);
        assert_eq!(main(&["build".into(), "--out".into()]), EXIT_REFUSED);
        assert_eq!(main(&["build".into(), "--name".into(), "x".into()]), EXIT_REFUSED);
        assert_eq!(main(&["nothing".into()]), EXIT_REFUSED);
        assert_eq!(main(&["build".into(), "--version".into(), "1.2".into(), "--out".into(), "/nonexistent/never".into()]), 1);
    }
}
