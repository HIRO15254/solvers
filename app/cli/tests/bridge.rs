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

fn wait_for_terminal_job(bridge: &Bridge, id: &str) -> Value {
    let authenticated = bridge.authenticated_headers();
    let authenticated = string_headers(&authenticated);
    let started = std::time::Instant::now();
    loop {
        let response = bridge.request(
            "GET",
            &format!("/v2/jobs/{id}"),
            &bridge.addr,
            &authenticated,
            &[],
        );
        assert_eq!(response.status, 200, "body: {:?}", response.json());
        let body = response.json();
        match body["status"].as_str() {
            Some("succeeded" | "cancelled" | "resource_limit") => return body,
            Some("failed") => panic!("bridge v2 job failed: {body:?}"),
            Some("running" | "cancelling") => {}
            status => panic!("unexpected bridge job status {status:?}: {body:?}"),
        }
        assert!(
            started.elapsed() < Duration::from_secs(300),
            "bridge v2 job did not finish: {body:?}"
        );
        thread::sleep(Duration::from_millis(25));
    }
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

    let health_v2 = bridge.request("GET", "/v2/health", &bridge.addr, &authenticated, &[]);
    assert_eq!(health_v2.status, 200);
    let health_v2 = health_v2.json();
    assert_eq!(health_v2["apiVersion"], 2);
    assert_eq!(health_v2["capabilities"]["maxPlayers"], 9);
    assert_eq!(health_v2["capabilities"]["maxIcmField"], 10_000);
    assert_eq!(health_v2["capabilities"]["exactIcmField"], 15);
    assert_eq!(
        health_v2["capabilities"]["stages"],
        json!(["preflop", "all-streets", "tournament-icm"])
    );
    assert_eq!(
        health_v2["capabilities"]["artifacts"],
        json!(["result", "strategies", "checkpoint"])
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

    let multiway_toml = include_str!("../../../examples/preflop_multiway_bridge_compat_smoke.toml");
    let validate_body = serde_json::to_vec(&json!({ "configToml": multiway_toml })).unwrap();
    let validated = bridge.request(
        "POST",
        "/v2/validate",
        &bridge.addr,
        &post_headers,
        &validate_body,
    );
    assert_eq!(validated.status, 200, "body: {:?}", validated.json());
    assert_eq!(
        validated.json(),
        json!({ "valid": true, "schemaVersion": 3 })
    );

    let unsafe_resume_body = serde_json::to_vec(&json!({
        "configToml": multiway_toml,
        "resumeCheckpointUrl": "C:\\arbitrary\\checkpoint.mwckpt",
    }))
    .unwrap();
    let unsafe_resume = bridge.request(
        "POST",
        "/v2/jobs",
        &bridge.addr,
        &post_headers,
        &unsafe_resume_body,
    );
    assert_error(&unsafe_resume, 400, "invalid_resume_checkpoint");

    let missing_id = "ab".repeat(16);
    let missing_resume_body = serde_json::to_vec(&json!({
        "configToml": multiway_toml,
        "resumeCheckpointUrl": format!("/v2/jobs/{missing_id}/checkpoint"),
    }))
    .unwrap();
    let missing_resume = bridge.request(
        "POST",
        "/v2/jobs",
        &bridge.addr,
        &post_headers,
        &missing_resume_body,
    );
    assert_error(&missing_resume, 404, "resume_checkpoint_not_found");

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

#[test]
#[ignore = "builds the full EHS2 tables; explicit release acceptance only"]
fn bridge_v2_multiway_lifecycle_and_managed_resume() {
    let bridge = Bridge::spawn();
    let authenticated = bridge.authenticated_headers();
    let mut post_headers = string_headers(&authenticated);
    post_headers.push(("Content-Type", "application/json"));

    let config_toml = include_str!("../../../examples/preflop_multiway_bridge_compat_smoke.toml")
        .replace("sweeps = 2", "sweeps = 1");
    let create_body = serde_json::to_vec(&json!({ "configToml": config_toml })).unwrap();
    let created = bridge.request(
        "POST",
        "/v2/jobs",
        &bridge.addr,
        &post_headers,
        &create_body,
    );
    assert_eq!(created.status, 202, "body: {:?}", created.json());
    let first_id = created.json()["id"]
        .as_str()
        .expect("created job id")
        .to_owned();
    let authenticated_get = bridge.authenticated_headers();
    let authenticated_get = string_headers(&authenticated_get);
    let hidden_from_v1 = bridge.request(
        "GET",
        &format!("/v1/jobs/{first_id}"),
        &bridge.addr,
        &authenticated_get,
        &[],
    );
    assert_error(&hidden_from_v1, 404, "not_found");
    let first = wait_for_terminal_job(&bridge, &first_id);
    assert_eq!(first["status"], "succeeded");
    let checkpoint_url = first["checkpointUrl"]
        .as_str()
        .expect("terminal multiway job checkpoint URL")
        .to_owned();

    let result = bridge.request(
        "GET",
        first["resultUrl"].as_str().unwrap(),
        &bridge.addr,
        &authenticated_get,
        &[],
    );
    assert_eq!(result.status, 200);
    assert_eq!(result.json()["schemaVersion"], 2);
    assert_eq!(result.json()["approximateProfile"], true);

    let v1_result = bridge.request(
        "GET",
        &format!("/v1/jobs/{first_id}/result"),
        &bridge.addr,
        &authenticated_get,
        &[],
    );
    assert_error(&v1_result, 404, "not_found");

    let strategies = bridge.request(
        "GET",
        &format!("/v2/jobs/{first_id}/strategies?cursor=0&limit=2"),
        &bridge.addr,
        &authenticated_get,
        &[],
    );
    assert_eq!(strategies.status, 200, "body: {:?}", strategies.json());
    assert!(strategies.json()["items"].is_array());

    let checkpoint = bridge.request(
        "GET",
        &checkpoint_url,
        &bridge.addr,
        &authenticated_get,
        &[],
    );
    assert_eq!(checkpoint.status, 200);
    assert_eq!(checkpoint.header("Cache-Control"), Some("no-store"));
    assert_eq!(
        checkpoint.header("Access-Control-Allow-Origin"),
        Some(ORIGIN)
    );
    assert_eq!(checkpoint.header("Vary"), Some("Origin"));
    assert_eq!(
        checkpoint.header("Content-Length"),
        Some(checkpoint.body.len().to_string().as_str())
    );
    assert_eq!(
        checkpoint.header("Content-Type"),
        Some("application/octet-stream")
    );
    assert!(!checkpoint.body.is_empty());

    let resume_body = serde_json::to_vec(&json!({
        "configToml": config_toml,
        "resumeCheckpointUrl": checkpoint_url,
    }))
    .unwrap();
    let resumed = bridge.request(
        "POST",
        "/v2/jobs",
        &bridge.addr,
        &post_headers,
        &resume_body,
    );
    assert_eq!(resumed.status, 202, "body: {:?}", resumed.json());
    let resumed_id = resumed.json()["id"]
        .as_str()
        .expect("resumed job id")
        .to_owned();
    let resumed = wait_for_terminal_job(&bridge, &resumed_id);
    assert_eq!(resumed["status"], "succeeded");
    let resumed_result = bridge.request(
        "GET",
        resumed["resultUrl"].as_str().unwrap(),
        &bridge.addr,
        &authenticated_get,
        &[],
    );
    assert_eq!(resumed_result.status, 200);
    assert_eq!(resumed_result.json()["sweeps"], 1);
}

#[test]
#[ignore = "builds full EHS2 tables before running a cancellable release worker"]
fn bridge_v2_cancel_interrupts_a_large_solver_chunk() {
    let bridge = Bridge::spawn();
    let authenticated = bridge.authenticated_headers();
    let mut post_headers = string_headers(&authenticated);
    post_headers.push(("Content-Type", "application/json"));
    let base = include_str!("../../../examples/preflop_multiway_bridge_compat_smoke.toml");
    let config_toml = base
        .replace("sweeps = 2", "sweeps = 10000000")
        .replace("check_every = 1", "check_every = 10000000")
        .replace("checkpoint_every = 1", "checkpoint_every = 10000000")
        .replace("evaluation_cadence = 1", "evaluation_cadence = 10000000");
    assert_ne!(
        config_toml, base,
        "large-chunk cancellation fixture changed"
    );
    let create_body = serde_json::to_vec(&json!({ "configToml": config_toml })).unwrap();
    let created = bridge.request(
        "POST",
        "/v2/jobs",
        &bridge.addr,
        &post_headers,
        &create_body,
    );
    assert_eq!(created.status, 202, "body: {:?}", created.json());
    let id = created.json()["id"].as_str().unwrap().to_owned();

    // Give the worker time to enter the 10M-sweep chunk. The old worker only
    // checked cancellation after that entire chunk and would hang here.
    thread::sleep(Duration::from_millis(500));
    let cancelled = bridge.request(
        "POST",
        &format!("/v2/jobs/{id}/cancel"),
        &bridge.addr,
        &post_headers,
        &[],
    );
    assert_eq!(cancelled.status, 202, "body: {:?}", cancelled.json());

    let get_headers = bridge.authenticated_headers();
    let get_headers = string_headers(&get_headers);
    let started = std::time::Instant::now();
    let terminal = loop {
        let response = bridge.request(
            "GET",
            &format!("/v2/jobs/{id}"),
            &bridge.addr,
            &get_headers,
            &[],
        );
        assert_eq!(response.status, 200, "body: {:?}", response.json());
        let body = response.json();
        match body["status"].as_str() {
            Some("cancelled") => break body,
            Some("running" | "cancelling") => {}
            status => panic!("unexpected cancellation status {status:?}: {body:?}"),
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "cancel was not observed at a sweep boundary: {body:?}"
        );
        thread::sleep(Duration::from_millis(25));
    };
    let result = bridge.request(
        "GET",
        terminal["resultUrl"].as_str().unwrap(),
        &bridge.addr,
        &get_headers,
        &[],
    );
    assert_eq!(result.status, 200);
    let result = result.json();
    assert_eq!(result["status"], "cancelled");
    assert!(result["sweeps"].as_u64().unwrap() < 10_000_000);
}
