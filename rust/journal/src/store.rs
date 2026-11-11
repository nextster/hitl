use crate::{codec, json};
use rusqlite::{Connection, Error, ErrorCode, params_from_iter, types::Value};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, BufRead},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const COLUMNS: &str = "at,process,seq,identity,actor,source,kind,phase,trace,action,parent,request,attempt,subject,outcome,error,duration_ms,input_tokens,output_tokens,cost_nano_usd,usage_known,context";
const MAX_BYTES: u64 = 128 * 1024 * 1024;
type Loss = Arc<Mutex<BTreeMap<String, i64>>>;
fn wall() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}
fn token() -> Result<String, i32> {
    let mut b = [0u8; 6];
    if unsafe { libc::getentropy(b.as_mut_ptr().cast(), b.len()) } != 0 {
        return Err(1);
    }
    Ok(format!(
        "{}-{}",
        std::process::id(),
        b.iter().map(|v| format!("{v:02x}")).collect::<String>()
    ))
}
fn loss(lost: &Loss, key: &str, count: i64) {
    *lost.lock().unwrap().entry(key.into()).or_default() += count;
}

/// The writer inside a process that produces events itself (hitl's own programs): the same queue, worker and
/// rows as the program `hitl-journal`, fed with one event line at a time instead of a pipe. Best effort: nothing
/// it does can fail the caller.
pub struct Writer {
    disabled: bool,
    send: mpsc::SyncSender<Vec<Value>>,
    receive: MutexReceiver,
    lost: Loss,
    home: PathBuf,
    process: String,
    seq: i64,
    worker: Option<mpsc::Receiver<()>>,
}
impl Writer {
    /// A writer into `home/journal`; `disabled` writes nothing (`CONVEYOR_JOURNAL=0`). Err is a program's exit code.
    pub fn new(home: &Path, disabled: bool) -> Result<Writer, i32> {
        let (send, receive) = mpsc::sync_channel(512);
        Ok(Writer {
            disabled,
            send,
            receive: Mutex::new(Some(receive)),
            lost: Arc::new(Mutex::new(BTreeMap::new())),
            home: home.to_path_buf(),
            process: token()?,
            seq: 0,
            worker: None,
        })
    }
    /// One event as JSON, as the pipe takes it (the same kinds and fields; a line over 8192 characters is dropped).
    pub fn submit(&mut self, event: &str) {
        let mut line = event.as_bytes().to_vec();
        line.push(b'\n');
        submit(&line, self.disabled, &self.send, &self.receive, &self.lost, &self.home, &self.process,
               &mut self.seq, &mut self.worker);
    }
    /// One general event (`general.rs`): the object a producer of hitl itself builds, not the pipe's line.
    pub fn submit_event(&mut self, event: &serde_json::Value) {
        if self.disabled {
            return;
        }
        let row = crate::general::row(event, wall());
        enqueue(row, &self.send, &self.receive, &self.lost, &self.home, &self.process, &mut self.seq, &mut self.worker);
    }
    /// What `run` does at EOF: lets the worker write what it holds, for up to two bounded drains of 200 ms.
    pub fn close(self) {
        drop(self.send);
        if let Some(done) = self.worker {
            if matches!(done.recv_timeout(Duration::from_millis(200)), Err(mpsc::RecvTimeoutError::Timeout)) {
                let _ = done.recv_timeout(Duration::from_millis(200));
            }
        }
    }
}

