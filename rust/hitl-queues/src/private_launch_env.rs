//! Per-ticket launch input. Values never belong in queue state, arguments or a worker's environment.
use crate::state::Queue;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::Path;

pub const ERROR: &str =
    "Protected launch input is missing, unsafe or does not match this ticket, app and device.";
const LIMIT: u64 = 65_536;

// Deliberately no Debug: neither errors nor command diagnostics may print these values.
#[derive(Clone, Default)]
pub struct LaunchEnvironment {
    variables: BTreeMap<String, String>,
}
impl LaunchEnvironment {
    pub fn variables(&self) -> &BTreeMap<String, String> {
        &self.variables
    }
    pub fn is_empty(&self) -> bool {
        self.variables.is_empty()
    }
    pub fn redact(&self, text: &str) -> String {
        let mut patterns: Vec<String> = self
            .variables
            .values()
            .flat_map(|value| {
                let escaped = serde_json::to_string(value).unwrap_or_default();
                vec![
                    value.clone(),
                    escaped[1..escaped.len().saturating_sub(1)].to_string(),
                ]
            })
            .collect();
        patterns.sort_by_key(|value| std::cmp::Reverse(value.len()));
        patterns.dedup();
        patterns.into_iter().fold(text.to_string(), |text, value| {
            text.replace(&value, "[REDACTED]")
        })
    }
    pub fn redact_value(&self, value: &Value) -> Value {
        match value {
            Value::String(text) => Value::String(self.redact(text)),
            Value::Array(values) => {
                Value::Array(values.iter().map(|v| self.redact_value(v)).collect())
            }
            Value::Object(values) => Value::Object(
                values
                    .iter()
                    .map(|(k, v)| (self.redact(k), self.redact_value(v)))
                    .collect(),
            ),
            value => value.clone(),
        }
    }
}

fn valid_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b'A'..=b'Z' | b'_'))
        && name.len() <= 64
        && bytes.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
        && ![
            "DEVICECTL_",
            "SIMCTL_",
            "DYLD_",
            "LD_",
            "XPC_",
            "__XPC_",
            "CONVEYOR_",
            "HITL_",
        ]
        .iter()
        .any(|p| name.starts_with(p))
        && ![
            "PATH",
            "HOME",
            "SHELL",
            "USER",
            "LOGNAME",
            "TMPDIR",
            "TMP",
            "TEMP",
            "ENV",
            "BASH_ENV",
            "IFS",
            "SHELLOPTS",
            "RUST_LOG",
            "RUST_BACKTRACE",
        ]
        .contains(&name)
}
fn decode(
    value: &Value,
    ticket: Option<&str>,
    bundle: &str,
    udid: &str,
) -> Result<LaunchEnvironment, &'static str> {
    let object = value.as_object().ok_or(ERROR)?;
    if object.len() != if ticket.is_some() { 5 } else { 4 }
        || value["version"].as_u64() != Some(1)
        || value["bundleID"].as_str() != Some(bundle)
        || value["udid"].as_str() != Some(udid)
        || ticket.is_some_and(|id| value["ticketID"].as_str() != Some(id))
    {
        return Err(ERROR);
    }
    let vars = value["variables"]
        .as_object()
        .filter(|v| !v.is_empty() && v.len() <= 16)
        .ok_or(ERROR)?;
    let mut variables = BTreeMap::new();
    for (name, value) in vars {
        let text = value
            .as_str()
            .filter(|v| !v.is_empty() && v.len() <= 8192 && !v.contains('\0'))
            .ok_or(ERROR)?;
        if !valid_name(name) {
            return Err(ERROR);
        }
        variables.insert(name.clone(), text.to_string());
    }
    Ok(LaunchEnvironment { variables })
}

