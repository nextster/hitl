//! Where hitl's events of the action journal go: to the journal set in the process (`set_journal`) as they happen,
//! with no program started for them; without one, kept and handed at the end to `HITL_HOOK _hitl journal`. One test
//! in this binary: the environment and the journal set are the process's.

use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

static WRITTEN: Mutex<Vec<Value>> = Mutex::new(Vec::new());
static FINISHED: AtomicUsize = AtomicUsize::new(0);

fn write(event: &Value) {
    WRITTEN.lock().unwrap().push(event.clone());
}

fn finish() {
    FINISHED.fetch_add(1, Ordering::SeqCst);
}

#[test]
fn events_go_to_the_journal_set_in_the_process_and_no_hook_is_started() {
    let folder = std::env::temp_dir().join(format!("hitl-journal-tests-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    // A made-up hitl: what it is handed through the hook is written beside it.
    let hook = folder.join("hitl");
    std::fs::write(&hook, format!("#!/bin/sh\necho \"$@\" >> '{0}/called'\ncat >> '{0}/handed'\necho '{{}}'\n", folder.display())).unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    // SAFETY: the only test of this binary.
    unsafe {
        std::env::set_var("HITL_TOOL", "hitl-host");
        std::env::set_var("HITL_HOOK", &hook);
        std::env::set_var("CONVEYOR_HOME", folder.join("home"));
        std::env::remove_var("CONVEYOR_JOURNAL");
    }

    // No journal set: the events are kept and handed to hitl's hook at the end, as before.
    hitl::common::emit("hitl.before", json!({"request": "r0", "source": "hitl"}));
    hitl::common::flush_events();
    assert_eq!(std::fs::read_to_string(folder.join("called")).unwrap().trim(), "_hitl journal");
    let handed: Value = serde_json::from_str(&std::fs::read_to_string(folder.join("handed")).unwrap()).unwrap();
    assert_eq!(handed[0]["kind"], "hitl.before");
    std::fs::remove_file(folder.join("called")).unwrap();

    // A journal set in the process: every event goes to it at once, with its kind, its moment and its fields.
    hitl::common::set_journal(write, finish);
    let before = hitl::common::now();
    hitl::common::emit("hitl.queued", json!({"request": "1030-1", "source": "hitl", "outcome": "queued", "subject": "s", "error": "",
                                             "context": {"kind": "plan", "items": 2}}));
    let written = WRITTEN.lock().unwrap().clone();
    assert_eq!(written.len(), 1, "{written:?}");
    let event = &written[0];
    assert_eq!(event["kind"], "hitl.queued");
    assert!(event["at"].as_f64().is_some_and(|at| at >= before && at <= hitl::common::now()));
    for (key, value) in [("request", json!("1030-1")), ("source", json!("hitl")), ("outcome", json!("queued")), ("subject", json!("s")),
                         ("error", json!("")), ("context", json!({"kind": "plan", "items": 2}))] {
        assert_eq!(event[key], value, "{key}");
    }
    // The end of the run lets the journal finish; nothing is left for the hook, and no hook is started.
    hitl::common::flush_events();
    assert_eq!(FINISHED.load(Ordering::SeqCst), 1);
    assert!(!folder.join("called").exists(), "no program was started for the journal");

    // Off, and outside hitl, there is no journal at all.
    unsafe { std::env::set_var("CONVEYOR_JOURNAL", "0") };
    hitl::common::emit("hitl.off", json!({}));
    unsafe {
        std::env::remove_var("CONVEYOR_JOURNAL");
        std::env::remove_var("HITL_TOOL");
    }
    hitl::common::emit("hitl.team", json!({}));
    assert_eq!(WRITTEN.lock().unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(&folder);
}
