//! Opt-in MCP delivery to the local T3 environment. OAuth approval belongs to the user.
use crate::common;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

fn userdata(base: &str) -> PathBuf {
    let base = if base.is_empty() {
        common::var("T3CODE_HOME")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| crate::tell::home_dir().join(".t3"))
    } else {
        PathBuf::from(base)
    };
    base.join("userdata")
}

fn origin(base: &str) -> Option<String> {
    let value = common::read_json(&userdata(base).join("server-runtime.json"))?;
    let origin = value["origin"].as_str()?;
    let port = origin
        .strip_prefix("http://127.0.0.1:")
        .or_else(|| origin.strip_prefix("http://localhost:"))?;
    (port.parse::<u16>().ok()? > 0 && port.bytes().all(|c| c.is_ascii_digit()))
        .then(|| origin.into())
}

fn form(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

fn decode(value: &str) -> Option<String> {
    let mut bytes = Vec::new();
    let mut chars = value.bytes();
    while let Some(c) = chars.next() {
        bytes.push(match c {
            b'%' => {
                (char::from(chars.next()?).to_digit(16)? * 16
                    + char::from(chars.next()?).to_digit(16)?) as u8
            }
            b'+' => b' ',
            _ => c,
        });
    }
    String::from_utf8(bytes).ok()
}

fn random() -> Option<String> {
    let mut bytes = [0; 32];
    std::fs::File::open("/dev/urandom")
        .ok()?
        .read_exact(&mut bytes)
        .ok()?;
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn safe_header(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|c| c.is_ascii_graphic() && c != b'"' && c != b'\\')
}

struct Response {
    headers: String,
    body: String,
}

fn post(
    url: &str,
    content_type: &str,
    body: &str,
    token: Option<&str>,
    session: Option<&str>,
    version: Option<&str>,
) -> Option<Response> {
    let mut config = format!("url = \"{url}\"\nheader = \"Content-Type: {content_type}\"\n");
    config.push_str("header = \"Accept: application/json, text/event-stream\"\n");
    if let Some(token) = token {
        if !safe_header(token) {
            return None;
        }
        config.push_str(&format!("header = \"Authorization: Bearer {token}\"\n"));
    }
    if let Some(session) = session {
        if !safe_header(session) {
            return None;
        }
        config.push_str(&format!("header = \"Mcp-Session-Id: {session}\"\n"));
    }
    if let Some(version) = version {
        if !safe_header(version) {
            return None;
        }
        config.push_str(&format!("header = \"MCP-Protocol-Version: {version}\"\n"));
    }
    let body = body
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r");
    config.push_str(&format!("data-binary = \"{body}\"\n"));
    let result = common::run(
        Command::new("/usr/bin/curl").args([
            "-q",
            "--silent",
            "--fail",
            "--include",
            "--noproxy",
            "*",
            "--max-time",
            "12",
            "--config",
            "-",
        ]),
        Some(config.as_bytes()),
        14.0,
    )?;
    if result.code != 0 {
        return None;
    }
    let (headers, body) = result.stdout.split_once("\r\n\r\n")?;
    Some(Response {
        headers: headers.into(),
        body: body.into(),
    })
}

fn payload(response: &Response, id: i64) -> Option<Value> {
    let matching =
        |value: &Value| value["id"].as_i64() == Some(id) && value.get("result").is_some();
    if let Ok(value) = serde_json::from_str(&response.body) {
        return matching(&value).then_some(value);
    }
    response
        .body
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line.strip_prefix("data:")?.trim()).ok())
        .find(matching)
}

fn call(origin: &str, token: &str, request: Value, attempted: &mut bool) -> Option<Value> {
    let url = format!("{origin}/mcp");
    let rpc = |object: Value, session: Option<&str>, version: Option<&str>| {
        post(
            &url,
            "application/json",
            &object.to_string(),
            Some(token),
            session,
            version,
        )
    };
    let init = rpc(
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"HITL","version":"1"}
        }}),
        None,
        None,
    )?;
    let protocol = payload(&init, 1)?;
    let version = protocol["result"]["protocolVersion"].as_str()?;
    let session = init.headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("mcp-session-id")
            .then(|| value.trim())
    });
    rpc(
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        session,
        Some(version),
    )?;
    *attempted = true;
    let reply = rpc(request, session, Some(version))?;
    let value = payload(&reply, 2)?;
    let result = &value["result"];
    if result["isError"] == true {
        return None;
    }
    Some(result.clone())
}

fn credential(base: &str) -> PathBuf {
    userdata(base).join("hitl-mcp.json")
}