fn private_dir(path: &Path, create: bool) -> Result<File, &'static str> {
    if create {
        match std::fs::DirBuilder::new().mode(0o700).create(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(ERROR),
        }
    }
    let dir = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_DIRECTORY)
        .open(path)
        .map_err(|_| ERROR)?;
    let m = dir.metadata().map_err(|_| ERROR)?;
    if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o7777 != 0o700 {
        return Err(ERROR);
    }
    Ok(dir)
}
fn read_at(dir: &File, name: &std::ffi::OsStr) -> Result<Value, &'static str> {
    use std::os::unix::ffi::OsStrExt;
    let name = std::ffi::CString::new(name.as_bytes()).map_err(|_| ERROR)?;
    // O_NONBLOCK prevents a substituted FIFO from hanging validation before fstat.
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        return Err(ERROR);
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    let m = file.metadata().map_err(|_| ERROR)?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o7777 != 0o600
        || m.nlink() != 1
        || m.len() > LIMIT
    {
        return Err(ERROR);
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ERROR)?;
    if bytes.len() as u64 > LIMIT {
        return Err(ERROR);
    }
    serde_json::from_slice(&bytes).map_err(|_| ERROR)
}
fn source(path: &Path) -> Result<Value, &'static str> {
    if !path.is_absolute() {
        return Err(ERROR);
    }
    let dir = private_dir(path.parent().ok_or(ERROR)?, false)?;
    read_at(&dir, path.file_name().ok_or(ERROR)?)
}
pub fn validate_source(path: &Path) -> Result<(), &'static str> {
    let value = source(path)?;
    let bundle = value["bundleID"]
        .as_str()
        .filter(|v| !v.is_empty())
        .ok_or(ERROR)?;
    let udid = value["udid"]
        .as_str()
        .filter(|v| !v.is_empty())
        .ok_or(ERROR)?;
    decode(&value, None, bundle, udid).map(|_| ())
}
pub fn snapshot(queue: &Queue, path: &Path, ticket: &mut Value) -> Result<(), &'static str> {
    let mut value = source(path)?;
    decode(
        &value,
        None,
        crate::base::text(ticket, "bundleId"),
        &queue.udid,
    )?;
    value["ticketID"] = ticket["id"].clone();
    let bytes = serde_json::to_vec(&value).map_err(|_| ERROR)?;
    if bytes.len() as u64 > LIMIT {
        return Err(ERROR);
    }
    let dir = private_dir(&queue.folder.join("private-launch"), true)?;
    let mut random = [0u8; 24];
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut random))
        .map_err(|_| ERROR)?;
    let reference = random
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
        + ".json";
    let name = std::ffi::CString::new(reference.as_str()).map_err(|_| ERROR)?;
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(ERROR);
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    if file
        .write_all(&bytes)
        .and_then(|_| file.sync_all())
        .is_err()
    {
        unsafe { libc::unlinkat(dir.as_raw_fd(), name.as_ptr(), 0) };
        return Err(ERROR);
    }
    ticket["launchEnvRef"] = json!(reference);
    ticket["launchEnvVersion"] = json!(1);
    Ok(())
}
fn reference(ticket: &Value) -> Result<Option<&str>, &'static str> {
    if ticket.get("launchEnvRef").is_none() && ticket.get("launchEnvVersion").is_none() {
        return Ok(None);
    }
    let name = ticket["launchEnvRef"].as_str().ok_or(ERROR)?;
    if ticket["launchEnvVersion"].as_u64() != Some(1)
        || name.len() != 53
        || !name.ends_with(".json")
        || !name.as_bytes()[..48]
            .iter()
            .copied()
            .all(|c| c.is_ascii_hexdigit())
    {
        return Err(ERROR);
    }
    Ok(Some(name))
}
pub fn load(queue: &Queue, ticket: &Value) -> Result<LaunchEnvironment, &'static str> {
    let Some(name) = reference(ticket)? else {
        return Ok(LaunchEnvironment::default());
    };
    let dir = private_dir(&queue.folder.join("private-launch"), false)?;
    let value = read_at(&dir, std::ffi::OsStr::new(name))?;
    decode(
        &value,
        Some(crate::base::text(ticket, "id")),
        crate::base::text(ticket, "bundleId"),
        &queue.udid,
    )
}
pub fn prune(queue: &Queue, state: &Value) {
    let folder = queue.folder.join("private-launch");
    let Ok(dir) = private_dir(&folder, false) else {
        return;
    };
    let mut keep: Vec<&str> = state["queue"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| reference(t).ok().flatten())
        .collect();
    if let Some(active) = state.get("active").filter(|v| v.is_object()) {
        if let Ok(Some(name)) = reference(active) {
            keep.push(name);
        }
    }
    for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
        let filename = entry.file_name();
        let Some(name) = filename.to_str() else {
            continue;
        };
        // Never follow a ref, path or directory supplied by a ticket during cleanup.
        if name.len() != 53
            || !name.ends_with(".json")
            || !name.as_bytes()[..48]
                .iter()
                .copied()
                .all(|c| c.is_ascii_hexdigit())
            || keep.contains(&name)
        {
            continue;
        }
        if let Ok(name) = std::ffi::CString::new(name) {
            unsafe { libc::unlinkat(dir.as_raw_fd(), name.as_ptr(), 0) };
        }
    }
}
/// Environment on *each child Command*, never on the worker process.
pub fn configure_child(command: &mut std::process::Command, environment: &LaunchEnvironment) {
    for (name, _) in
        std::env::vars_os().filter(|(n, _)| n.to_string_lossy().starts_with("DEVICECTL_CHILD_"))
    {
        command.env_remove(name);
    }
    for (name, value) in environment.variables() {
        command.env(format!("DEVICECTL_CHILD_{name}"), value);
    }
}

