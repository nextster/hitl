//! git, as the Python `common.git` runs it: what it printed, trimmed, or "" when it failed or took over ten seconds.

use crate::process::{Options, run};
use std::path::Path;

pub fn git(arguments: &[&str]) -> String {
    match run("git", arguments, &Options::new("git").timeout(10.0)) {
        Ok(done) if done.success() => done.out().trim().to_string(),
        _ => String::new(),
    }
}

/// `git -C FOLDER …`.
pub fn git_in(folder: &Path, arguments: &[&str]) -> String {
    let folder = folder.to_string_lossy().to_string();
    let mut all = vec!["-C", folder.as_str()];
    all.extend_from_slice(arguments);
    git(&all)
}

/// The repository a folder is a checkout of, by the git folder its worktrees share; "" for none.
pub use hitl::common::{project_name, repository_of};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_git_says_nothing() {
        assert_eq!(git_in(Path::new("/nonexistent-folder"), &["rev-parse", "HEAD"]), "");
    }
}