fn read_credential(path: &Path) -> Option<Value> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.permissions().mode() & 0o077 != 0
    {
        return None;
    }
    serde_json::from_reader(file.take(16_384)).ok()
}

pub fn route(base: &str) -> Option<(String, String)> {
    let endpoint = origin(base)?;
    let auth = read_credential(&credential(base))?;
    if auth["origin"] != endpoint || auth["expiresAt"].as_f64()? <= common::now() + 30.0 {
        return None;
    }
    Some((endpoint, auth["token"].as_str()?.into()))
}

pub fn send(
    route: &(String, String),
    thread: &str,
    text: &str,
    ticket: &str,
    attempted: &mut bool,
) -> Option<&'static str> {
    let reply = call(
        &route.0,
        &route.1,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
            "name":"t3_thread_send", "arguments":{"threadId":thread, "message":text, "mode":"auto", "clientRequestId":format!("hitl:{ticket}")}
        }}),
        attempted,
    )?;
    let result = &reply["structuredContent"];
    if result["threadId"] != thread {
        return None;
    }
    match result["delivery"].as_str()? {
        "steered" => Some("t3_steered"),
        "started" => Some("t3_started"),
        "queued" => Some("t3_queued"),
        _ => None,
    }
}

fn authorize(base: &str, mut announce: impl FnMut(&str)) -> Option<()> {
    let origin = origin(base)?;
    let listener = TcpListener::bind("127.0.0.1:0").ok()?;
    listener.set_nonblocking(true).ok()?;
    let redirect = format!(
        "http://127.0.0.1:{}/callback",
        listener.local_addr().ok()?.port()
    );
    let state = random()?;
    let verifier = random()?;
    let digest = Sha256::digest(verifier.as_bytes());
    let encoded = common::run(&mut Command::new("/usr/bin/base64"), Some(&digest), 2.0)?;
    if encoded.code != 0 {
        return None;
    }
    let challenge = encoded
        .stdout
        .trim()
        .trim_end_matches('=')
        .replace('+', "-")
        .replace('/', "_");
    let registration = post(&format!("{origin}/oauth/mcp/register"), "application/json",
        &json!({"client_name":"HITL", "redirect_uris":[redirect], "token_endpoint_auth_method":"none"}).to_string(), None, None, None)?;
    let registration: Value = serde_json::from_str(&registration.body).ok()?;
    let client = registration["client_id"].as_str()?;
    let url = format!(
        "{origin}/oauth/mcp/authorize?response_type=code&client_id={}&redirect_uri={}&state={}&code_challenge={}&code_challenge_method=S256&resource={}&scope={}",
        form(client),
        form(&redirect),
        state,
        challenge,
        form(&format!("{origin}/mcp")),
        form("orchestration:read orchestration:operate")
    );
    announce(&url);

    let deadline = Instant::now() + Duration::from_secs(300);
    while Instant::now() < deadline {
        let Ok((mut stream, _)) = listener.accept() else {
            std::thread::sleep(Duration::from_millis(100));
            continue;
        };
        stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .ok()?;
        let mut request = [0; 8192];
        let mut len = 0;
        while len < request.len() && !request[..len].windows(2).any(|bytes| bytes == b"\r\n") {
            match stream.read(&mut request[len..]) {
                Ok(0) | Err(_) => break,
                Ok(n) => len += n,
            }
        }
        let Some(target) = std::str::from_utf8(&request[..len])
            .ok()
            .and_then(|s| s.lines().next())
            .and_then(|line| line.strip_prefix("GET "))
            .and_then(|line| line.split_whitespace().next())
            .and_then(|target| target.strip_prefix("/callback?"))
        else {
            continue;
        };
        let param = |name| {
            target.split('&').find_map(|part| {
                let (key, value) = part.split_once('=')?;
                (key == name).then(|| decode(value)).flatten()
            })
        };
        if param("state").as_deref() != Some(state.as_str())
            || param("iss").as_deref().is_some_and(|iss| iss != origin)
        {
            continue;
        }
        let result = (|| {
            let code = param("code")?;
            let body = format!(
                "grant_type=authorization_code&client_id={}&redirect_uri={}&code={}&code_verifier={}&resource={}",
                form(client),
                form(&redirect),
                form(&code),
                form(&verifier),
                form(&format!("{origin}/mcp"))
            );
            let reply = post(
                &format!("{origin}/oauth/mcp/token"),
                "application/x-www-form-urlencoded",
                &body,
                None,
                None,
                None,
            )?;
            let reply: Value = serde_json::from_str(&reply.body).ok()?;
            let token = reply["access_token"].as_str()?;
            let expires = reply["expires_in"].as_f64()?;
            if expires <= 30.0
                || !reply["scope"]
                    .as_str()?
                    .split_whitespace()
                    .any(|scope| scope == "orchestration:operate")
            {
                return None;
            }
            let list = call(
                &origin,
                token,
                json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
                &mut false,
            )?;
            if !list["tools"]
                .as_array()?
                .iter()
                .any(|tool| tool["name"] == "t3_thread_send")
            {
                return None;
            }
            let path = credential(base);
            let temp = path.with_extension(format!("{state}.tmp"));
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temp)
                .ok()?;
            let saved = (|| {
                serde_json::to_writer(
                    &mut file,
                    &json!({"origin":origin,"token":token,"expiresAt":common::now()+expires}),
                )
                .ok()?;
                file.sync_all().ok()?;
                std::fs::rename(&temp, path).ok()
            })();
            if saved.is_none() {
                let _ = std::fs::remove_file(temp);
            }
            saved
        })();
        let body = if result.is_some() {
            "HITL connected. Close this tab."
        } else {
            "HITL authorization failed."
        };
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        return result;
    }
    None
}

