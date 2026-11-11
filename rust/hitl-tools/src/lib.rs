//! Installation, updates, test runs and HITL diagnostics.

pub mod install;
pub mod journal;
pub mod stats;
pub mod testing;
pub mod update;

pub fn dispatch(command: &str, rest: &[String]) -> Option<i32> {
    [update::dispatch, install::dispatch, testing::dispatch, stats::dispatch, journal::dispatch]
        .into_iter().find_map(|module: fn(&str, &[String]) -> Option<i32>| module(command, rest))
}
