//! End-to-end tests over real sockets.
//!
//! A native stub covers process orchestration cheaply. The solution-view
//! contract also uses the real CLI to solve and export a small P1 artifact.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;

/// Build the std-only native `solvers` stand-in once for this test process.
/// A native helper keeps the process orchestration assertions portable and,
/// unlike a shell script, can be spawned and killed directly on Windows.
fn stub_solver() -> &'static Path {
    struct StubSolver {
        _directory: tempfile::TempDir,
        executable: PathBuf,
    }

    static STUB: OnceLock<StubSolver> = OnceLock::new();
    &STUB
        .get_or_init(|| {
            let directory = tempfile::tempdir().expect("stub build directory");
            let source = directory.path().join("solver_stub.rs");
            std::fs::write(&source, include_str!("support/solver_stub.rs"))
                .expect("write solver stub source");
            let executable = directory
                .path()
                .join(format!("solver-stub{}", std::env::consts::EXE_SUFFIX));
            let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
            let status = Command::new(rustc)
                .arg("--edition=2024")
                .arg(&source)
                .arg("-o")
                .arg(&executable)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .status()
                .expect("spawn rustc for solver stub");
            assert!(status.success(), "compile solver stub");
            StubSolver {
                _directory: directory,
                executable,
            }
        })
        .executable
}

