//! With `CONVEYOR_JOURNAL=0` a program started through `hitl_host_core::process` leaves nothing: no file either.

use hitl_host_core::process::{Options, run};

#[test]
fn nothing_is_written_when_the_journal_is_off() {
    let home = std::env::temp_dir().join(format!("hitl-host-core-journal-off-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    // SAFETY: the only test of this binary.
    unsafe {
        std::env::set_var("CONVEYOR_HOME", &home);
        std::env::set_var("CONVEYOR_JOURNAL", "0");
    }
    let done = run("true", &[], &Options::new("test.journal_off")).unwrap();
    assert!(done.success());
    hitl_host_core::journal::shutdown();
    assert!(!home.join("journal").exists());
    let _ = std::fs::remove_dir_all(&home);
}
