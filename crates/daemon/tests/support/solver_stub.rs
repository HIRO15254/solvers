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
        "line" => line(&args[1..]),
        "solve" | "resume" => solve_or_resume(command, &args[1..]),
        "export" => export(&args[1..]),
        _ => {}
    }
}

fn export(args: &[std::ffi::OsString]) {
    let node = args
        .windows(2)
        .find(|pair| pair[0] == "--node")
        .map(|pair| format!("{:?}", pair[1].to_string_lossy()))
        .unwrap_or_else(|| "null".to_string());
    println!(r#"{{"node":{node}}}"#);
}

fn validate(args: &[std::ffi::OsString]) {
    let config = Path::new(args.first().expect("validate config"));
    let contents = std::fs::read_to_string(config).expect("read submitted config");
    if contents.contains("REJECT") {
        eprintln!("MWP999: the stub was asked to reject this config");
        std::process::exit(1);
    }
    let effective = args
        .windows(2)
        .find(|pair| pair[0] == "--write-effective")
        .map(|pair| Path::new(&pair[1]))
        .expect("--write-effective path");
    std::fs::copy(config, effective).expect("write effective config");
    println!(r#"{{"status":"valid","schema":"solvers.nlh/v1"}}"#);
}

fn line(args: &[std::ffi::OsString]) {
    let config = Path::new(args.first().expect("line config"));
    let contents = std::fs::read_to_string(config).expect("read submitted config");
    assert!(
        args.windows(2)
            .any(|pair| pair[0] == "--format" && pair[1] == "json")
    );
    if contents.contains("REJECT") {
        eprintln!("NLH004: illegal move at index 1");
        std::process::exit(2);
    }
    let moves = args
        .windows(2)
        .find(|pair| pair[0] == "--moves")
        .map(|pair| pair[1].to_str().expect("UTF-8 moves"));
    // Echo argument distinctions so socket tests cover missing and empty prefixes.
    let moves_json = match moves {
        None => "null".to_string(),
        Some(moves) => format!("\"{moves}\""),
    };
    println!(
        r#"{{"moves":[],"line":"","actions":[],"street":"preflop","pot":1.5,"complete":false,"boardCards":null,"players":2,"next":null,"terminal":false,"receivedMoves":{moves_json}}}"#
    );
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
    let config = std::fs::read_to_string(directory.join("run.toml")).unwrap_or_default();
    if config.contains("EARLY_EXIT") {
        std::process::exit(1);
    }
    if command == "resume" {
        std::fs::write(directory.join("resumed"), b"").expect("write resume marker");
    }
    let run_id = directory
        .file_name()
        .expect("run directory name")
        .to_string_lossy();
    let running = format!(
        concat!(
            r#"{{"schemaVersion":1,"runId":"{}","state":"running","gameKind":"hu-postflop","#,
            r#""configSchema":"solvers.nlh/v1","configHash":"aa","cliVersion":"stub","#,
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

    if config.contains("BLOCK") {
        std::thread::sleep(Duration::from_secs(30));
    }
    if config.contains("WAIT_RELEASE") {
        while !directory.join("release").exists() {
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    // Like the real CLI (`RunDir::finish`), progress and the terminal event
    // land before the terminal manifest, so a reader that sees `completed`
    // also sees the final progress row and the terminal event.
    std::fs::write(
        directory.join("progress.jsonl"),
        "{\"iteration\":42,\"elapsed_secs\":0.5}\n",
    )
    .expect("write progress");
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
    let completed = format!(
        concat!(
            r#"{{"schemaVersion":1,"runId":"{}","state":"completed","gameKind":"hu-postflop","#,
            r#""configSchema":"solvers.nlh/v1","configHash":"aa","cliVersion":"stub","#,
            r#""command":["solve"],"pid":1,"createdUnixMs":{},"startedUnixMs":{},"#,
            r#""finishedUnixMs":{},"failure":null,"completion":"max-iterations"}}"#,
        ),
        run_id, NOW, NOW, NOW,
    );
    std::fs::write(directory.join("manifest.json"), completed).expect("write completed manifest");
}
