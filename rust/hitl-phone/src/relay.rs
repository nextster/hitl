//! The way between the Mac and the phone when they are not in one network: a server of his own in between.
//!
//! «короче надо срочно делать синхронизацию конвейера с мобилкой», then, asked what that is to do, «Телефон
//! работает везде, не только дома», through «Свой сервер-посредник на Cloudflare» (2026-10-11). At home the phone
//! finds the Mac by itself; anywhere else, and with a VPN on, neither can reach the other, but both can reach a
//! server. The server (`relay/worker.js`, in his own account) is a room with two doors: what the Mac says there
//! is handed to the phone and what the phone says to the Mac, and it tells each whether the other is in.
//!
//! What is said in the room is sealed here and on the phone (`demo/ConveyorDemo/Queue/RelaySeal.swift`), with a key
//! that lies on the Mac and is put on the phone by cable, or told to it at home: the server is handed frames it
//! cannot read or make. This file is the Mac's half of that: the seal, and a WebSocket as a client speaks it.
//!
//!   a frame   12 bytes of a nonce, the sealed words, 32 bytes of their tag
//!   sealed    the launch that says it (8 bytes), its number in that launch, the moment, whether more of the same
//!             line follows, then the line or a part of it; mixed with a stream of HMAC-SHA256 over the nonce
//!             and a counter
//!   the tag   HMAC-SHA256, under a second key, of who speaks, the nonce and the sealed words

use crate::codec;
use crate::tls;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// How much of a line goes in one frame: a picture of several megabytes is said in parts.
pub const PART_BYTES: usize = 96 * 1024;
/// A frame older than this, by the clock of who hears it, is not taken: what was said once is not said again by
/// somebody who kept it.
pub const FRESH_SECONDS: f64 = 600.0;
/// What the phone said while the Mac was away, held for it in the room, is taken for a day by a seal that keeps what
/// it has heard on the disk: that note, not the age, is what keeps a frame from being taken twice (his pick of
/// 2026-10-16 that a press without a way to the Mac is kept, «на сервере, если он доступен»).
pub const HELD_SECONDS: f64 = 24.0 * 3600.0;
pub const MAC: &[u8] = b"M";
pub const PHONE: &[u8] = b"P";
pub const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// Bytes mixed with a stream of the same length or longer.
pub fn mixed(data: &[u8], stream: &[u8]) -> Vec<u8> {
    data.iter().zip(stream).map(|(byte, with)| byte ^ with).collect()
}

fn now() -> f64 {
    hitl_host_core::now()
}

/// Seals what one side says and opens what the other said. `mine` and `theirs` are who speaks: a frame is opened only
/// as the other side's, so what this side said is never taken back as said to it.
pub struct Seal {
    cipher: [u8; 32],
    tag: [u8; 32],
    mine: Vec<u8>,
    theirs: Vec<u8>,
    launch: Vec<u8>,
    count: u64,
    /// The last number heard of each launch of the other side, and when. With `kept` it outlives this seal: a frame
    /// heard once is not taken again by a seal made after a restart or a new way into the room, as it was for a frame
    /// kept by somebody and said again within its ten minutes (the audit of 2026-10-16, A12).
    kept: Option<PathBuf>,
    heard: HashMap<Vec<u8>, (u64, f64)>,
    parts: Vec<Vec<u8>>,
    speaker: Vec<u8>,
}

impl Seal {
    pub fn new(key: &[u8], mine: &[u8], theirs: &[u8], kept: Option<&Path>) -> Result<Seal, String> {
        if key.len() < 32 {
            return Err("the key of the server's room is shorter than 32 bytes".into());
        }
        Ok(Seal {
            cipher: codec::hmac(key, b"hitl relay: cipher"),
            tag: codec::hmac(key, b"hitl relay: tag"),
            mine: mine.to_vec(),
            theirs: theirs.to_vec(),
            launch: codec::random(8),
            count: 0,
            kept: kept.map(Path::to_path_buf),
            heard: HashMap::new(),
            parts: Vec::new(),
            speaker: Vec::new(),
        })
    }

    /// How old a frame may be to be taken: a day where what was heard is kept on the disk, ten minutes elsewhere.
    fn window(&self) -> f64 {
        if self.kept.is_some() { HELD_SECONDS } else { FRESH_SECONDS }
    }

