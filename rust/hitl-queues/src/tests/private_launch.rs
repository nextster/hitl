//! Protected input is exercised only against the made-up phone and temporary queue.
use super::support::*;
use crate::{base::FileLock, dispatcher::Dispatcher, private_launch_env as private};
use serde_json::{Value, json};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};

const SECRET: &str = "sentinel-NotARealPassword-\"quoted\"\nline";
fn source(case: &Case) -> PathBuf {
    let folder = case.sandbox.folder.join("private-input");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&folder)
        .unwrap();
    let path = folder.join("input.json");
    write(
        &path,
        &json!({"version":1,"bundleID":"dev.example.demo","udid":UDID,
        "variables":{"REALIZE_DEV_PASSWORD":SECRET,"REALIZE_DEV_EMAIL":"private@example.invalid"}}),
    );
    path
}
fn write(path: &Path, value: &Value) {
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    f.write_all(value.to_string().as_bytes()).unwrap();
}
fn submit(case: &mut Case, path: &Path) -> String {
    case.submit_as(
        "secret",
        &[],
        "",
        "secret",
        &["--launch-env-file", path.to_str().unwrap()],
    )
}
fn private_path(case: &Case, ticket: &Value) -> PathBuf {
    case.queue
        .folder
        .join("private-launch")
        .join(ticket["launchEnvRef"].as_str().unwrap())
}
fn assert_no_secret(text: &str) {
    assert!(
        !text.contains("sentinel-NotARealPassword"),
        "private value escaped its boundary"
    );
    assert!(
        !text.contains("private@example.invalid"),
        "private value escaped its boundary"
    );
}
#[test]
fn immutable_snapshot_survives_source_removal_and_new_dispatcher_then_next_ticket_is_empty() {
    let mut c = Case::new("private-new-worker");
    let p = source(&c);
    let id = submit(&mut c, &p);
    let ticket = c.state()["queue"][0].clone();
    let snapshot = private_path(&c, &ticket);
    assert_eq!(
        std::fs::metadata(&snapshot).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(snapshot.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    write(&p, &json!({}));
    std::fs::remove_file(p).unwrap();
    c.dispatcher = Dispatcher::new(c.queue.clone(), c.device.clone()).with_sleep(|_| {});
    c.step();
    assert_eq!(
        c.device
            .with(|s| s.launch_environments[0]["REALIZE_DEV_PASSWORD"].clone()),
        SECRET
    );
    assert_no_secret(&c.state().to_string());
    assert_no_secret(&format!("{:?}", c.device.launches()));
    c.report(&id, &[], true, None);
    c.step();
    assert!(!snapshot.exists());
    c.submit("next", &[]);
    c.step();
    assert!(c.device.with(|s| s.launch_environments[1].is_empty()));
}
#[test]
fn unknown_old_worker_refuses_private_ticket_while_updated_existing_worker_accepts() {
    let mut c = Case::new("private-existing-worker");
    let p = source(&c);
    std::fs::create_dir_all(&c.queue.folder).unwrap();
    let _old = FileLock::exclusive(&c.queue.folder.join("dispatcher.lock"));
    let app = c.app.to_str().unwrap();
    let (code, out, err) = c.run_cli(&[
        "check",
        "--udid",
        UDID,
        "--app",
        app,
        "--bundle-id",
        "dev.example.demo",
        "--item",
        "Check",
        "--no-wait",
        "--launch-env-file",
        p.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    assert!(c.state()["queue"].as_array().unwrap().is_empty());
    assert_no_secret(&(out + &err));
    let _new = FileLock::exclusive(&c.queue.folder.join("private-launch-v1.lock"));
    submit(&mut c, &p);
    c.step();
    assert_eq!(c.device.launches().len(), 1);
}
#[test]
fn mismatch_or_missing_private_input_fails_before_any_device_call() {
    for key in [
        "ticketID",
        "bundleID",
        "udid",
        "version",
        "missing",
        "ref-version",
        "unicode-ref",
    ] {
        let mut c = Case::new("private-binding");
        let p = source(&c);
        submit(&mut c, &p);
        let ticket = c.state()["queue"][0].clone();
        let path = private_path(&c, &ticket);
        if key == "missing" {
            std::fs::remove_file(&path).unwrap();
        } else if key == "ref-version" || key == "unicode-ref" {
            c.queue.locked(|s| {
                if key == "ref-version" {
                    s["queue"][0]["launchEnvVersion"] = json!(2);
                } else {
                    s["queue"][0]["launchEnvRef"] = json!("é".repeat(24) + ".json");
                }
            });
        } else {
            let mut v: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            v[key] = json!("wrong");
            write(&path, &v);
        }
        c.step();
        assert!(c.device.installs().is_empty());
        assert!(c.device.launches().is_empty());
        assert_eq!(c.state()["recent"][0]["state"], "failed");
        assert!(!path.exists());
    }
}
#[test]
fn unsafe_files_and_variable_names_are_rejected_without_echoing_input() {
    let c = Case::new("private-file-policy");
    let p = source(&c);
    let good = std::fs::read(&p).unwrap();
    assert!(private::validate_source(&p).is_ok());
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(private::validate_source(&p).is_err());
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
    let link = p.with_file_name("symlink");
    symlink(&p, &link).unwrap();
    assert!(private::validate_source(&link).is_err());
    let hard = p.with_file_name("hard");
    std::fs::hard_link(&p, &hard).unwrap();
    assert!(private::validate_source(&p).is_err());
    std::fs::remove_file(hard).unwrap();
    let dirlink = p.parent().unwrap().with_file_name("dir-link");
    symlink(p.parent().unwrap(), &dirlink).unwrap();
    assert!(private::validate_source(&dirlink.join("input.json")).is_err());
    std::fs::set_permissions(p.parent().unwrap(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(private::validate_source(&p).is_err());
    std::fs::set_permissions(p.parent().unwrap(), std::fs::Permissions::from_mode(0o700)).unwrap();
    for name in [
        "DEVICECTL_CHILD_PASSWORD",
        "DYLD_INSERT_LIBRARIES",
        "PATH",
        "HOME",
        "LD_PRELOAD",
        "SIMCTL_CHILD_KEY",
        "badName",
        "HITL_HOME",
    ] {
        let mut v: Value = serde_json::from_slice(&good).unwrap();
        v["variables"] = json!({name:SECRET});
        write(&p, &v);
        let e = private::validate_source(&p).unwrap_err();
        assert_no_secret(e);
    }
    for value in [
        json!(true),
        json!(""),
        json!("bad\u{0}value"),
        json!("a".repeat(8193)),
    ] {
        let mut v: Value = serde_json::from_slice(&good).unwrap();
        v["variables"]["REALIZE_DEV_PASSWORD"] = value;
        write(&p, &v);
        assert!(private::validate_source(&p).is_err());
    }
    std::fs::write(&p, b"{").unwrap();
    assert!(private::validate_source(&p).is_err());
    std::fs::write(&p, vec![b'x'; 65537]).unwrap();
    assert!(private::validate_source(&p).is_err());
    std::fs::remove_file(&p).unwrap();
    use std::os::unix::ffi::OsStrExt;
    let name = std::ffi::CString::new(p.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    assert!(private::validate_source(&p).is_err());
}
#[test]
fn cancel_supersede_terminal_failures_and_done_remove_only_their_private_snapshot() {
    for how in ["cancel", "supersede", "install", "launch", "done"] {
        let mut c = Case::new("private-cleanup");
        let p = source(&c);
        let id = submit(&mut c, &p);
        let path = private_path(&c, &c.state()["queue"][0]);
        match how {
            "cancel" => {
                assert_eq!(c.run_cli(&["drop", "--udid", UDID, &id]).0, 0);
            }
            "supersede" => {
                submit(&mut c, &p);
            }
            "install" => {
                c.device
                    .with(|s| s.install_result = (false, "failed".into()));
                for _ in 0..3 {
                    c.step();
                }
            }
            "launch" => {
                c.device
                    .with(|s| s.launch_result = Some((false, SECRET.into())));
                for _ in 0..3 {
                    c.step();
                }
                assert_no_secret(&c.state().to_string());
            }
            _ => {
                c.step();
                c.report(&id, &[], true, None);
                c.step();
            }
        }
        assert!(!path.exists(), "cleanup {how}");
        assert!(p.exists());
    }
}
#[test]
fn pause_requeue_restart_keep_live_inputs_and_prune_only_orphans() {
    let mut c = Case::new("private-pause");
    let p = source(&c);
    submit(&mut c, &p);
    let ticket = c.state()["queue"][0].clone();
    let path = private_path(&c, &ticket);
    let orphan = path.with_file_name("a".repeat(48) + ".json");
    std::fs::copy(&path, &orphan).unwrap();
    c.as_artem();
    assert_eq!(c.run_cli(&["pause", "--udid", UDID]).0, 0);
    c.step();
    assert!(path.exists());
    assert!(!orphan.exists());
    assert!(c.device.installs().is_empty());
    c.run_cli(&["resume", "--udid", UDID]);
    c.step();
    assert!(path.exists());
    c.run_cli(&["pause", "--udid", UDID]);
    assert!(path.exists());
    c.dispatcher = Dispatcher::new(c.queue.clone(), c.device.clone());
    assert!(private::load(&c.queue, &ticket).is_ok());
}
#[test]
fn report_file_and_attached_report_redact_strings_and_escaped_output_before_persistence() {
    let mut c = Case::new("private-redaction");
    let p = source(&c);
    let id = submit(&mut c, &p);
    let ticket = c.state()["queue"][0].clone();
    c.step();
    let env = private::load(&c.queue, &ticket).unwrap();
    assert_no_secret(&env.redact(&serde_json::to_string(SECRET).unwrap()));
    c.report(
        &id,
        &[("1", Some("problem"), Some(SECRET))],
        false,
        Some(SECRET),
    );
    c.step();
    let file = c.queue.ticket_folder(&id).join("phone.json");
    assert_no_secret(&std::fs::read_to_string(&file).unwrap());
    let heard = c.device.with(|s| s.heard.clone().unwrap());
    heard(json!({"id":id,"results":[],"comment":SECRET,"current":SECRET,"currentAt":100}));
    c.report(
        &id,
        &[("1", Some("problem"), Some(SECRET))],
        true,
        Some(SECRET),
    );
    c.step();
    assert_no_secret(&c.state().to_string());
    assert_no_secret(
        &std::fs::read_to_string(c.queue.ticket_folder(&id).join("verdict.json")).unwrap(),
    );
}
