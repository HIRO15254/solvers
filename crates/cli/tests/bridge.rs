use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

const ORIGIN: &str = "http://localhost:3000";
const IO_TIMEOUT: Duration = Duration::from_secs(5);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);

struct ChildGuard {
    child: Child,
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

struct Bridge {
    _process: ChildGuard,
    addr: String,
    token: String,
}

impl Bridge {
    fn spawn() -> Self {
        let child = Command::new(env!("CARGO_BIN_EXE_solvers"))
            .args(["serve", "--port", "0", "--origin", ORIGIN])
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn solvers serve");
        let mut process = ChildGuard { child };

        // Read on a helper thread so a broken startup cannot hang the test
        // indefinitely. The bridge's stdout contract contains only this line.
        let stdout = process.child.stdout.take().expect("capture bridge stdout");
        let (sender, receiver) = mpsc::sync_channel(1);
        let reader = thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            let result = match reader.read_line(&mut line) {
                Ok(0) => Err("bridge exited before printing its startup line".to_owned()),
                Ok(_) => Ok(line),
                Err(error) => Err(format!("read bridge startup line: {error}")),
            };
            let _ = sender.send(result);
        });

        let line = receiver
            .recv_timeout(STARTUP_TIMEOUT)
            .unwrap_or_else(|error| panic!("wait for bridge startup line: {error}"))
            .expect("bridge startup failed");
        reader.join().expect("bridge stdout reader panicked");

        let fields: Vec<_> = line.trim_end().split_ascii_whitespace().collect();
        assert_eq!(fields.len(), 4, "unexpected bridge startup line: {line:?}");
        assert_eq!(fields[0], "bridge:");

        let url = fields[1]
            .strip_prefix("url=")
            .unwrap_or_else(|| panic!("startup line has no url field: {line:?}"));
        let port = url
            .strip_prefix("http://127.0.0.1:")
            .and_then(|port| port.parse::<u16>().ok())
            .filter(|port| *port != 0)
            .unwrap_or_else(|| panic!("invalid loopback URL in startup line: {line:?}"));
        let addr = format!("127.0.0.1:{port}");
        let token = fields[2]
            .strip_prefix("token=")
            .unwrap_or_else(|| panic!("startup line has no token field: {line:?}"))
            .to_owned();
        assert!(
            !token.is_empty() && token.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "bridge token is not hexadecimal: {token:?}"
        );
        let origin = fields[3]
            .strip_prefix("origin=")
            .unwrap_or_else(|| panic!("startup line has no origin field: {line:?}"));
        assert_eq!(origin, ORIGIN);

        Self {
            _process: process,
            addr,
            token,
        }
    }

    fn request(
        &self,
        method: &str,
        path: &str,
        host: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> HttpResponse {
        let mut stream = TcpStream::connect(&self.addr).expect("connect to bridge");
        stream
            .set_read_timeout(Some(IO_TIMEOUT))
            .expect("set bridge read timeout");
        stream
            .set_write_timeout(Some(IO_TIMEOUT))
            .expect("set bridge write timeout");

        let mut request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Length: {}\r\n",
            body.len()
        );
        for (name, value) in headers {
            request.push_str(name);
            request.push_str(": ");
            request.push_str(value);
            request.push_str("\r\n");
        }
        request.push_str("\r\n");

        stream
            .write_all(request.as_bytes())
            .and_then(|()| stream.write_all(body))
            .expect("write bridge request");

        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).expect("read bridge response");
        HttpResponse::parse(&raw)
    }

    fn authenticated_headers(&self) -> [(&'static str, String); 2] {
        [
            ("Origin", ORIGIN.to_owned()),
            ("Authorization", format!("Bearer {}", self.token)),
        ]
    }
}

struct HttpResponse {
    status: u16,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

impl HttpResponse {
    fn parse(raw: &[u8]) -> Self {
        let header_end = raw
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap_or_else(|| panic!("HTTP response has no header terminator: {raw:?}"));
        let head = std::str::from_utf8(&raw[..header_end]).expect("HTTP headers are UTF-8");
        let mut lines = head.split("\r\n");
        let status_line = lines.next().expect("HTTP response has a status line");
        let status = status_line
            .split_ascii_whitespace()
            .nth(1)
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or_else(|| panic!("invalid HTTP status line: {status_line:?}"));
        let headers = lines
            .map(|line| {
                let (name, value) = line
                    .split_once(':')
                    .unwrap_or_else(|| panic!("invalid HTTP header: {line:?}"));
                (name.to_ascii_lowercase(), value.trim().to_owned())
            })
            .collect();

        Self {
            status,
            headers,
            body: raw[header_end + 4..].to_vec(),
        }
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|error| {
            panic!(
                "response body is not JSON ({error}): {:?}",
                String::from_utf8_lossy(&self.body)
            )
        })
    }
}