    /// Takes in what any seal with the same note has heard, whenever it was made.
    fn recall(&mut self) {
        let Some(path) = &self.kept else { return };
        let Some(Value::Object(found)) = hitl_host_core::read_json(path) else { return };
        let window = self.window();
        for (launch, entry) in found {
            let (Some(launch), Some(count), Some(at)) = (codec::unhex(&launch), entry.get(0).and_then(Value::as_f64),
                                                         entry.get(1).and_then(Value::as_f64)) else { continue };
            let count = count as u64;
            if now() - at < 2.0 * window && count > self.heard.get(&launch).map_or(0, |known| known.0) {
                self.heard.insert(launch, (count, at));
            }
        }
    }

    /// Writes down what has been heard, whole or not at all; false when the disk did not take it.
    fn keep(&self) -> bool {
        let Some(path) = &self.kept else { return false };
        let mut map = Map::new();
        for (launch, (count, at)) in &self.heard {
            map.insert(codec::hex(launch), json!([count, at]));
        }
        let write = || -> std::io::Result<()> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let name = path.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default();
            let spare = path.with_file_name(format!(".{name}.{}", std::process::id()));
            std::fs::write(&spare, Value::Object(map).to_string())?;
            std::fs::rename(&spare, path)
        };
        write().is_ok()
    }

    fn stream(&self, nonce: &[u8], size: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(size.div_ceil(32) * 32);
        for block in 0..size.div_ceil(32) as u32 {
            let mut input = nonce.to_vec();
            input.extend_from_slice(&block.to_be_bytes());
            out.extend_from_slice(&codec::hmac(&self.cipher, &input));
        }
        out
    }

    /// A line as the frames to say it in, in their order. `moment` is when it is said, by this Mac's clock.
    pub fn frames_at(&mut self, line: &[u8], moment: f64) -> Vec<Vec<u8>> {
        let pieces: Vec<&[u8]> = if line.is_empty() { vec![line] } else { line.chunks(PART_BYTES).collect() };
        let mut frames = Vec::with_capacity(pieces.len());
        for (number, piece) in pieces.iter().enumerate() {
            self.count += 1;
            let more = number < pieces.len() - 1;
            let mut plain = self.launch.clone();
            plain.extend_from_slice(&self.count.to_be_bytes());
            plain.extend_from_slice(&((moment * 1000.0) as u64).to_be_bytes());
            plain.push(u8::from(more));
            plain.extend_from_slice(piece);
            let nonce = codec::random(12);
            let sealed = mixed(&plain, &self.stream(&nonce, plain.len()));
            let mut said = self.mine.clone();
            said.extend_from_slice(&nonce);
            said.extend_from_slice(&sealed);
            let mut frame = nonce;
            frame.extend_from_slice(&sealed);
            frame.extend_from_slice(&codec::hmac(&self.tag, &said));
            frames.push(frame);
        }
        frames
    }

    pub fn frames(&mut self, line: &[u8]) -> Vec<Vec<u8>> {
        self.frames_at(line, now())
    }

    /// The line a frame ends; None for a frame that is a part of one, was not sealed by the other side with this key,
    /// is old, or was heard before.
    pub fn line(&mut self, frame: &[u8]) -> Option<Vec<u8>> {
        if frame.len() < 12 + 25 + 32 {
            return None;
        }
        let (nonce, rest) = frame.split_at(12);
        let (sealed, tag) = rest.split_at(rest.len() - 32);
        let mut said = self.theirs.clone();
        said.extend_from_slice(nonce);
        said.extend_from_slice(sealed);
        if !codec::same(tag, &codec::hmac(&self.tag, &said)) {
            return None;
        }
        let plain = mixed(sealed, &self.stream(nonce, sealed.len()));
        let launch = plain[..8].to_vec();
        let count = u64::from_be_bytes(plain[8..16].try_into().ok()?);
        let moment = u64::from_be_bytes(plain[16..24].try_into().ok()?) as f64 / 1000.0;
        let more = plain[24] != 0;
        if (now() - moment).abs() > self.window() {
            return None;
        }
        if self.kept.is_some() {
            self.recall();
        }
        if count <= self.heard.get(&launch).map_or(0, |known| known.0) {
            return None;
        }
        self.heard.insert(launch.clone(), (count, now()));
        if self.heard.len() > 8 {
            let window = self.window();
            self.heard.retain(|_, kept| now() - kept.1 < 2.0 * window);
        }
        if self.kept.is_some() && !self.keep() {
            return None; // what cannot be remembered as heard is not acted on
        }
        if launch != self.speaker {
            // Another launch of the other side: what the earlier one left half said is not a part of this.
            self.speaker = launch;
            self.parts.clear();
        }
        self.parts.push(plain[25..].to_vec());
        if more {
            return None;
        }
        Some(std::mem::take(&mut self.parts).concat())
    }
}

