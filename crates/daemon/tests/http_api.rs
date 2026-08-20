//! End-to-end tests over a real socket, against a stub solver.
//!
//! The daemon's job is orchestration -- accept a config, prepare a run
//! directory, spawn something into it, and serve what that directory says.
//! A stub solver exercises all of that in milliseconds; the real CLI is
//! covered by its own tests and by the ignored acceptance runs.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// A `solvers` stand-in.
///
/// `validate` echoes the config back as its own effective form, and `solve`
/// writes the manifest and events a real run would. That is exactly the
/// surface the daemon depends on, so a change to it fails here.
const STUB_SOLVER: &str = r#"#!/bin/sh
set -e
# Skip the global flags the daemon may pass.
while [ "$1" = "--cache-dir" ]; do shift 2; done
command="$1"; shift
case "$command" in
  validate)
    config="$1"; shift
    effective=""
    while [ $# -gt 0 ]; do
      case "$1" in
        --write-effective) effective="$2"; shift 2;;
        *) shift;;
      esac
    done
    if grep -q "REJECT" "$config"; then
      echo "MWP999: the stub was asked to reject this config" >&2
      exit 1
    fi
    if grep -q "NEEDS_FILE" "$config"; then
      echo "reading mwtree source /nowhere/tree.mwtree" >&2
      exit 1
    fi
    [ -n "$effective" ] && cp "$config" "$effective"
    echo '{"status":"valid","schema":"solvers.toy/v1"}'
    ;;
  solve|resume)
    directory=""
    for argument in "$@"; do
      case "$previous" in --out) directory="$argument";; esac
      previous="$argument"
    done
    [ "$command" = resume ] && directory="$1"
    # A config marked BLOCK holds its slot, so a test can observe queueing.
    if grep -q "BLOCK" "$directory/run.toml" 2>/dev/null; then sleep 30; fi
    now=1700000000000
    printf '{"schemaVersion":1,"runId":"%s","state":"completed","gameKind":"kuhn",' \
      "$(basename "$directory")" > "$directory/manifest.json"
    printf '"configSchema":"solvers.toy/v1","configHash":"aa","cliVersion":"stub",' \
      >> "$directory/manifest.json"
    printf '"command":["solve"],"pid":1,"createdUnixMs":%s,"startedUnixMs":%s,' \
      "$now" "$now" >> "$directory/manifest.json"
    printf '"finishedUnixMs":%s,"failure":null,"completion":"completed"}\n' \
      "$now" >> "$directory/manifest.json"
    printf '{"seq":0,"unixMs":%s,"level":"info","kind":"state","state":"running"}\n' \
      "$now" > "$directory/events.jsonl"
    printf '{"seq":1,"unixMs":%s,"level":"info","kind":"state","state":"completed"}\n' \
      "$now" >> "$directory/events.jsonl"
    printf '{"iteration":42,"elapsedSecs":0.5}\n' > "$directory/progress.jsonl"
    ;;
esac
"#;

