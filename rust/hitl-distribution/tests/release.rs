//! `hitl package release` with nothing real behind it: a made-up checkout (a copy of what the release builds, with
//! the file `hitl`, in a repository of its own whose origin is a folder), a made-up `gh` and `brew`, and a tap
//! that is a folder too. No path to the real `gh` or `brew` is on the PATH it runs with: nothing is published, nothing
//! is pushed anywhere but into the made-up folders.

mod common;

use common::{Folder, environment, git, project, run};
use sha2::{Digest, Sha256};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Copies what a release needs of the project: the entry, the crates (without what cargo built in them), the window
/// and the skill. The copy's entry builds the same program as the project's, from the same sources.
fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let (source, made) = (entry.path(), to.join(entry.file_name()));
        if source.is_dir() {
            if entry.file_name() != "target" {
                copy(&source, &made);
            }
        } else {
            std::fs::copy(&source, &made).unwrap();
        }
    }
}

fn script(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

struct Release {
    folder: Folder,
    root: PathBuf,
    fakes: PathBuf,
    tap: PathBuf,
}

impl Release {
    fn new() -> Release {
        let folder = Folder::new("release");
        let root = folder.join("checkout");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::copy(project().join("hitl"), root.join("hitl")).unwrap();
        for part in ["rust", "window", "skills/hitl"] {
            copy(&project().join(part), &root.join(part));
        }
        std::fs::write(root.join(".gitignore"), ".DS_Store\n").unwrap();
        let origin = folder.join("origin.git");
        git(&folder.0, &["init", "-q", "--bare", "-b", "main", &origin.display().to_string()]);
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "Start"]);
        git(&root, &["remote", "add", "origin", &origin.display().to_string()]);
        git(&root, &["push", "-q", "origin", "main"]);
        // His tap: a repository whose origin is a folder, with a commit and the author it commits as.
        let tap_origin = folder.join("tap.git");
        git(&folder.0, &["init", "-q", "--bare", "-b", "main", &tap_origin.display().to_string()]);
        let tap = folder.join("tap");
        git(&folder.0, &["clone", "-q", &tap_origin.display().to_string(), &tap.display().to_string()]);
        std::fs::create_dir_all(tap.join("Formula")).unwrap();
        std::fs::write(tap.join("Formula/other.rb"), "class Other < Formula\nend\n").unwrap();
        git(&tap, &["add", "-A"]);
        git(&tap, &["commit", "-q", "-m", "Tap"]);
        git(&tap, &["push", "-q", "origin", "HEAD:main"]);
        git(&tap, &["config", "user.name", "tap"]);
        git(&tap, &["config", "user.email", "tap@example.test"]);
        // gh and brew of their own: every call written down, the archive of a release kept.
        let fakes = folder.join("fakes");
        std::fs::create_dir_all(&fakes).unwrap();
        let log = fakes.join("gh.log");
        script(
            &fakes.join("gh"),
            &format!(
                "#!/bin/sh\nprintf '%s\\037' \"$@\" >> '{log}'\nprintf '\\036' >> '{log}'\n\
case \"$1 $2\" in\n  'repo view') [ -f '{fakes}/no-repo' ] && exit 1; exit 0 ;;\n  'repo create') rm -f '{fakes}/no-repo' ;;\n  \
'release create') cp \"$4\" '{fakes}/released.tar.gz' ;;\nesac\nexit 0\n",
                log = log.display(),
                fakes = fakes.display()
            ),
        );
        script(&fakes.join("brew"), &format!("#!/bin/sh\n[ \"$*\" = '--repository nextster/tap' ] || exit 1\necho '{}'\n", tap.display()));
        Release { root, fakes, tap, folder }
    }

    /// `hitl package release VERSION` from the checkout's own entry, with the made-up gh and brew and no other.
    fn release(&self, version: &str) -> common::Run {
        let mut environment = environment(&self.folder.0);
        let cargo = std::env::var("PATH").unwrap_or_default().split(':').map(|folder| Path::new(folder).join("cargo")).find(|path| path.is_file())
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .expect("cargo on the PATH");
        let path = format!("{}:/usr/bin:/bin:/usr/sbin:/sbin:{}", self.fakes.display(), cargo.display());
        for tool in ["gh", "brew"] {
            assert!(!cargo.join(tool).exists(), "{tool} beside cargo would be the real one");
        }
        environment.insert("PATH".into(), path);
        run(&self.root.join("hitl"), &["package", "release", version], &environment, &self.root)
    }

    fn calls(&self) -> Vec<Vec<String>> {
        std::fs::read_to_string(self.fakes.join("gh.log"))
            .unwrap_or_default()
            .split('\u{1e}')
            .filter(|call| !call.is_empty())
            .map(|line| line.trim_end_matches('\u{1f}').split('\u{1f}').map(str::to_string).collect())
            .collect()
    }
}

