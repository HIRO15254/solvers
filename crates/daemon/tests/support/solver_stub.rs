use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

const NOW: u64 = 1_700_000_000_000;

fn main() {
    let mut args = std::env::args_os().skip(1).collect::<Vec<_>>();
    while args.first().is_some_and(|arg| arg == "--cache-dir") {
        args.drain(..2);
    }
    let Some(command) = args.first().and_then(|arg| arg.to_str()) else {
        return;
    };
    match command {
        "validate" => validate(&args[1..]),
        "solve" | "resume" => solve_or_resume(command, &args[1..]),
        _ => {}
    }
}

fn validate(args: &[std::ffi::OsString]) {
    let config = Path::new(args.first().expect("validate config"));
    let contents = std::fs::read_to_string(config).expect("read submitted config");
    if contents.contains("REJECT") {
        eprintln!("MWP999: the stub was asked to reject this config");
        std::process::exit(1);
    }
    if contents.contains("NEEDS_FILE") {
        eprintln!("reading mwtree source /nowhere/tree.mwtree");
        std::process::exit(1);
    }
    let effective = args
        .windows(2)
        .find(|pair| pair[0] == "--write-effective")
        .map(|pair| Path::new(&pair[1]))
        .expect("--write-effective path");
    std::fs::copy(config, effective).expect("write effective config");
    println!(r#"{{"status":"valid","schema":"solvers.toy/v1"}}"#);
}

fn solve_or_resume(command: &str, args: &[std::ffi::OsString]) {
    let directory = if command == "resume" {
        PathBuf::from(args.first().expect("resume directory"))
    } else {
        args.windows(2)
            .find(|pair| pair[0] == OsStr::new("--out"))
            .map(|pair| PathBuf::from(&pair[1]))
            .expect("--out directory")
    };
    let run_id = directory
        .file_name()
        .expect("run directory name")
        .to_string_lossy();
    let running = format!(
        concat!(
            r#"{{"schemaVersion":1,"runId":"{}","state":"running","gameKind":"kuhn","#,
            r#""configSchema":"solvers.toy/v1","configHash":"aa","cliVersion":"stub","#,
            r#""command":["solve"],"pid":{},"createdUnixMs":{},"startedUnixMs":{},"#,
            r#""finishedUnixMs":null,"failure":null,"completion":null}}"#,
        ),
        run_id,
        std::process::id(),
        NOW,
        NOW,
    );
    std::fs::write(directory.join("manifest.json"), running).expect("write running manifest");
    std::fs::write(
        directory.join("events.jsonl"),
        format!(
            "{{\"seq\":0,\"unixMs\":{NOW},\"level\":\"info\",\"kind\":\"state\",\"state\":\"running\"}}\n"
        ),
    )
    .expect("write running event");

    let config = std::fs::read_to_string(directory.join("run.toml")).unwrap_or_default();
    if config.contains("BLOCK") {
        std::thread::sleep(Duration::from_secs(30));
    }

    let completed = format!(
        concat!(
            r#"{{"schemaVersion":1,"runId":"{}","state":"completed","gameKind":"kuhn","#,
            r#""configSchema":"solvers.toy/v1","configHash":"aa","cliVersion":"stub","#,
            r#""command":["solve"],"pid":1,"createdUnixMs":{},"startedUnixMs":{},"#,
            r#""finishedUnixMs":{},"failure":null,"completion":"completed"}}"#,
        ),
        run_id, NOW, NOW, NOW,
    );
    std::fs::write(directory.join("manifest.json"), completed).expect("write completed manifest");
    let completed_event = format!(
        "{{\"seq\":1,\"unixMs\":{NOW},\"level\":\"info\",\"kind\":\"state\",\"state\":\"completed\"}}\n"
    );
    let mut events = std::fs::OpenOptions::new()
        .append(true)
        .open(directory.join("events.jsonl"))
        .expect("open events");
    use std::io::Write as _;
    events
        .write_all(completed_event.as_bytes())
        .expect("write completed event");
    std::fs::write(
        directory.join("progress.jsonl"),
        "{\"iteration\":42,\"elapsedSecs\":0.5}\n",
    )
    .expect("write progress");
}