struct Daemon {
    child: Child,
    port: u16,
    #[allow(dead_code)]
    home: tempfile::TempDir,
    runs: PathBuf,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Daemon {
    fn start(max_concurrent: usize) -> Self {
        let home = tempfile::tempdir().expect("temp home");
        let solver = home.path().join("solvers");
        std::fs::write(&solver, STUB_SOLVER).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&solver, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let runs = home.path().join("runs");

        // Port 0 asks the OS for a free port, which the daemon prints.
        let mut child = Command::new(env!("CARGO_BIN_EXE_solversd"))
            .args(["--bind", "127.0.0.1:0", "--token", "test-token"])
            .arg("--runs")
            .arg(&runs)
            .arg("--solver")
            .arg(&solver)
            .arg("--max-concurrent")
            .arg(max_concurrent.to_string())
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn solversd");

        let mut banner = [0u8; 256];
        let read = child
            .stdout
            .as_mut()
            .expect("daemon stdout")
            .read(&mut banner)
            .expect("read the daemon banner");
        let banner = String::from_utf8_lossy(&banner[..read]).into_owned();
        let port = banner
            .split("(port ")
            .nth(1)
            .and_then(|rest| rest.split(')').next())
            .and_then(|port| port.parse().ok())
            .unwrap_or_else(|| panic!("no port in banner: {banner:?}"));

        Self {
            child,
            port,
            home,
            runs,
        }
    }

    fn request(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<&str>,
    ) -> (u16, String) {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).expect("connect");
        let body = body.unwrap_or("");
        let mut request = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        );
        if let Some(token) = token {
            request.push_str(&format!("Authorization: Bearer {token}\r\n"));
        }
        request.push_str("\r\n");
        request.push_str(body);
        stream.write_all(request.as_bytes()).expect("write request");

        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .expect("read the response");
        let status = response
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse().ok())
            .unwrap_or(0);
        let body = response
            .split_once("\r\n\r\n")
            .map(|(_, body)| body.to_string())
            .unwrap_or_default();
        (status, body)
    }

    fn get(&self, path: &str) -> (u16, serde_json::Value) {
        let (status, body) = self.request("GET", path, Some("test-token"), None);
        (status, parse(&body))
    }

    fn post(&self, path: &str, body: &str) -> (u16, serde_json::Value) {
        let (status, body) = self.request("POST", path, Some("test-token"), Some(body));
        (status, parse(&body))
    }

    /// Polls until a run reaches a terminal state.
    fn await_terminal(&self, run_id: &str) -> serde_json::Value {
        for _ in 0..200 {
            let (_, run) = self.get(&format!("/v1/runs/{run_id}"));
            if run["state"] == "completed" || run["state"] == "failed" || run["state"] == "canceled"
            {
                return run;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        panic!("run {run_id} never finished");
    }
}

fn parse(body: &str) -> serde_json::Value {
    serde_json::from_str(body).unwrap_or_else(|error| panic!("not JSON: {body:?} ({error})"))
}

fn config(marker: &str) -> String {
    serde_json::json!({ "configToml": format!("schema = \"solvers.toy/v1\"\n# {marker}\n") })
        .to_string()
}

#[test]
fn a_request_without_the_token_is_refused() {
    let daemon = Daemon::start(1);
    let (status, _) = daemon.request("GET", "/v1", None, None);
    assert_eq!(status, 401);
    let (status, _) = daemon.request("GET", "/v1", Some("wrong"), None);
    assert_eq!(status, 401);
    let (status, info) = daemon.get("/v1");
    assert_eq!(status, 200);
    assert_eq!(info["protocolVersion"], 1);
}

/// The whole point of the daemon: hand it a config, get a run directory
/// that every other reader -- `solvers status`, a GUI -- can read too.
#[test]
fn a_submitted_config_becomes_a_run_directory() {
    let daemon = Daemon::start(1);
    let (status, created) = daemon.post("/v1/runs", &config("first"));
    assert_eq!(status, 200, "{created}");
    let run_id = created["runId"].as_str().unwrap().to_string();

    let finished = daemon.await_terminal(&run_id);
    assert_eq!(finished["state"], "completed");
    assert_eq!(finished["progress"], 42);

    // The daemon holds no state of its own: everything it served is on disk.
    let directory = daemon.runs.join(&run_id);
    assert!(directory.join("manifest.json").is_file());
    assert!(directory.join("run.toml").is_file());
    assert!(directory.join("events.jsonl").is_file());
}

#[test]
fn events_resume_from_the_offset_the_previous_page_reported() {
    let daemon = Daemon::start(1);
    let (_, created) = daemon.post("/v1/runs", &config("events"));
    let run_id = created["runId"].as_str().unwrap().to_string();
    daemon.await_terminal(&run_id);

    let (status, page) = daemon.get(&format!("/v1/runs/{run_id}/events?from=0"));
    assert_eq!(status, 200);
    let seqs: Vec<u64> = page["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["seq"].as_u64().unwrap())
        .collect();
    assert_eq!(seqs, vec![0, 1]);
    assert_eq!(page["terminal"], true);

    let offset = page["nextOffset"].as_u64().unwrap();
    let (_, empty) = daemon.get(&format!("/v1/runs/{run_id}/events?from={offset}"));
    assert!(empty["events"].as_array().unwrap().is_empty());
    assert_eq!(empty["nextOffset"], offset);
}

/// A config the daemon cannot resolve on its own filesystem is refused with
/// a code that says why, rather than being resolved against whatever the
/// daemon happens to have there (R10).
#[test]
fn a_config_that_names_a_file_is_refused_as_not_self_contained() {
    let daemon = Daemon::start(1);
    let body = serde_json::json!({
        "configToml": "schema = \"solvers.toy/v1\"\n# NEEDS_FILE\n"
    })
    .to_string();
    let (status, error) = daemon.post("/v1/runs", &body);
    assert_eq!(status, 400);
    assert_eq!(error["code"], "config-not-self-contained");
}

#[test]
fn an_invalid_config_is_refused_before_a_run_directory_exists() {
    let daemon = Daemon::start(1);
    let body =
        serde_json::json!({ "configToml": "schema = \"solvers.toy/v1\"\n# REJECT\n" }).to_string();
    let (status, error) = daemon.post("/v1/runs", &body);
    assert_eq!(status, 400);
    assert_eq!(error["code"], "invalid-config");
    assert!(
        std::fs::read_dir(&daemon.runs)
            .map(|entries| entries.count() == 0)
            .unwrap_or(true),
        "a rejected config must not leave a run directory behind"
    );
}

#[test]
fn unknown_runs_and_routes_report_not_found() {
    let daemon = Daemon::start(1);
    let (status, error) = daemon.get("/v1/runs/nope");
    assert_eq!(status, 404);
    assert_eq!(error["code"], "not-found");

    let (status, _) = daemon.get("/v1/nothing-here");
    assert_eq!(status, 404);

    // A run id must not be able to address anything outside the runs root.
    let (status, _) = daemon.get("/v1/runs/..%2Fescape");
    assert_eq!(status, 404);
}

#[test]
fn cancelling_a_finished_run_is_a_conflict() {
    let daemon = Daemon::start(1);
    let (_, created) = daemon.post("/v1/runs", &config("cancel"));
    let run_id = created["runId"].as_str().unwrap().to_string();
    daemon.await_terminal(&run_id);

    let (status, error) = daemon.post(&format!("/v1/runs/{run_id}/cancel"), "");
    assert_eq!(status, 409);
    assert_eq!(error["code"], "conflict");
}

/// A run the client named twice must not silently reuse the first one's
/// directory.
#[test]
fn a_duplicate_run_id_is_a_conflict() {
    let daemon = Daemon::start(1);
    let body = serde_json::json!({
        "configToml": "schema = \"solvers.toy/v1\"\n",
        "runId": "named"
    })
    .to_string();
    let (status, _) = daemon.post("/v1/runs", &body);
    assert_eq!(status, 200);
    daemon.await_terminal("named");

    let (status, error) = daemon.post("/v1/runs", &body);
    assert_eq!(status, 409);
    assert_eq!(error["code"], "conflict");
}

fn run_ids(daemon: &Daemon) -> Vec<String> {
    let (_, listing) = daemon.get("/v1/runs");
    listing["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|run| run["runId"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn the_listing_reports_every_run_the_root_holds() {
    let daemon = Daemon::start(1);
    for marker in ["a", "b", "c"] {
        let (_, created) = daemon.post("/v1/runs", &config(marker));
        daemon.await_terminal(created["runId"].as_str().unwrap());
    }
    assert_eq!(run_ids(&daemon).len(), 3);
}

/// Beyond the concurrency limit a run waits, and a waiting run is already
/// on disk -- that is what lets a restarted daemon find it again.
#[test]
fn a_run_past_the_limit_is_queued_and_visible() {
    let daemon = Daemon::start(1);
    let (_, first) = daemon.post("/v1/runs", &config("BLOCK"));
    assert_eq!(first["state"], "running");
    let (_, second) = daemon.post("/v1/runs", &config("waits"));
    assert_eq!(second["state"], "queued");

    let waiting = second["runId"].as_str().unwrap();
    let (_, run) = daemon.get(&format!("/v1/runs/{waiting}"));
    assert_eq!(run["state"], "queued");
    assert!(
        Path::new(&daemon.runs)
            .join(waiting)
            .join("run.toml")
            .is_file(),
        "a queued run must already be on disk"
    );
}
