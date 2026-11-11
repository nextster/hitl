//! Where hitl keeps what it keeps, and where its own files stand.

use std::path::{Path, PathBuf};

fn user_home() -> PathBuf {
    PathBuf::from(crate::var("HOME").filter(|value| !value.is_empty()).unwrap_or_else(|| "/".into()))
}

/// Where checks, queues, and their history live.
pub fn home() -> PathBuf {
    hitl::common::home()
}

/// A folder of the state: `state("queue")` is `~/.conveyor/queue`.
pub fn state(name: &str) -> PathBuf {
    home().join(name)
}

/// The phone's queue of one phone: `iphone/<UDID>`.
pub fn iphone(udid: &str) -> PathBuf {
    state("iphone").join(udid)
}

/// The project: the folder with the file `hitl`, `src/` and `rust/`. `HITL_ROOT` (the script exports it), else
/// `CONVEYOR_ROOT`, else the nearest folder above the working one that has them.
pub fn root() -> PathBuf {
    for name in ["HITL_ROOT", "CONVEYOR_ROOT"] {
        if let Some(value) = crate::var(name).filter(|value| !value.is_empty()) {
            return PathBuf::from(value);
        }
    }
    let here = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    here.ancestors().find(|folder| folder.join("hitl").is_file() && folder.join("rust").is_dir()).unwrap_or(&here).to_path_buf()
}

/// The file `hitl` at the root of the project.
pub fn entry() -> PathBuf {
    root().join("hitl")
}

/// How a chat calls the command: by its link among the shared scripts when there is one (`HITL_COMMAND`, which
/// the script exports, is that), else the project's own file.
pub fn command(words: &[&str]) -> String {
    let base = match crate::var("HITL_COMMAND").filter(|value| !value.is_empty()) {
        Some(named) => named,
        None => {
            let link = user_home().join("Code/scripts/hitl");
            if link.exists() { link.display().to_string() } else { entry().display().to_string() }
        }
    };
    std::iter::once(base).chain(words.iter().map(|word| word.to_string())).collect::<Vec<_>>().join(" ")
}

/// `~/` and `~` of a path as the shell writes them.
pub fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => user_home().join(rest),
        None if path == "~" => user_home(),
        None => Path::new(path).to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_is_named_by_conveyor_home() {
        // SAFETY: the only test of the crate that touches the environment.
        unsafe { std::env::set_var("CONVEYOR_HOME", "/tmp/made-up-conveyor-home") };
        assert_eq!(home(), PathBuf::from("/tmp/made-up-conveyor-home"));
        assert_eq!(state("queue"), PathBuf::from("/tmp/made-up-conveyor-home/queue"));
        assert_eq!(iphone("ABC"), PathBuf::from("/tmp/made-up-conveyor-home/iphone/ABC"));
        unsafe { std::env::set_var("CONVEYOR_HOME", "") };
        assert!(home().ends_with(".hitl") || home().ends_with(".conveyor"));
    }

    #[test]
    fn tilde_is_expanded() {
        assert_eq!(expand("/a/b"), PathBuf::from("/a/b"));
        assert!(!expand("~/x").starts_with("~"));
    }
}