/// What a server's room answered with other than a room, or how the way to it broke.
#[derive(Debug)]
pub enum WireError {
    /// The server answered, and not with a room: the first line of what it said.
    Refused(String),
    Broken(String),
}

impl std::fmt::Display for WireError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WireError::Refused(line) | WireError::Broken(line) => formatter.write_str(line),
        }
    }
}

impl From<std::io::Error> for WireError {
    fn from(error: std::io::Error) -> WireError {
        WireError::Broken(error.to_string())
    }
}

/// One whole message of the room.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    Text(Vec<u8>),
    Binary(Vec<u8>),
    Closed(Vec<u8>),
}

/// An address of a server's room: `ws://host:port/path` or `wss://…`.
struct Address {
    secure: bool,
    host: String,
    port: u16,
    netloc: String,
    path: String,
}

fn address(url: &str) -> Option<Address> {
    let (scheme, rest) = url.split_once("://")?;
    let secure = match scheme.to_ascii_lowercase().as_str() {
        "ws" => false,
        "wss" => true,
        _ => return None,
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let netloc = &rest[..end];
    let path_end = rest[end..].find(['?', '#']).map_or(rest.len(), |at| end + at);
    let path = if rest[end..path_end].is_empty() { "/".to_string() } else { rest[end..path_end].to_string() };
    let host_port = netloc.rsplit('@').next().unwrap_or(netloc);
    let (host, port) = if let Some(inner) = host_port.strip_prefix('[') {
        let (host, after) = inner.split_once(']')?;
        (host.to_string(), after.strip_prefix(':').map(str::to_string))
    } else {
        match host_port.rsplit_once(':') {
            Some((host, port)) => (host.to_string(), Some(port.to_string())),
            None => (host_port.to_string(), None),
        }
    };
    if host.is_empty() {
        return None;
    }
    let port = match port {
        Some(port) => port.parse().ok()?,
        None => if secure { 443 } else { 80 },
    };
    Some(Address { secure, host: host.to_ascii_lowercase(), port, netloc: netloc.to_string(), path })
}

/// The way to the room: plain TCP or TLS over it.
enum Link {
    Plain(TcpStream),
    Secure(Box<rustls::ClientConnection>, TcpStream),
}

impl Link {
    fn write_all(&mut self, data: &[u8]) -> std::io::Result<()> {
        match self {
            Link::Plain(socket) => socket.write_all(data),
            Link::Secure(connection, socket) => {
                // rustls keeps a limited buffer of what waits to be sealed: a long frame goes in as it is drained.
                let mut rest = data;
                while !rest.is_empty() {
                    let taken = connection.writer().write(rest)?;
                    rest = &rest[taken..];
                    if taken == 0 && !connection.wants_write() {
                        return Err(std::io::Error::new(ErrorKind::WriteZero, "the TLS connection took nothing"));
                    }
                    while connection.wants_write() {
                        connection.write_tls(socket)?;
                    }
                }
                Ok(())
            }
        }
    }

    /// What the TLS connection already holds, read out without waiting: Ok(None) when it holds nothing, Ok(Some(empty))
    /// at the end. A plain way holds nothing of its own.
    fn held(&mut self) -> std::io::Result<Option<Vec<u8>>> {
        let Link::Secure(connection, _) = self else { return Ok(None) };
        let mut buffer = vec![0u8; 1 << 16];
        match connection.reader().read(&mut buffer) {
            Ok(count) => Ok(Some(buffer[..count].to_vec())),
            Err(error) if error.kind() == ErrorKind::WouldBlock => Ok(None),
            Err(error) if error.kind() == ErrorKind::UnexpectedEof => Ok(Some(Vec::new())),
            Err(error) => Err(error),
        }
    }

    /// What has come on a socket that poll says can be read: Ok(None) when it was not yet a whole piece of TLS,
    /// Ok(Some(empty)) at the end.
    fn read_ready(&mut self) -> std::io::Result<Option<Vec<u8>>> {
        match self {
            Link::Plain(socket) => {
                let mut buffer = vec![0u8; 1 << 16];
                match socket.read(&mut buffer) {
                    Ok(count) => Ok(Some(buffer[..count].to_vec())),
                    Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => Ok(None),
                    Err(error) => Err(error),
                }
            }
            Link::Secure(connection, socket) => {
                while connection.wants_write() {
                    connection.write_tls(socket)?;
                }
                match connection.read_tls(socket) {
                    Ok(0) => return Ok(Some(Vec::new())),
                    Ok(_) => {
                        if let Err(error) = connection.process_new_packets() {
                            while connection.wants_write() {
                                let _ = connection.write_tls(socket);
                            }
                            return Err(std::io::Error::new(ErrorKind::InvalidData, error));
                        }
                    }
                    Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => return Ok(None),
                    Err(error) => return Err(error),
                }
                self.held()
            }
        }
    }
}

/// Whether a socket can be read from within `wait`: there is something, or its end.
fn readable(socket: &TcpStream, wait: Duration) -> bool {
    let mut asked = libc::pollfd { fd: socket.as_raw_fd(), events: libc::POLLIN, revents: 0 };
    let milliseconds = i32::try_from(wait.as_millis()).unwrap_or(i32::MAX).max(i32::from(!wait.is_zero()));
    unsafe { libc::poll(&raw mut asked, 1, milliseconds) > 0 }
}

struct WireState {
    link: Link,
    heard: Vec<u8>,
    kind: u8,
    pieces: Vec<Vec<u8>>,
}

/// A WebSocket as a client speaks it, and no more of it than the room needs: whole messages each way, a ping
/// answered, an end noticed. One thread reads; any may send or close.
pub struct Wire {
    state: Mutex<WireState>,
    socket: TcpStream,
    over: AtomicBool,
}

fn connect(host: &str, port: u16, timeout: Duration) -> std::io::Result<TcpStream> {
    let mut last = std::io::Error::new(ErrorKind::NotFound, format!("no address for {host}"));
    for place in (host, port).to_socket_addrs()? {
        match TcpStream::connect_timeout(&place, timeout) {
            Ok(socket) => return Ok(socket),
            Err(error) => last = error,
        }
    }
    Err(last)
}

impl Wire {
    pub fn open(url: &str, headers: &[(&str, &str)]) -> Result<Wire, WireError> {
        Wire::open_within(url, headers, Duration::from_secs(10))
    }

    pub fn open_within(url: &str, headers: &[(&str, &str)], timeout: Duration) -> Result<Wire, WireError> {
        let Some(place) = address(url) else {
            return Err(WireError::Broken(format!("not an address of a server's room: {url}")));
        };
        let socket = connect(&place.host, place.port, timeout)?;
        socket.set_read_timeout(Some(timeout))?;
        socket.set_write_timeout(Some(timeout))?;
        let _ = socket.set_nodelay(true);
        let mut link = if place.secure {
            let mut connection = tls::client(&place.host).map_err(WireError::Broken)?;
            let mut plain = socket.try_clone()?;
            while connection.is_handshaking() {
                connection.complete_io(&mut plain)?;
            }
            Link::Secure(Box::new(connection), plain)
        } else {
            Link::Plain(socket.try_clone()?)
        };
        let key = codec::base64(&codec::random(16));
        let mut asked = vec![format!("GET {} HTTP/1.1", place.path), format!("Host: {}", place.netloc), "Upgrade: websocket".into(),
                             "Connection: Upgrade".into(), format!("Sec-WebSocket-Key: {key}"), "Sec-WebSocket-Version: 13".into()];
        asked.extend(headers.iter().map(|(name, value)| format!("{name}: {value}")));
        link.write_all((asked.join("\r\n") + "\r\n\r\n").as_bytes())?;
        let mut head = Vec::new();
        let until = Instant::now() + timeout;
        let split = loop {
            if let Some(at) = head.windows(4).position(|window| window == b"\r\n\r\n") {
                break at;
            }
            let chunk = match link.held()? {
                Some(chunk) => Some(chunk),
                None if readable(&socket, until.saturating_duration_since(Instant::now())) => link.read_ready()?,
                None => return Err(WireError::Broken("timed out".into())),
            };
            match chunk {
                Some(chunk) if !chunk.is_empty() && head.len() <= 65536 => head.extend_from_slice(&chunk),
                Some(_) => return Err(WireError::Broken("the server closed before it answered".into())),
                None => {}
            }
        };
        let rest = head[split + 4..].to_vec();
        let text: String = head[..split].iter().map(|&byte| byte as char).collect(); // latin-1
        let lines: Vec<&str> = text.split("\r\n").collect();
        let words: Vec<&str> = lines[0].split_whitespace().collect();
        if words.len() < 2 || words[1] != "101" {
            return Err(WireError::Refused(lines[0].to_string()));
        }
        let said: HashMap<String, String> = lines[1..].iter().map(|line| {
            let (name, value) = line.split_once(':').unwrap_or((line, ""));
            (name.trim().to_lowercase(), value.trim().to_string())
        }).collect();
        let accept = codec::base64(&codec::sha1(format!("{key}{GUID}").as_bytes()));
        if said.get("sec-websocket-accept") != Some(&accept) {
            return Err(WireError::Broken("the server did not answer as a WebSocket does".into()));
        }
        socket.set_read_timeout(Some(Duration::from_secs(30)))?;
        socket.set_write_timeout(Some(Duration::from_secs(30)))?;
        Ok(Wire { state: Mutex::new(WireState { link, heard: rest, kind: 2, pieces: Vec::new() }), socket, over: AtomicBool::new(false) })
    }

    pub fn is_over(&self) -> bool {
        self.over.load(Ordering::SeqCst)
    }

    fn send_locked(state: &mut WireState, payload: &[u8], opcode: u8) -> std::io::Result<()> {
        let mask = codec::random(4);
        let size = payload.len();
        let mut frame = vec![0x80 | opcode];
        if size < 126 {
            frame.push(0x80 | size as u8);
        } else if size < 65536 {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(size as u16).to_be_bytes());
        } else {
            frame.push(0x80 | 127);
            frame.extend_from_slice(&(size as u64).to_be_bytes());
        }
        frame.extend_from_slice(&mask);
        frame.extend(payload.iter().enumerate().map(|(at, byte)| byte ^ mask[at % 4]));
        state.link.write_all(&frame)
    }

    pub fn send(&self, payload: &[u8]) -> std::io::Result<()> {
        self.send_as(payload, 2)
    }

    pub fn send_as(&self, payload: &[u8], opcode: u8) -> std::io::Result<()> {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        Wire::send_locked(&mut state, payload, opcode)
    }

    pub fn say(&self, text: &str) -> std::io::Result<()> {
        self.send_as(text.as_bytes(), 1)
    }

    /// One whole message out of what has come, if one is there.
    fn parsed(&self, state: &mut WireState) -> std::io::Result<Option<Message>> {
        loop {
            let heard = &state.heard;
            if heard.len() < 2 {
                return Ok(None);
            }
            let (mut size, mut at) = (u64::from(heard[1] & 0x7F), 2usize);
            if size == 126 {
                if heard.len() < 4 {
                    return Ok(None);
                }
                size = u64::from(u16::from_be_bytes([heard[2], heard[3]]));
                at = 4;
            } else if size == 127 {
                if heard.len() < 10 {
                    return Ok(None);
                }
                size = u64::from_be_bytes(heard[2..10].try_into().expect("eight bytes"));
                at = 10;
            }
            let mut mask = None;
            if heard[1] & 0x80 != 0 {
                if heard.len() < at + 4 {
                    return Ok(None);
                }
                mask = Some([heard[at], heard[at + 1], heard[at + 2], heard[at + 3]]);
                at += 4;
            }
            let size = usize::try_from(size).unwrap_or(usize::MAX);
            if heard.len().saturating_sub(at) < size {
                return Ok(None);
            }
            let (first, end) = (heard[0], at + size);
            let mut payload = heard[at..end].to_vec();
            state.heard.drain(..end);
            if let Some(mask) = mask {
                payload.iter_mut().enumerate().for_each(|(at, byte)| *byte ^= mask[at % 4]);
            }
            let (opcode, last) = (first & 0x0F, first & 0x80 != 0);
            match opcode {
                0x9 => {
                    Wire::send_locked(state, &payload, 0xA)?;
                    continue;
                }
                0xA => continue,
                0x8 => {
                    self.over.store(true, Ordering::SeqCst);
                    return Ok(Some(Message::Closed(payload)));
                }
                1 | 2 => {
                    state.kind = opcode;
                    state.pieces = vec![payload];
                }
                _ => state.pieces.push(payload),
            }
            if !last {
                continue;
            }
            let whole = std::mem::take(&mut state.pieces).concat();
            return Ok(Some(if state.kind == 1 { Message::Text(whole) } else { Message::Binary(whole) }));
        }
    }

    /// The next whole message, or None when none came in `wait` seconds. The way is waited on without holding it, so
    /// that another thread can say something meanwhile: holding it, this thread took it back at once every time, and
    /// the Mac's «resting» waited behind it for good.
    pub fn receive(&self, wait: f64) -> std::io::Result<Option<Message>> {
        let until = Instant::now() + Duration::from_secs_f64(wait.max(0.0));
        loop {
            {
                let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                loop {
                    if let Some(message) = self.parsed(&mut state)? {
                        return Ok(Some(message));
                    }
                    match state.link.held()? {
                        Some(chunk) if chunk.is_empty() => return Ok(Some(self.ended())),
                        Some(chunk) => state.heard.extend_from_slice(&chunk),
                        None => break,
                    }
                }
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() || !readable(&self.socket, left) {
                return Ok(None);
            }
            let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            match state.link.read_ready()? {
                None => {}
                Some(chunk) if chunk.is_empty() => return Ok(Some(self.ended())),
                Some(chunk) => state.heard.extend_from_slice(&chunk),
            }
        }
    }

    fn ended(&self) -> Message {
        self.over.store(true, Ordering::SeqCst);
        Message::Closed(Vec::new())
    }

    pub fn close(&self) {
        self.over.store(true, Ordering::SeqCst);
        let _ = self.socket.shutdown(Shutdown::Both);
    }
}

impl Drop for Wire {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(key: &[u8]) -> (Seal, Seal) {
        (Seal::new(key, MAC, PHONE, None).unwrap(), Seal::new(key, PHONE, MAC, None).unwrap())
    }

    fn key() -> Vec<u8> {
        (0u8..32).collect()
    }

    #[test]
    fn a_line_comes_out_as_it_went_in_whatever_its_length() {
        let (mut mac, mut far) = pair(&key());
        let lines = [b"".to_vec(), b"{}".to_vec(), "я".repeat(5).into_bytes(), codec::random(PART_BYTES),
                     codec::random(PART_BYTES + 1), codec::random(700_001)];
        for line in lines {
            let frames = mac.frames(&line);
            assert_eq!(frames.len(), 1.max(line.len().div_ceil(PART_BYTES)));
            let heard: Vec<Option<Vec<u8>>> = frames.iter().map(|frame| far.line(frame)).collect();
            assert_eq!(heard.last().unwrap().as_deref(), Some(&line[..]));
            assert!(heard[..heard.len() - 1].iter().all(Option::is_none));
            let opening = if line.len() >= 24 { line[..24].to_vec() } else { vec![0u8; 24] };
            assert!(!frames.concat().windows(24).any(|window| window == opening));
        }
    }

    #[test]
    fn a_frame_is_opened_only_by_the_other_side_with_the_same_key() {
        let (mut mac, mut far) = pair(&key());
        let frame = mac.frames(b"{}").remove(0);
        assert_eq!(mac.line(&frame), None); // what the Mac said is not taken as said to the Mac
        assert_eq!(Seal::new(&[0u8; 32], PHONE, MAC, None).unwrap().line(&frame), None);
        for at in [0, 13, frame.len() - 1] {
            let mut bent = frame.clone();
            bent[at] ^= 1;
            assert_eq!(far.line(&bent), None);
        }
        assert_eq!(far.line(&frame[..40]), None);
        assert_eq!(far.line(&frame).as_deref(), Some(&b"{}"[..]));
    }

    #[test]
    fn a_frame_is_heard_once_and_not_when_it_is_old() {
        let (mut mac, mut far) = pair(&key());
        let first = mac.frames(b"1").remove(0);
        let second = mac.frames(b"2").remove(0);
        assert_eq!(far.line(&second).as_deref(), Some(&b"2"[..]));
        assert_eq!(far.line(&second), None);
        assert_eq!(far.line(&first), None); // an earlier one, kept and said late
        let old = mac.frames_at(b"3", now() - FRESH_SECONDS - 5.0).remove(0);
        assert_eq!(far.line(&old), None);
        assert_eq!(far.line(&mac.frames(b"4").remove(0)).as_deref(), Some(&b"4"[..]));
    }

    #[test]
    fn a_frame_heard_before_a_restart_is_not_taken_by_the_next_seal_of_the_mac() {
        // The audit of 2026-10-16, A12: a new seal with the same key took a frame the earlier one had heard.
        let folder = std::env::temp_dir().join(format!("relay-heard.{}.{}", std::process::id(), codec::token_hex(4)));
        let kept = folder.join("phone").join("relay-heard.json");
        let mut phone_side = Seal::new(&key(), PHONE, MAC, None).unwrap();
        let press = phone_side.frames(br#"{"press":1}"#).remove(0);
        let later = phone_side.frames(br#"{"press":2}"#).remove(0);
        let mut mac = Seal::new(&key(), MAC, PHONE, Some(&kept)).unwrap();
        assert_eq!(mac.line(&press).as_deref(), Some(&br#"{"press":1}"#[..]));
        for _ in 0..2 {
            // the process started anew, or came into the room again
            assert_eq!(Seal::new(&key(), MAC, PHONE, Some(&kept)).unwrap().line(&press), None);
        }
        assert_eq!(Seal::new(&key(), MAC, PHONE, Some(&kept)).unwrap().line(&later).as_deref(), Some(&br#"{"press":2}"#[..]));
        // A disk that does not take the note: the frame is not acted on, and the next seal can still hear it once.
        let mut broken = Seal::new(&key(), MAC, PHONE, Some(&folder.join("file").join("relay-heard.json"))).unwrap();
        std::fs::write(folder.join("file"), "not a folder").unwrap();
        assert_eq!(broken.line(&phone_side.frames(br#"{"press":3}"#).remove(0)), None);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_launch_that_left_a_line_half_said_does_not_spoil_the_next() {
        let (mut mac, mut far) = pair(&key());
        assert_eq!(far.line(&mac.frames(&codec::random(PART_BYTES * 2)).remove(0)), None);
        let mut again = Seal::new(&key(), MAC, PHONE, None).unwrap(); // the Mac's process started anew
        assert_eq!(far.line(&again.frames(b"{}").remove(0)).as_deref(), Some(&b"{}"[..]));
    }

    #[test]
    fn a_short_key_is_refused() {
        assert!(Seal::new(&[0u8; 31], MAC, PHONE, None).is_err());
    }

    #[test]
    fn an_address_of_a_room_is_read_as_python_splits_it() {
        let place = address("ws://127.0.0.1:8123/room/5f/mac").unwrap();
        assert_eq!((place.secure, place.host.as_str(), place.port, place.netloc.as_str(), place.path.as_str()),
                   (false, "127.0.0.1", 8123, "127.0.0.1:8123", "/room/5f/mac"));
        let place = address("wss://Conveyor-Relay.example.workers.dev/room/x/mac?a=1").unwrap();
        assert_eq!((place.secure, place.host.as_str(), place.port, place.path.as_str()),
                   (true, "conveyor-relay.example.workers.dev", 443, "/room/x/mac"));
        assert_eq!(address("ws://[::1]:9/").map(|place| (place.host, place.port)), Some(("::1".into(), 9)));
        assert!(address("https://example.test/").is_none() && address("ws:///x").is_none());
    }
}
