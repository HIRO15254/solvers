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
    now=1700000000000
    # The real CLI records `running` before it does any work, so a crash
    # never leaves a started run looking like it never started. The stub
    # does the same, then optionally holds its slot.
    printf '{"schemaVersion":1,"runId":"%s","state":"running","gameKind":"kuhn",' \
      "$(basename "$directory")" > "$directory/manifest.json"
    printf '"configSchema":"solvers.toy/v1","configHash":"aa","cliVersion":"stub",' \
      >> "$directory/manifest.json"
    printf '"command":["solve"],"pid":%s,"createdUnixMs":%s,"startedUnixMs":%s,' \
      "$$" "$now" "$now" >> "$directory/manifest.json"
    printf '"finishedUnixMs":null,"failure":null,"completion":null}\n' \
      >> "$directory/manifest.json"
    printf '{"seq":0,"unixMs":%s,"level":"info","kind":"state","state":"running"}\n' \
      "$now" > "$directory/events.jsonl"
    # A config marked BLOCK holds its slot, so a test can observe queueing.
    if grep -q "BLOCK" "$directory/run.toml" 2>/dev/null; then sleep 30; fi
    printf '{"schemaVersion":1,"runId":"%s","state":"completed","gameKind":"kuhn",' \
      "$(basename "$directory")" > "$directory/manifest.json"
    printf '"configSchema":"solvers.toy/v1","configHash":"aa","cliVersion":"stub",' \
      >> "$directory/manifest.json"
    printf '"command":["solve"],"pid":1,"createdUnixMs":%s,"startedUnixMs":%s,' \
      "$now" "$now" >> "$directory/manifest.json"
    printf '"finishedUnixMs":%s,"failure":null,"completion":"completed"}\n' \
      "$now" >> "$directory/manifest.json"
    printf '{"seq":1,"unixMs":%s,"level":"info","kind":"state","state":"completed"}\n' \
      "$now" >> "$directory/events.jsonl"
    printf '{"iteration":42,"elapsedSecs":0.5}\n' > "$directory/progress.jsonl"
    ;;
esac
"#;