fn string_headers<'a>(headers: &'a [(&'static str, String)]) -> Vec<(&'static str, &'a str)> {
    headers
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect()
}

fn assert_error(response: &HttpResponse, status: u16, code: &str) {
    assert_eq!(response.status, status, "body: {:?}", response.json());
    let body = response.json();
    let root = body.as_object().expect("error response is a JSON object");
    assert_eq!(root.len(), 1, "unexpected error envelope fields: {body:?}");
    let error = root
        .get("error")
        .and_then(Value::as_object)
        .expect("response has an error object");
    assert_eq!(error.len(), 2, "unexpected error fields: {body:?}");
    assert_eq!(error.get("code").and_then(Value::as_str), Some(code));
    assert!(
        error
            .get("message")
            .and_then(Value::as_str)
            .is_some_and(|message| !message.is_empty()),
        "error message is absent or empty: {body:?}"
    );
}

#[test]
fn bridge_http_smoke() {
    let bridge = Bridge::spawn();
    let authenticated = bridge.authenticated_headers();
    let authenticated = string_headers(&authenticated);

    let health = bridge.request("GET", "/v1/health", &bridge.addr, &authenticated, &[]);
    assert_eq!(health.status, 200);
    assert_eq!(
        health.json(),
        json!({
            "service": "solvers",
            "version": "0.1.0",
            "apiVersion": 1,
            "busy": false,
        })
    );

    let missing_token = bridge.request(
        "GET",
        "/v1/health",
        &bridge.addr,
        &[("Origin", ORIGIN)],
        &[],
    );
    assert_error(&missing_token, 401, "unauthorized");

    let authorization = format!("Bearer {}", bridge.token);
    let wrong_origin = bridge.request(
        "GET",
        "/v1/health",
        &bridge.addr,
        &[
            ("Origin", "http://attacker.invalid"),
            ("Authorization", &authorization),
        ],
        &[],
    );
    assert_error(&wrong_origin, 403, "forbidden_origin");

    let wrong_host = bridge.request("GET", "/v1/health", "attacker.invalid", &authenticated, &[]);
    assert_error(&wrong_host, 403, "forbidden_host");

    let preflight = bridge.request(
        "OPTIONS",
        "/v1/jobs",
        &bridge.addr,
        &[
            ("Origin", ORIGIN),
            ("Access-Control-Request-Method", "POST"),
            (
                "Access-Control-Request-Headers",
                "Authorization, Content-Type",
            ),
        ],
        &[],
    );
    assert_eq!(preflight.status, 204);
    assert_eq!(
        preflight.header("Access-Control-Allow-Origin"),
        Some(ORIGIN)
    );
    assert_eq!(
        preflight.header("Access-Control-Allow-Methods"),
        Some("GET, POST, OPTIONS")
    );
    assert_eq!(
        preflight.header("Access-Control-Allow-Headers"),
        Some("Authorization, Content-Type")
    );

    let kuhn_toml = r#"[game]
kind = "kuhn"

[algorithm]
schedule = "dcfr"

[run]
iterations = 1
check_every = 1
"#;
    let kuhn_body =
        serde_json::to_vec(&json!({ "configToml": kuhn_toml })).expect("serialize Kuhn request");
    let mut post_headers = authenticated.clone();
    post_headers.push(("Content-Type", "application/json"));
    let invalid_config =
        bridge.request("POST", "/v1/jobs", &bridge.addr, &post_headers, &kuhn_body);
    assert_error(&invalid_config, 400, "invalid_config");

    let oversized_toml = "x".repeat(64 * 1024);
    let oversized_body = serde_json::to_vec(&json!({ "configToml": oversized_toml }))
        .expect("serialize oversized request");
    assert!(oversized_body.len() > 64 * 1024);
    let oversized = bridge.request(
        "POST",
        "/v1/jobs",
        &bridge.addr,
        &post_headers,
        &oversized_body,
    );
    assert_error(&oversized, 413, "body_too_large");
}