#[cfg(test)]
mod child_tests {
    use super::*;
    use crate::base::testing::Sandbox;
    use crate::device::{Device, Devicectl, Heard};
    use std::os::unix::fs::PermissionsExt;
    use std::sync::{Arc, Mutex};

    #[test]
    fn actual_fake_children_get_only_current_values_both_launch_branches_and_redact_early_errors() {
        let mut sandbox = Sandbox::new("private-child");
        sandbox.set("DEVICECTL_CHILD_OLD_PASSWORD", "old-sentinel");
        sandbox.set("CONVEYOR_JOURNAL", "0");
        let script = sandbox.folder.join("fake-devicectl");
        let body = r#"#!/usr/bin/python3
import os,sys,json
assert os.environ.get('DEVICECTL_CHILD_OLD_PASSWORD') is None
secret=os.environ.get('DEVICECTL_CHILD_REALIZE_PASSWORD')
if '--plain' in sys.argv:
 assert secret is None
 print('Launched application')
 sys.exit(0)
assert secret=='secret-with-"quotes"-sentinel'
assert not any(secret in a for a in sys.argv)
if '--console' in sys.argv:
 print('\x1eios-run-check '+json.dumps({'id':'ticket','comment':secret}),flush=True)
 sys.exit(0)
print('early failure: '+json.dumps(secret),file=sys.stderr)
sys.exit(1)
"#;
        std::fs::write(&script, body).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let device =
            Devicectl::with_program("TEST", script.to_str().unwrap()).with_launch_seconds(1.0);
        let environment = LaunchEnvironment {
            variables: BTreeMap::from([(
                "REALIZE_PASSWORD".into(),
                "secret-with-\"quotes\"-sentinel".into(),
            )]),
        };
        let (ok, error) = device.launch("own.bundle", &[], None, None, &environment);
        assert!(!ok);
        assert!(error.contains("output suppressed"));
        assert!(!error.contains("sentinel"));
        let reports = Arc::new(Mutex::new(Vec::new()));
        let saved = reports.clone();
        let heard: Heard = Arc::new(move |v| saved.lock().unwrap().push(v));
        assert!(
            device
                .launch("own.bundle", &[], None, Some(heard), &environment)
                .0
        );
        device.detach();
        assert_eq!(reports.lock().unwrap()[0]["comment"], "[REDACTED]");
        assert!(
            device
                .launch(
                    "next.bundle",
                    &["--plain".into()],
                    None,
                    None,
                    &Default::default()
                )
                .0
        );
        assert!(std::env::var("DEVICECTL_CHILD_REALIZE_PASSWORD").is_err());
        assert_eq!(
            std::env::var("DEVICECTL_CHILD_OLD_PASSWORD").unwrap(),
            "old-sentinel"
        );
        // A dispatcher child is scrubbed independently from the per-launch child.
        let mut child = std::process::Command::new(&script);
        child.arg("--plain");
        configure_child(&mut child, &Default::default());
        assert!(child.output().unwrap().status.success());
    }
}