struct Daemon {
    child: Child,
    port: u16,
    /// Kept alive for the daemon's lifetime when this instance owns it. A
    /// restart test supplies its own directory instead, so the runs root
    /// and the stub survive the first daemon.
    #[allow(dead_code)]
    home: Option<tempfile::TempDir>,
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
        let mut daemon = Self::start_in(home.path(), max_concurrent);
        daemon.home = Some(home);
        daemon
    }

    /// Starts a daemon over `home`, writing the stub solver there if it is
    /// not already present. Restarting means calling this twice.
    fn start_in(home: &Path, max_concurrent: usize) -> Self {
        let solver = home.join("solvers");
        if !solver.exists() {
            std::fs::write(&solver, STUB_SOLVER).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&solver, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        let runs = home.join("runs");

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

        let port = read_port(&mut child);

        Self {
            child,
            port,
            home: None,
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

/// The daemon prints the port it bound, which is how a test finds an
/// OS-assigned one.
///
/// Read line by line rather than in one chunk: the daemon prints more after
/// the banner, and a fixed read could split a line in half.
fn read_port(child: &mut Child) -> u16 {
    let stdout = child.stdout.as_mut().expect("daemon stdout");
    let mut reader = std::io::BufReader::new(stdout);
    let mut line = String::new();
    for _ in 0..10 {
        line.clear();
        let read = std::io::BufRead::read_line(&mut reader, &mut line).expect("read a banner line");
        assert!(read > 0, "the daemon exited before announcing a port");
        if let Some(port) = line
            .split("(port ")
            .nth(1)
            .and_then(|rest| rest.split(')').next())
            .and_then(|port| port.parse().ok())
        {
            return port;
        }
    }
    panic!("no port in the daemon's first lines: {line:?}");
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

/// A client needs the run's outputs, not just its status. The listing is an
/// allow-list of the contract's file names, so a run directory never
/// becomes a general file share.
#[test]
fn artifacts_are_listed_and_downloadable_by_contract_name() {
    let daemon = Daemon::start(1);
    let (_, created) = daemon.post("/v1/runs", &config("artifacts"));
    let run_id = created["runId"].as_str().unwrap().to_string();
    daemon.await_terminal(&run_id);

    let (status, listing) = daemon.get(&format!("/v1/runs/{run_id}/artifacts"));
    assert_eq!(status, 200);
    let names: Vec<&str> = listing["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"manifest.json"), "{names:?}");
    assert!(names.contains(&"run.toml"), "{names:?}");
    assert!(names.contains(&"events.jsonl"), "{names:?}");
    // Only files that exist are listed, with their real size. An empty
    // `stdout.log` is a legitimate entry -- a run that printed nothing.
    assert!(!names.contains(&"solution.mwsol"), "{names:?}");
    let manifest = listing["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == "manifest.json")
        .unwrap();
    assert_eq!(
        manifest["bytes"].as_u64().unwrap(),
        std::fs::metadata(daemon.runs.join(&run_id).join("manifest.json"))
            .unwrap()
            .len()
    );

    let (status, body) = daemon.request(
        "GET",
        &format!("/v1/runs/{run_id}/artifacts/run.toml"),
        Some("test-token"),
        None,
    );
    assert_eq!(status, 200);
    assert!(body.contains("solvers.toy/v1"), "{body:?}");
}

/// Anything outside the contract is not reachable, however it is spelled.
#[test]
fn an_artifact_outside_the_contract_is_not_found() {
    let daemon = Daemon::start(1);
    let (_, created) = daemon.post("/v1/runs", &config("allowlist"));
    let run_id = created["runId"].as_str().unwrap().to_string();
    daemon.await_terminal(&run_id);

    for name in ["..%2F..%2Fetc%2Fpasswd", ".cache", "run.toml.bak"] {
        let (status, _) = daemon.get(&format!("/v1/runs/{run_id}/artifacts/{name}"));
        assert_eq!(status, 404, "{name} was reachable");
    }
}

/// Asking for a file the run has not produced is not the same as asking for
/// one that does not exist in the contract.
#[test]
fn a_missing_artifact_is_reported_as_unavailable() {
    let daemon = Daemon::start(1);
    let (_, created) = daemon.post("/v1/runs", &config("missing"));
    let run_id = created["runId"].as_str().unwrap().to_string();
    daemon.await_terminal(&run_id);

    let (status, error) = daemon.get(&format!("/v1/runs/{run_id}/artifacts/solution.mwsol"));
    assert_eq!(status, 409);
    assert_eq!(error["code"], "unavailable");
}

#[test]
fn a_solution_view_of_a_run_without_one_is_unavailable() {
    let daemon = Daemon::start(1);
    let (_, created) = daemon.post("/v1/runs", &config("noview"));
    let run_id = created["runId"].as_str().unwrap().to_string();
    daemon.await_terminal(&run_id);

    let (status, error) = daemon.get(&format!("/v1/runs/{run_id}/solution/summary"));
    assert_eq!(status, 409);
    assert_eq!(error["code"], "unavailable");

    let (status, _) = daemon.get(&format!("/v1/runs/{run_id}/solution/nonsense"));
    assert_eq!(status, 404);
}

/// A queued run outlives the daemon that accepted it.
///
/// Its directory is the only record that it was accepted, so a restart has
/// to find it and start it. Otherwise "the daemon keeps no state" would
/// mean "the daemon forgets".
#[test]
fn a_restart_picks_up_a_run_that_was_still_queued() {
    let home = tempfile::tempdir().expect("temp home");
    let daemon = Daemon::start_in(home.path(), 1);
    let (_, blocking) = daemon.post("/v1/runs", &config("BLOCK"));
    assert_eq!(blocking["state"], "running");
    let (_, waiting) = daemon.post("/v1/runs", &config("waits-for-a-restart"));
    assert_eq!(waiting["state"], "queued");
    let waiting_id = waiting["runId"].as_str().unwrap().to_string();

    // Restart over the same directory, with nothing holding a slot.
    drop(daemon);
    let restarted = Daemon::start_in(home.path(), 1);

    let recovered = restarted.await_terminal(&waiting_id);
    assert_eq!(recovered["state"], "completed");
}