/// The real CLI is built by the workspace verification before these tests.
/// An explicit override also supports testing an installed CLI.
fn real_solver() -> PathBuf {
    let executable = std::env::var_os("SOLVERS_TEST_SOLVER")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_BIN_EXE_solversd"))
                .with_file_name(format!("solvers{}", std::env::consts::EXE_SUFFIX))
        });
    assert!(
        executable.is_file(),
        "real CLI missing at {}; run cargo build -p cli first or set SOLVERS_TEST_SOLVER",
        executable.display()
    );
    executable
}

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

    /// Starts a daemon over `home`. Restarting means calling this twice.
    fn start_in(home: &Path, max_concurrent: usize) -> Self {
        Self::start_with_solver(home, max_concurrent, stub_solver())
    }

    fn start_with_solver(home: &Path, max_concurrent: usize, solver: &Path) -> Self {
        let runs = home.join("runs");

        // Port 0 asks the OS for a free port, which the daemon prints.
        let mut child = Command::new(env!("CARGO_BIN_EXE_solversd"))
            .args(["--bind", "127.0.0.1:0", "--token", "test-token"])
            .arg("--runs")
            .arg(&runs)
            .arg("--solver")
            .arg(solver)
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

#[test]
fn config_tree_preview_delegates_to_real_cli() {
    let home = tempfile::tempdir().expect("temp home");
    let daemon = Daemon::start_with_solver(home.path(), 1, &real_solver());
    let config = include_str!("../../../examples/hu-postflop/river_small.toml");
    let body = serde_json::json!({ "configToml": config }).to_string();
    let (status, rows) = daemon.post("/v1/tree", &body);
    assert_eq!(status, 200, "{rows}");
    assert_eq!(rows.as_array().unwrap().len(), 1);
    assert_eq!(rows[0]["history"], "");
    assert_eq!(rows[0]["kind"], "action");
    assert_eq!(rows[0]["street"], "river");
    assert_eq!(rows[0]["actor"], "oop");
    assert!(rows[0]["pot"].as_f64().unwrap() > 0.0);
    assert_eq!(rows[0].as_object().unwrap().len(), 8);
    assert!(rows[0].get("stored").is_none());
    let child = rows[0]["children"][0].as_str().unwrap();
    let body = serde_json::json!({ "configToml": config, "nodes": [child] }).to_string();
    let (status, selected) = daemon.post("/v1/tree", &body);
    assert_eq!(status, 200, "{selected}");
    assert_eq!(selected[0]["history"], child);
    let body = serde_json::json!({ "configToml": config, "restOfStreet": true }).to_string();
    let (status, street) = daemon.post("/v1/tree", &body);
    assert_eq!(status, 200, "{street}");
    let histories: Vec<&str> = street
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["history"].as_str().unwrap())
        .collect();
    assert_eq!(histories, ["", "x", "xr1", "xr1r2.5", "r1", "r1r2.5"]);
    let (status, error) = daemon.post("/v1/tree", r#"{"configToml":"not valid TOML"}"#);
    assert_eq!(status, 400, "{error}");
    assert_eq!(error["code"], "invalid-config");
    assert!(error["message"].as_str().unwrap().contains("NLH"));
    let body = serde_json::json!({ "configToml": config, "nodes": ["not-a-node"] }).to_string();
    let (status, error) = daemon.post("/v1/tree", &body);
    assert_eq!(status, 400, "{error}");
    assert_eq!(error["code"], "invalid-config");
    assert!(error["message"].as_str().unwrap().contains("not-a-node"));
    assert!(!daemon.runs.exists() || std::fs::read_dir(&daemon.runs).unwrap().next().is_none());
}

fn config(marker: &str) -> String {
    serde_json::json!({ "configToml": format!("schema = \"solvers.nlh/v1\"\n# {marker}\n") })
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

#[test]
fn line_passes_cli_json_and_preserves_optional_prefixes() {
    let daemon = Daemon::start(1);
    for (moves, expected) in [
        (None, serde_json::Value::Null),
        (Some(vec![]), serde_json::json!("")),
        (Some(vec!["r3", "c"]), serde_json::json!("r3,c")),
    ] {
        let mut body = serde_json::json!({"configToml": "# submitted table\n"});
        if let Some(moves) = moves {
            body["moves"] = serde_json::json!(moves);
        }
        let (status, cursor) = daemon.post("/v1/line", &body.to_string());
        assert_eq!(status, 200, "{cursor}");
        assert_eq!(cursor["receivedMoves"], expected);
        assert_eq!(cursor["pot"], 1.5);
        assert_eq!(cursor["complete"], false);
        assert!(cursor.get("summary").is_none(), "JSON must not be wrapped");
    }
}

#[test]
fn line_invalid_config_preserves_cli_diagnostic() {
    let daemon = Daemon::start(1);
    let (status, error) = daemon.post("/v1/line", &config("REJECT"));
    assert_eq!(status, 400);
    assert_eq!(error["code"], "invalid-config");
    assert_eq!(error["message"], "NLH004: illegal move at index 1");
}

#[test]
fn line_requires_the_same_bearer_token_as_validate() {
    let daemon = Daemon::start(1);
    for token in [None, Some("wrong")] {
        let (status, body) = daemon.request("POST", "/v1/line", token, Some(&config("auth")));
        assert_eq!(status, 401);
        assert_eq!(parse(&body)["code"], "unauthorized");
    }
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
    assert_eq!(
        finished["lastProgress"],
        serde_json::json!({"iteration": 42, "elapsedSecs": 0.5})
    );
    let (_, listed) = daemon.get("/v1/runs");
    let listed_run = listed["runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|run| run["runId"] == run_id)
        .unwrap();
    assert_eq!(listed_run["lastProgress"], finished["lastProgress"]);

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
        "configToml": "schema = \"solvers.nlh/v1\"\n# NEEDS_FILE\n"
    })
    .to_string();
    let (status, error) = daemon.post("/v1/runs", &body);
    assert_eq!(status, 400);
    assert_eq!(error["code"], "config-not-self-contained");
}

#[test]
fn upstream_profile_paths_are_rejected_before_any_config_endpoint_invokes_cli() {
    let daemon = Daemon::start(1);
    for config in [
        "[profile]\nfreeze_upstream = { solution = '/host/secret.sol' }\n",
        "profile.freeze_upstream.solution = '/host/secret.sol'\n",
        "[profile.'freeze_upstream']\nsolution = '/host/secret.sol'\n",
    ] {
        let body = serde_json::json!({ "configToml": config }).to_string();
        for endpoint in ["/v1/validate", "/v1/line", "/v1/tree", "/v1/runs"] {
            let (status, error) = daemon.post(endpoint, &body);
            assert_eq!(status, 400, "{endpoint}: {error}");
            assert_eq!(
                error["code"], "config-not-self-contained",
                "{endpoint}: {error}"
            );
        }
    }
}

#[test]
fn relock_rejects_runs_without_a_p1_solution() {
    let daemon = Daemon::start(1);
    let (_, created) = daemon.post("/v1/runs", &config("no-solution"));
    let run_id = created["runId"].as_str().unwrap();
    daemon.await_terminal(run_id);
    let body = serde_json::json!({"profileToml": ""}).to_string();
    let (status, error) = daemon.post(&format!("/v1/runs/{run_id}/relock"), &body);
    assert_eq!(status, 409, "{error}");
    assert_eq!(error["code"], "conflict");
}

#[test]
fn an_invalid_config_is_refused_before_a_run_directory_exists() {
    let daemon = Daemon::start(1);
    let body =
        serde_json::json!({ "configToml": "schema = \"solvers.nlh/v1\"\n# REJECT\n" }).to_string();
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
        "configToml": "schema = \"solvers.nlh/v1\"\n",
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
    assert!(body.contains("solvers.nlh/v1"), "{body:?}");
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

#[test]
fn postflop_solution_views_use_real_cli_and_ordered_decoded_nodes() {
    let home = tempfile::tempdir().expect("temp home");
    let daemon = Daemon::start_with_solver(home.path(), 1, &real_solver());
    let config = r#"
schema = "solvers.nlh/v1"
[table]
players = 2
stack_bb = 90
ante_bb = 9
[spot]
line = "BTN c, BB x / BB x, BTN x / BB x, BTN x"
board = "2c 7d 9h Js Qs"
[ranges]
BB = "AsAh"
BTN = "KsKh"
[tree]
script = '''river when unopened { force bet [50] }'''
[solver]
storage = "f32"
[solver.stop]
max_iterations = 2
check_every = 1
[run]
threads = 1
"#;
    let body = serde_json::json!({ "configToml": config }).to_string();
    let (status, created) = daemon.post("/v1/runs", &body);
    assert_eq!(status, 200, "{created}");
    let run_id = created["runId"].as_str().unwrap();
    let finished = daemon.await_terminal(run_id);
    assert_eq!(finished["state"], "completed", "{finished}");
    let directory = daemon.runs.join(run_id);
    assert!(directory.join("solution.sol").is_file());
    let progress: serde_json::Value = std::fs::read_to_string(directory.join("progress.jsonl"))
        .unwrap()
        .lines()
        .last()
        .map(|row| serde_json::from_str(row).unwrap())
        .unwrap();
    assert_eq!(finished["lastProgress"], progress);
    assert_eq!(finished["progress"], progress["iteration"]);
    assert!(finished["lastProgress"]["nash_conv"].is_number());
    // Even when both names exist, P1 takes precedence.
    std::fs::write(directory.join("solution.mwsol"), b"not a multiway artifact").unwrap();

    let base = format!("/v1/runs/{run_id}/solution");
    let (status, summary) = daemon.get(&format!("{base}/summary"));
    assert_eq!(status, 200, "{summary}");
    assert!(summary.get("profile").is_none());
    assert!(summary.get("profile_config").is_none());
    let (status, tree) = daemon.get(&format!("{base}/tree?node=root"));
    assert_eq!(status, 200, "{tree}");
    let rows = tree.as_array().expect("tree rows");
    assert_eq!(rows.len(), 1);
    let root = &rows[0];
    assert_eq!(root["kind"], "action");
    assert_eq!(root["stored"], true);
    let body = serde_json::json!({ "configToml": config }).to_string();
    let (status, preview) = daemon.post("/v1/tree", &body);
    assert_eq!(status, 200, "{preview}");
    let mut expected = root.clone();
    expected.as_object_mut().unwrap().remove("stored");
    assert_eq!(preview[0], expected);
    assert!(root["pot"].as_f64().unwrap() > 0.0);
    let actions = root["actions"].as_array().unwrap();
    let children = root["children"].as_array().unwrap();
    let child_kinds = root["child_kinds"].as_array().unwrap();
    assert_eq!(children.len(), actions.len());
    assert_eq!(child_kinds.len(), actions.len());
    let bet = actions
        .iter()
        .position(|action| action == "bet 10")
        .unwrap();
    assert_eq!(child_kinds[bet], "action");

    let (status, selected) = daemon.get(&format!("{base}/tree?node=%62et%2010"));
    assert_eq!(status, 200, "{selected}");
    assert_eq!(selected[0]["history"], children[bet]);

    let (status, strategy) = daemon.get(&format!("{base}/strategy?node=root"));
    assert_eq!(status, 200, "{strategy}");
    let strategy_rows = strategy.as_array().expect("strategy rows");
    assert!(!strategy_rows.is_empty());
    assert!(
        strategy_rows
            .iter()
            .all(|row| row["history"] == root["history"])
    );

    let (status, repeated) = daemon.get(&format!(
        "{base}/tree?node=%62et%2010&node=%72oot&node=bet+10"
    ));
    assert_eq!(status, 200, "{repeated}");
    assert_eq!(repeated.as_array().unwrap().len(), 3);
    assert_eq!(repeated[0], selected[0]);
    assert_eq!(repeated[1], *root);
    assert_eq!(repeated[2], selected[0]);

    let (status, error) = daemon.get(&format!("{base}/strategy?node=not-a-node"));
    assert_eq!(status, 400, "{error}");
    assert_eq!(error["code"], "invalid-request");
    assert!(!error["message"].as_str().unwrap().is_empty());

    let (status, terminal) = daemon.get(&format!("{base}/tree?node=bet%2010%2Ffold"));
    assert_eq!(status, 200, "{terminal}");
    assert_eq!(terminal[0]["kind"], "terminal");
    let (status, error) = daemon.get(&format!("{base}/strategy?node=bet%2010%2Ffold"));
    assert_eq!(status, 400, "{error}");
    assert_eq!(error["code"], "invalid-request");
    assert!(!error["message"].as_str().unwrap().is_empty());

    for node in ["%ZZ", "%FF"] {
        let (status, error) = daemon.get(&format!("{base}/tree?node={node}"));
        assert_eq!(status, 400, "{error}");
        assert_eq!(error["code"], "invalid-request");
    }

    // Artifact parse/IO failures are not client selector errors.
    std::fs::write(directory.join("solution.sol"), b"not a postflop artifact").unwrap();
    let (status, error) = daemon.get(&format!("{base}/tree"));
    assert_eq!(status, 500, "{error}");
    assert_eq!(error["code"], "internal");
}

#[test]
fn postflop_profile_solution_views_preserve_locked_strategy() {
    let home = tempfile::tempdir().expect("temp home");
    let daemon = Daemon::start_with_solver(home.path(), 1, &real_solver());
    let config = r#"
schema = "solvers.nlh/v1"
[table]
players = 2
stack_bb = 90
ante_bb = 9
[spot]
line = "BTN c, BB x / BB x, BTN x / BB x, BTN x"
board = "2c 7d 9h Js Qs"
[ranges]
BB = "AsAh"
BTN = "KsKh"
[tree]
script = '''river when unopened { force bet [50] }'''
[solver]
storage = "f32"
[solver.stop]
max_iterations = 2
check_every = 1
[run]
threads = 1
[[profile.lock]]
node = "root"
strategy = { "bet 10" = 1.0 }
"#;
    let body = serde_json::json!({ "configToml": config }).to_string();
    let (status, created) = daemon.post("/v1/runs", &body);
    assert_eq!(status, 200, "{created}");
    let run_id = created["runId"].as_str().unwrap();
    let finished = daemon.await_terminal(run_id);
    assert_eq!(finished["state"], "completed", "{finished}");
    let base = format!("/v1/runs/{run_id}/solution");
    let (status, tree) = daemon.get(&format!("{base}/tree?node=root"));
    assert_eq!(status, 200, "{tree}");
    let (status, summary) = daemon.get(&format!("{base}/summary"));
    assert_eq!(status, 200, "{summary}");
    let resolved = summary["profile"]["summary"]["resolved_nodes"]
        .as_array()
        .unwrap();
    let lock = resolved
        .iter()
        .find(|entry| entry["kind"] == "lock")
        .unwrap();
    assert_eq!(lock["history"], tree[0]["history"]);
    assert_eq!(lock["node"], 0);
    assert_eq!(lock["entry"], 0);
    assert_eq!(
        summary["profile_config"]["lock"][0]["strategy"]["bet 10"],
        1.0
    );
    let actions = tree[0]["actions"].as_array().unwrap();
    let bet = actions
        .iter()
        .position(|action| action == "bet 10")
        .unwrap();
    let (status, strategy) = daemon.get(&format!("{base}/strategy?node=root"));
    assert_eq!(status, 200, "{strategy}");
    let rows = strategy.as_array().unwrap();
    assert!(!rows.is_empty());
    for row in rows {
        let probabilities = row["probabilities"].as_array().unwrap();
        assert_eq!(probabilities[bet], 1.0);
        assert!(
            probabilities
                .iter()
                .enumerate()
                .all(|(i, p)| i == bet || p == 0.0)
        );
    }

    let fragment =
        "[[profile.lock]]\nnode = \"root\"\nstrategy = { \"bet 10\" = 1.0 }\nconfidence = 0.5\n";
    let request = serde_json::json!({
        "profileToml": fragment,
        "replace": true,
        "freezeUpstream": true,
        "clearProfile": false
    })
    .to_string();
    let (status, created) = daemon.post(&format!("/v1/runs/{run_id}/relock"), &request);
    assert_eq!(status, 200, "{created}");
    let derived = created["runId"].as_str().unwrap();
    let finished = daemon.await_terminal(derived);
    assert_eq!(finished["state"], "completed", "{finished}");
    assert_eq!(finished["relockedFrom"], run_id);
    let (_, list) = daemon.get("/v1/runs");
    assert!(
        list["runs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|run| run["runId"] == derived && run["relockedFrom"] == run_id)
    );
    let effective = std::fs::read_to_string(daemon.runs.join(derived).join("run.toml")).unwrap();
    assert!(effective.contains("freeze_upstream"));
    let (status, summary) = daemon.get(&format!("/v1/runs/{derived}/solution/summary"));
    assert_eq!(status, 200, "{summary}");
    assert_eq!(summary["profile_config"]["lock"][0]["confidence"], 0.5);
    let (status, error) = daemon.post("/v1/runs/missing/relock", &request);
    assert_eq!(status, 404, "{error}");
    let bad = serde_json::json!({"profileToml": "[profile]\nfreeze_upstream = { solution = '/host/secret' }\n"}).to_string();
    let (status, error) = daemon.post(&format!("/v1/runs/{run_id}/relock"), &bad);
    assert_eq!(status, 400, "{error}");
    assert_eq!(error["code"], "config-not-self-contained");
}

/// Cancelling reaches the real solver on every platform: it stops at an
/// evaluation boundary and records a resumable `canceled` run, rather than
/// the request succeeding while the solve carries on.
#[test]
fn cancelling_a_running_postflop_solve_stops_it_resumably() {
    let home = tempfile::tempdir().expect("temp home");
    let daemon = Daemon::start_with_solver(home.path(), 1, &real_solver());
    let config = r#"
schema = "solvers.nlh/v1"
[table]
players = 2
stack_bb = 90
ante_bb = 9
[spot]
line = "BTN c, BB x / BB x, BTN x / BB x, BTN x"
board = "2c 7d 9h Js Qs"
[ranges]
BB = "AsAh"
BTN = "KsKh"
[tree]
script = '''river when unopened { force bet [50] }'''
[solver]
storage = "f32"
[solver.stop]
max_iterations = 1000000000
check_every = 1000
[run]
threads = 1
"#;
    let body = serde_json::json!({ "configToml": config }).to_string();
    let (status, created) = daemon.post("/v1/runs", &body);
    assert_eq!(status, 200, "{created}");
    let run_id = created["runId"].as_str().unwrap();

    // A first progress row means the solver is past start-up and its
    // interrupt handler is installed.
    let mut started = false;
    for _ in 0..400 {
        let (_, run) = daemon.get(&format!("/v1/runs/{run_id}"));
        assert!(
            run["state"] == "queued" || run["state"] == "running",
            "{run}"
        );
        if run["lastProgress"].is_object() {
            started = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    assert!(started, "the solve never reported progress");

    let (status, cancelled) = daemon.post(&format!("/v1/runs/{run_id}/cancel"), "");
    assert_eq!(status, 200, "{cancelled}");
    let finished = daemon.await_terminal(run_id);
    assert_eq!(finished["state"], "canceled", "{finished}");
    assert_eq!(finished["resumable"], true, "{finished}");
    assert!(finished["progress"].as_u64().unwrap() < 1_000_000_000);
}

#[test]
fn node_selectors_are_rejected_for_multiway_artifacts_before_export() {
    let daemon = Daemon::start(1);
    let (_, created) = daemon.post("/v1/runs", &config("multiway-node"));
    let run_id = created["runId"].as_str().unwrap();
    daemon.await_terminal(run_id);
    // No valid artifact is needed: the daemon rejects the P1-only option
    // before invoking export.
    std::fs::write(
        daemon.runs.join(run_id).join("solution.mwsol"),
        b"placeholder",
    )
    .unwrap();
    for path in [
        "tree?node=root",
        "tree?resolve=rivers",
        "tree?restOfStreet=true",
        "decision",
    ] {
        let (status, error) = daemon.get(&format!("/v1/runs/{run_id}/solution/{path}"));
        assert_eq!(status, 400, "{path}: {error}");
        assert_eq!(error["code"], "invalid-request");
    }
}

/// `resolve=rivers` lets the views answer river nodes a no-rivers run did
/// not store; without it they stay an export failure.
#[test]
fn postflop_solution_views_resolve_unstored_rivers_on_request() {
    let home = tempfile::tempdir().expect("temp home");
    let daemon = Daemon::start_with_solver(home.path(), 1, &real_solver());
    let config = r#"
schema = "solvers.nlh/v1"
[table]
players = 2
stack_bb = 90
ante_bb = 9
[spot]
line = "BTN c, BB x / BB x, BTN x"
board = "2c 7d 9h Js"
[ranges]
BB = "AsAh"
BTN = "KsKh"
[tree]
script = '''turn, river { replace bet [50] replace raise [] }'''
[solver]
storage = "f32"
[solver.stop]
max_iterations = 2
check_every = 1
[run]
threads = 1
[output]
solution_streets = "no-rivers"
"#;
    let body = serde_json::json!({ "configToml": config }).to_string();
    let (status, created) = daemon.post("/v1/runs", &body);
    assert_eq!(status, 200, "{created}");
    let run_id = created["runId"].as_str().unwrap();
    let finished = daemon.await_terminal(run_id);
    assert_eq!(finished["state"], "completed", "{finished}");

    let base = format!("/v1/runs/{run_id}/solution");
    let (status, chance) = daemon.get(&format!("{base}/tree?node=xx"));
    assert_eq!(status, 200, "{chance}");
    let river = chance[0]["children"][0].as_str().unwrap();
    // The first dealt river card, by label path.
    let selector = format!("check/check/{}", chance[0]["actions"][0].as_str().unwrap());
    let (status, row) = daemon.get(&format!("{base}/tree?node={selector}"));
    assert_eq!(status, 200, "{row}");
    assert_eq!(row[0]["history"], river);
    assert_eq!(row[0]["stored"], false);

    let (status, error) = daemon.get(&format!("{base}/strategy?node={selector}"));
    assert_eq!(status, 500, "{error}");
    assert!(
        error["message"].as_str().unwrap().contains("no-rivers"),
        "{error}"
    );
    let (status, decision) = daemon.get(&format!("{base}/decision?node={selector}&resolve=rivers"));
    assert_eq!(status, 200, "{decision}");
    let rows = decision["strategy"].as_array().expect("strategy rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["history"], river);
    assert_eq!(rows[0]["combo"], "AsAh");
    assert_eq!(
        decision["actions"].as_array().expect("action rows").len(),
        row[0]["actions"].as_array().unwrap().len()
    );
    let seats: Vec<_> = decision["ev"]
        .as_array()
        .expect("ev rows")
        .iter()
        .map(|value| {
            (
                value["seat"].as_str().unwrap(),
                value["combo"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(seats, [("oop", "AsAh"), ("ip", "KsKh")]);

    // restOfStreet: one re-solve also answers every river action node below.
    let (status, street) = daemon.get(&format!("{base}/tree?node={selector}&restOfStreet=true"));
    assert_eq!(status, 200, "{street}");
    let histories: Vec<&str> = street
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["history"].as_str().unwrap())
        .collect();
    assert!(histories.len() > 1, "{street}");
    let (status, subgame) = daemon.get(&format!(
        "{base}/decision?node={selector}&restOfStreet=true&resolve=rivers"
    ));
    assert_eq!(status, 200, "{subgame}");
    let mut covered: Vec<&str> = subgame["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["history"].as_str().unwrap())
        .collect();
    covered.dedup();
    assert_eq!(covered, histories);

    for query in ["resolve=turns", "restOfStreet=yes"] {
        let (status, error) = daemon.get(&format!("{base}/strategy?node={selector}&{query}"));
        assert_eq!(status, 400, "{query}: {error}");
        assert_eq!(error["code"], "invalid-request");
    }
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