pub fn run() -> Result<(), i32> {
    let disabled = std::env::var("CONVEYOR_JOURNAL").as_deref() == Ok("0");
    let home = std::env::var_os("CONVEYOR_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".conveyor")))
        .ok_or(64)?;
    let process = token()?;
    let (send, receive) = mpsc::sync_channel(512);
    let receive = Mutex::new(Some(receive));
    let lost: Loss = Arc::new(Mutex::new(BTreeMap::new()));
    let mut worker = None;
    let mut seq: i64 = 0;
    // sys.stdin iterates LF-delimited lines; CR is JSON whitespace, not a frame.
    let mut input = io::stdin().lock();
    let mut line = Vec::new();
    let mut too_long = false;
    loop {
        let available = input.fill_buf().map_err(|_| 1)?;
        if available.is_empty() {
            if !line.is_empty() && !too_long {
                submit(
                    &line,
                    disabled,
                    &send,
                    &receive,
                    &lost,
                    &home,
                    &process,
                    &mut seq,
                    &mut worker,
                );
            }
            break;
        }
        let size = available.len();
        let mut used = 0;
        for &b in available {
            used += 1;
            if b == b'\n' {
                line.push(b'\n');
                if !too_long {
                    submit(
                        &line,
                        disabled,
                        &send,
                        &receive,
                        &lost,
                        &home,
                        &process,
                        &mut seq,
                        &mut worker,
                    );
                }
                line.clear();
                too_long = false;
                break;
            } else if !too_long {
                line.push(b);
                // 8192 Python characters need at most 32768 valid UTF-8 bytes.
                if line.len() > 32768 {
                    too_long = true;
                    line.clear();
                }
            }
        }
        input.consume(if used == 0 { size } else { used });
    }
    drop(send);
    if let Some(done) = worker {
        // Python explicitly closes at EOF and again via atexit. Preserve its
        // two bounded drain opportunities; neither acknowledges durable input.
        if matches!(
            done.recv_timeout(Duration::from_millis(200)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ) {
            let _ = done.recv_timeout(Duration::from_millis(200));
        }
    }
    Ok(())
}

// Receiver is moved once, lazily on the first accepted event; no SQLite/thread
// on an empty, disabled or malformed input stream.
#[allow(clippy::too_many_arguments)]
fn submit(
    bytes: &[u8],
    disabled: bool,
    send: &mpsc::SyncSender<Vec<Value>>,
    receive: &MutexReceiver,
    lost: &Loss,
    home: &Path,
    process: &str,
    seq: &mut i64,
    worker: &mut Option<mpsc::Receiver<()>>,
) {
    if disabled {
        return;
    }
    let (line, count) = json::stdin_line(bytes);
    if count > 8192 {
        return;
    }
    let Ok(value) = json::parse(&line) else {
        return;
    };
    let Ok(Some(row)) = codec::event(&value, wall()) else {
        return;
    };
    enqueue(row, send, receive, lost, home, process, seq, worker);
}
#[allow(clippy::too_many_arguments)]
fn enqueue(
    mut row: Vec<Value>,
    send: &mpsc::SyncSender<Vec<Value>>,
    receive: &MutexReceiver,
    lost: &Loss,
    home: &Path,
    process: &str,
    seq: &mut i64,
    worker: &mut Option<mpsc::Receiver<()>>,
) {
    *seq += 1;
    row[1] = Value::Text(process.into());
    row[2] = Value::Integer(*seq);
    match send.try_send(row) {
        Ok(()) => {}
        Err(mpsc::TrySendError::Full(_)) => {
            loss(lost, "queue_full", 1);
            return;
        }
        Err(_) => return,
    }
    if worker.is_none() {
        if let Some(receiver) = receive.lock().unwrap().take() {
            let root = home.join("journal");
            let lost = Arc::clone(lost);
            let process = process.to_string();
            let (done, finished) = mpsc::channel();
            thread::spawn(move || {
                work(receiver, root, lost, process);
                let _ = done.send(());
            });
            *worker = Some(finished);
        }
    }
}
type MutexReceiver = Mutex<Option<mpsc::Receiver<Vec<Value>>>>;

struct Store {
    db: Connection,
    maintenance: f64,
    max_bytes: u64,
}
impl Store {
    fn connect(root: &Path, max_bytes: u64) -> rusqlite::Result<Self> {
        let io_error = |e| Error::ToSqlConversionFailure(Box::new(e));
        fs::create_dir_all(root).map_err(io_error)?;
        if !fs::symlink_metadata(root).map_err(io_error)?.is_dir() {
            return Err(Error::InvalidQuery);
        }
        fs::set_permissions(root, fs::Permissions::from_mode(0o700)).map_err(io_error)?;
        let path = root.join("events.sqlite3");
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
            .map_err(io_error)?;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(io_error)?;
        drop(file);
        let db = Connection::open(&path)?;
        db.busy_timeout(Duration::from_millis(25))?;
        let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if !matches!(version, 0 | 1) {
            return Err(Error::InvalidQuery);
        }
        db.execute_batch(
            "PRAGMA journal_mode=TRUNCATE; PRAGMA synchronous=NORMAL; PRAGMA journal_size_limit=0;",
        )?;
        let page_size: i64 = db.query_row("PRAGMA page_size", [], |r| r.get(0))?;
        let pages = 16.max((max_bytes / 2 - (65536.max(max_bytes / 128))) / page_size as u64);
        db.pragma_update(None, "max_page_count", pages as i64)?;
        db.execute_batch(include_str!("schema.sql"))?;
        Ok(Self {
            db,
            maintenance: 0.,
            max_bytes,
        })
    }
    fn write(
        &mut self,
        rows: &[Vec<Value>],
        lost: &BTreeMap<String, i64>,
        process: &str,
    ) -> rusqlite::Result<()> {
        let mut batch = vec![];
        if !lost.is_empty() {
            let mut e = codec::event(&json::parse("{\"kind\":\"journal.loss\"}").unwrap(), wall())
                .unwrap()
                .unwrap();
            e[1] = Value::Text(process.into());
            e[2] = Value::Integer(0);
            e[4] = Value::Text("automation".into());
            e[5] = Value::Text("journal".into());
            e[14] = Value::Text("lost".into());
            e[21] = Value::Text(serde_json::to_string(lost).unwrap());
            batch.push(e);
        }
        batch.extend_from_slice(rows);
        let sql = format!(
            "INSERT INTO events({COLUMNS}) VALUES({}) ON CONFLICT(identity) DO UPDATE SET trace=CASE WHEN events.trace='' THEN excluded.trace ELSE events.trace END, parent=CASE WHEN events.parent='' THEN excluded.parent ELSE events.parent END, duration_ms=COALESCE(events.duration_ms,excluded.duration_ms), input_tokens=COALESCE(events.input_tokens,excluded.input_tokens), output_tokens=COALESCE(events.output_tokens,excluded.output_tokens), cost_nano_usd=COALESCE(events.cost_nano_usd,excluded.cost_nano_usd), usage_known=MAX(COALESCE(events.usage_known,0),COALESCE(excluded.usage_known,0))",
            vec!["?"; 22].join(",")
        );
        for pressure in [false, true] {
            let tx = self.db.transaction()?;
            let result = (|| {
                let now = wall();
                if pressure || now - self.maintenance > 5. {
                    let deleted = if pressure {
                        let count: i64 =
                            tx.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))?;
                        tx.execute("DELETE FROM events WHERE id IN (SELECT id FROM events ORDER BY id LIMIT ?)",[64.max(count/16)])?
                    } else {
                        tx.execute("DELETE FROM events WHERE at < ?", [now - 14. * 86400.])?
                    };
                    if deleted > 0 {
                        tx.execute("INSERT INTO meta VALUES('pruned',?) ON CONFLICT(name) DO UPDATE SET value=value+excluded.value",[deleted as i64])?;
                    }
                    tx.execute(
                        "INSERT OR REPLACE INTO meta VALUES('retention_days',14)",
                        [],
                    )?;
                    tx.execute(
                        "INSERT OR REPLACE INTO meta VALUES('max_bytes',?)",
                        [self.max_bytes as i64],
                    )?;
                    self.maintenance = now;
                }
                let mut statement = tx.prepare_cached(&sql)?;
                for row in &batch {
                    statement.execute(params_from_iter(row))?;
                }
                Ok::<_, Error>(())
            })();
            let result = match result {
                Ok(()) => tx.commit(),
                Err(e) => {
                    drop(tx);
                    Err(e)
                }
            };
            match result {
                Ok(()) => return Ok(()),
                Err(Error::SqliteFailure(e, _)) if e.code == ErrorCode::DiskFull && !pressure => {}
                Err(e) => return Err(e),
            }
        }
        unreachable!()
    }
}
fn work(receive: mpsc::Receiver<Vec<Value>>, root: PathBuf, lost: Loss, process: String) {
    let mut store = None;
    while let Ok(first) = receive.recv() {
        let mut batch = vec![first];
        let deadline = Instant::now() + Duration::from_millis(20);
        while batch.len() < 64 {
            match receive.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(v) => batch.push(v),
                Err(_) => break,
            }
        }
        let previous = lost.lock().unwrap().clone();
        let mut success = false;
        for retry in 0..3 {
            let result = (|| {
                if store.is_none() {
                    store = Some(Store::connect(&root, MAX_BYTES)?);
                }
                store.as_mut().unwrap().write(&batch, &previous, &process)
            })();
            match result {
                Ok(()) => {
                    success = true;
                    break;
                }
                Err(Error::SqliteFailure(e, _))
                    if matches!(e.code, ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
                        && retry < 2 =>
                {
                    thread::sleep(Duration::from_millis(5))
                }
                Err(_) => {
                    store = None;
                    break;
                }
            }
        }
        if success {
            let mut l = lost.lock().unwrap();
            for (k, n) in previous {
                if let Some(v) = l.get_mut(&k) {
                    *v -= n;
                    if *v == 0 {
                        l.remove(&k);
                    }
                }
            }
        } else {
            loss(&lost, "storage_error", batch.len() as i64);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn private_store(max: u64) -> (PathBuf, Store) {
        let root = std::env::temp_dir().join(format!("conveyor-rust-store-{}", token().unwrap()));
        let store = Store::connect(&root, max).unwrap();
        (root, store)
    }
    fn row(identity: Option<&str>) -> Vec<Value> {
        let mut e = codec::event(
            &json::parse("{\"kind\":\"hitl.mark\",\"at\":4000000000,\"request\":\"kept\"}")
                .unwrap(),
            wall(),
        )
        .unwrap()
        .unwrap();
        e[1] = Value::Text("original-process".into());
        e[2] = Value::Integer(1);
        e[3] = identity.map_or(Value::Null, |s| Value::Text(s.into()));
        e
    }
    #[test]
    fn identity_conflict_enriches_only_nullable_evidence() {
        let (root, mut s) = private_store(MAX_BYTES);
        let first = row(Some("same-identity"));
        s.write(&[first], &BTreeMap::new(), "test").unwrap();
        let before: (i64, String, String, f64) =
            s.db.query_row("SELECT id,process,request,at FROM events", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .unwrap();
        let mut next = row(Some("same-identity"));
        next[1] = Value::Text("foreign-process".into());
        next[8] = Value::Text("new-trace".into());
        next[11] = Value::Text("foreign-request".into());
        next[17] = Value::Integer(7);
        next[20] = Value::Integer(1);
        s.write(&[next], &BTreeMap::new(), "test").unwrap();
        let after: (i64, String, String, f64) =
            s.db.query_row("SELECT id,process,request,at FROM events", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .unwrap();
        assert_eq!(before, after);
        assert_eq!(
            s.db.query_row(
                "SELECT trace,input_tokens,usage_known FROM events",
                [],
                |r| Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?
                ))
            )
            .unwrap(),
            ("new-trace".into(), 7, 1)
        );
        drop(s);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn retention_and_pressure_prune_only_journal_rows_under_quota() {
        let max = 1024 * 1024;
        let (root, mut s) = private_store(max);
        let mut old = row(None);
        old[0] = Value::Real(wall() - 15. * 86400.);
        s.write(&[old], &BTreeMap::new(), "test").unwrap();
        s.maintenance = 0.;
        for _ in 0..400 {
            let mut e = row(None);
            e[21] = Value::Text("x".repeat(900));
            s.write(&vec![e; 16], &BTreeMap::new(), "test").unwrap();
        }
        let count: i64 =
            s.db.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
                .unwrap();
        assert!(count < 6400);
        let pruned: i64 =
            s.db.query_row("SELECT value FROM meta WHERE name='pruned'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(pruned > 0);
        assert_eq!(
            s.db.query_row(
                "SELECT COUNT(*) FROM events WHERE at<?",
                [wall() - 14. * 86400.],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        drop(s);
        let bytes: u64 = fs::read_dir(&root)
            .unwrap()
            .map(|p| p.unwrap().metadata().unwrap().len())
            .sum();
        assert!(bytes <= max, "{bytes}");
        fs::remove_dir_all(root).unwrap();
    }
}