#[test]
fn a_release_is_built_from_merged_main_only_and_published_with_its_formula() {
    let trial = Release::new();
    // Changes that are not merged: refused before anything is built or published.
    std::fs::write(trial.root.join("stray.txt"), "x").unwrap();
    let done = trial.release("1.2.3");
    assert_eq!(done.code, 64, "{}{}", done.out, done.err);
    assert_eq!(done.err, "hitl package release: в папке есть невлитые изменения; релиз собирается из влитого main.\n");
    std::fs::remove_file(trial.root.join("stray.txt")).unwrap();
    // A commit that is not on origin's main: refused too.
    std::fs::write(trial.root.join("window/Note.swift"), "// not merged\n").unwrap();
    git(&trial.root, &["commit", "-q", "-am", "Not merged"]);
    let done = trial.release("1.2.3");
    assert_eq!(done.code, 64, "{}{}", done.out, done.err);
    assert_eq!(done.err, "hitl package release: эта папка не на влитом main; релиз собирается из него.\n");
    git(&trial.root, &["reset", "-q", "--hard", "HEAD~1"]);
    assert!(trial.calls().is_empty(), "nothing was asked of gh: {:?}", trial.calls());

    // Merged main, and no public repository yet: it is made, the release published, the formula pointed at it.
    std::fs::write(trial.fakes.join("no-repo"), "").unwrap();
    let done = trial.release("1.2.3");
    assert_eq!(done.code, 0, "{}{}", done.out, done.err);
    assert_eq!(
        done.out,
        "Заведён публичный репозиторий nextster/hitl-releases.\n\
Релиз: https://github.com/nextster/hitl-releases/releases/tag/v1.2.3\n\
Формула: nextster/homebrew-tap/Formula/hitl.rb → 1.2.3. Ставится: brew install nextster/tap/hitl\n"
    );
    let calls = trial.calls();
    let short = git(&trial.root, &["rev-parse", "--short", "HEAD"]);
    assert_eq!(calls[0], ["repo", "view", "nextster/hitl-releases"]);
    assert_eq!(calls[1], ["repo", "create", "nextster/hitl-releases", "--public", "--description", "hitl: releases for brew install nextster/tap/hitl"]);
    let published = &calls[2];
    assert_eq!(published[..2], ["release", "create"]);
    assert_eq!(published[2], "v1.2.3");
    assert!(published[3].ends_with("/hitl-1.2.3.tar.gz"), "{published:?}");
    assert_eq!(
        published[4..],
        ["-R", "nextster/hitl-releases", "--title", "hitl 1.2.3", "--notes", &format!("brew install nextster/tap/hitl\n\nСобран из hitl {short}.")]
    );
    assert!(!Path::new(&published[3]).exists(), "the archive's folder goes after the release");
    let archive = std::fs::read(trial.fakes.join("released.tar.gz")).unwrap();
    let digest: String = Sha256::digest(&archive).iter().map(|byte| format!("{byte:02x}")).collect();
    // The tap's origin has the formula of exactly what was published, in a commit of its own.
    let checked = trial.folder.join("tap-check");
    git(&trial.folder.0, &["clone", "-q", &trial.folder.join("tap.git").display().to_string(), &checked.display().to_string()]);
    assert_eq!(git(&checked, &["log", "-1", "--format=%s"]), "hitl 1.2.3");
    assert_eq!(std::fs::read_to_string(checked.join("Formula/hitl.rb")).unwrap(), hitl_distribution::formula("1.2.3", &digest));
    assert!(trial.tap.join("Formula/other.rb").is_file());

    // The public repository there already: not made again.
    let done = trial.release("1.2.4");
    assert_eq!(done.code, 0, "{}{}", done.out, done.err);
    assert!(!done.out.contains("Заведён"), "{}", done.out);
    assert_eq!(trial.calls().iter().filter(|call| call[..2] == ["repo", "create"]).count(), 1);
}