pub fn connect(args: &[String]) -> i32 {
    let base = match args {
        [] => "",
        [flag, base] if flag == "--base-dir" => base,
        _ => {
            eprintln!("Usage: hitl t3-connect [--base-dir PATH]");
            return 64;
        }
    };
    if authorize(base, |url| println!("{url}")).is_some() {
        eprintln!("HITL: подключение T3 сохранено.");
        0
    } else {
        eprintln!("HITL: подключение T3 не подтверждено.");
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};

    #[test]
    fn calls_auto_and_distinguishes_a_lost_receipt_from_a_failed_handshake() {
        for failure in ["", "initialize", "send"] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = format!("http://{}", listener.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                for step in ["initialize", "notifications/initialized", "tools/call"] {
                    let (mut stream, _) = listener.accept().unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut reader = BufReader::new(&mut stream);
                    let mut line = String::new();
                    let mut length = 0;
                    let mut headers = String::new();
                    loop {
                        line.clear();
                        assert!(reader.read_line(&mut line).unwrap() > 0);
                        if line == "\r\n" {
                            break;
                        }
                        let lower = line.trim().to_ascii_lowercase();
                        if let Some(value) = lower.strip_prefix("content-length: ") {
                            length = value.parse().unwrap();
                        }
                        headers.push_str(&lower);
                        headers.push('\n');
                    }
                    assert!(headers.contains("authorization: bearer example-token"));
                    if step != "initialize" {
                        assert!(headers.contains("mcp-protocol-version: 2025-03-26"));
                        assert!(headers.contains("mcp-session-id: example-session"));
                    }
                    let mut bytes = vec![0; length];
                    reader.read_exact(&mut bytes).unwrap();
                    let request: Value = serde_json::from_slice(&bytes).unwrap();
                    assert_eq!(request["method"], step);
                    let (status, extra, body) = match step {
                        "initialize" if failure == "initialize" => {
                            ("401 Unauthorized", "", String::new())
                        }
                        "initialize" => (
                            "200 OK",
                            "Mcp-Session-Id: example-session\r\n",
                            json!({
                                "jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-03-26"}
                            })
                            .to_string(),
                        ),
                        "notifications/initialized" => ("202 Accepted", "", String::new()),
                        _ => {
                            assert_eq!(request["params"]["name"], "t3_thread_send");
                            assert_eq!(
                                request["params"]["arguments"],
                                json!({
                                    "threadId":"thread","message":"ответ \"в\"\nчат", "mode":"auto", "clientRequestId":"hitl:ticket"
                                })
                            );
                            if failure == "send" {
                                break;
                            }
                            (
                                "200 OK",
                                "Content-Type: text/event-stream\r\n",
                                format!(
                                    "data: {}\n\n",
                                    json!({
                                        "jsonrpc":"2.0","id":2,"result":{"structuredContent":{"threadId":"thread","delivery":"steered"}}
                                    })
                                ),
                            )
                        }
                    };
                    write!(stream, "HTTP/1.1 {status}\r\n{extra}Connection: close\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
                    if failure == "initialize" {
                        break;
                    }
                }
            });
            let mut attempted = false;
            let result = send(
                &(address, "example-token".into()),
                "thread",
                "ответ \"в\"\nчат",
                "ticket",
                &mut attempted,
            );
            assert_eq!(
                result,
                if failure.is_empty() {
                    Some("t3_steered")
                } else {
                    None
                }
            );
            assert_eq!(attempted, failure != "initialize");
            server.join().unwrap();
        }
    }
}
