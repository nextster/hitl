//! A program started through `hitl_host_core::process` leaves a record in the action journal of the made-up home,
//! and none with `CONVEYOR_JOURNAL=0`. One test binary for each case: the environment is the process's.

use hitl_host_core::process::{Options, run};
use std::path::{Path, PathBuf};

/// The tests share the process's environment and the one writer: one at a time.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn made_up_home(name: &str) -> PathBuf {
    let home = std::env::temp_dir().join(format!("hitl-host-core-journal-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    home
}

fn rows(home: &Path) -> Vec<(String, String)> {
    let database = home.join("journal/events.sqlite3");
    if !database.exists() {
        return Vec::new();
    }
    let connection = rusqlite::Connection::open(&database).unwrap();
    let mut statement = connection.prepare("SELECT kind, outcome FROM events ORDER BY id").unwrap();
    statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap().map(Result::unwrap).collect()
}

#[test]
fn a_launch_and_its_result_are_written() {
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let home = made_up_home("on");
    // SAFETY: the only test of this binary.
    unsafe {
        std::env::set_var("CONVEYOR_HOME", &home);
        std::env::remove_var("CONVEYOR_JOURNAL");
    }
    let done = run("true", &[], &Options::new("test.journal")).unwrap();
    assert!(done.success());
    hitl_host_core::journal::shutdown();
    let found = rows(&home);
    assert!(found.contains(&("native.process.launch".into(), "spawned".into())), "{found:?}");
    assert!(found.contains(&("native.process.result".into(), "ok".into())), "{found:?}");
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn an_operation_leaves_its_begin_and_end_and_a_failure_is_an_error() {
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let home = made_up_home("operation");
    // SAFETY: the tests of this binary set the same variables to the same values.
    unsafe {
        std::env::set_var("CONVEYOR_HOME", &home);
        std::env::remove_var("CONVEYOR_JOURNAL");
    }
    let sent = hitl_host_core::journal::operation("native.send", "native.Test", "chat-1", || "", |_| None);
    assert_eq!(sent, "");
    let refused = hitl_host_core::journal::operation(
        "native.send", "native.Test", "chat-2", || "no such chat".to_string(),
        |why| Some(hitl_host_core::journal::Failure::refused(why, false)));
    assert_eq!(refused, "no such chat");
    hitl_host_core::journal::emit(&serde_json::json!({"kind": "hitl.queued", "source": "hitl", "request": "t-1", "context": {"items": 2}}));
    hitl_host_core::journal::shutdown();
    let found = rows(&home);
    assert!(found.contains(&("native.send".into(), "ok".into())), "{found:?}");
    assert!(found.contains(&("native.send".into(), "error".into())), "{found:?}");
    assert!(found.contains(&("hitl.queued".into(), "".into())), "{found:?}");
    let _ = std::fs::remove_dir_all(&home);
}
