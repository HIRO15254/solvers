//! Board-list parsing and CSV rendering for P1 reports.
use anyhow::{Context, Result, anyhow};
use std::path::Path;
pub fn run(
    config_path: &Path,
    boards_arg: Option<&str>,
    boards_file: Option<&Path>,
    output: Option<&Path>,
) -> Result<()> {
    let raw = std::fs::read_to_string(config_path)
        .with_context(|| format!("reading {}", config_path.display()))?;
    hu_postflop::prepare::prepare(&raw, config_path)?;
    let boards: Vec<_> = collect_board_tokens(boards_arg, boards_file)?
        .iter()
        .map(|t| normalize_board_token(t))
        .collect();
    let rows = hu_postflop::report::compute(
        &raw,
        config_path,
        &boards,
        &crate::CLI_CANCEL,
        &mut |warning| eprintln!("warning: {warning}"),
    )?;
    let mut labels = Vec::new();
    for row in &rows {
        for label in &row.labels {
            if !labels.contains(label) {
                labels.push(label.clone());
            }
        }
    }
    let mut header = vec![
        "board".into(),
        "iterations".into(),
        "wall_s".into(),
        "nash_conv".into(),
        "ev_oop".into(),
        "ev_ip".into(),
        "oop_equity".into(),
    ];
    header.extend(
        labels
            .iter()
            .map(|label| format!("freq_{}", label.replace(' ', "_"))),
    );
    let mut out = header.join(",") + "\n";
    for row in rows {
        let own_labels = row.labels;
        let freqs = row.frequencies;
        let mut prefix = vec![
            row.board,
            row.iterations.to_string(),
            fmt_sig(row.wall_secs, 6),
            fmt_sig(row.nash_conv, 6),
            fmt_sig(row.ev[0], 6),
            fmt_sig(row.ev[1], 6),
            fmt_sig(row.oop_equity, 6),
        ];
        prefix.extend(labels.iter().map(|label| {
            own_labels
                .iter()
                .position(|l| l == label)
                .map(|i| fmt_sig(freqs[i], 6))
                .unwrap_or_default()
        }));
        out.push_str(&(prefix.join(",") + "\n"));
    }
    if let Some(path) = output {
        std::fs::write(path, out)?;
    } else {
        print!("{out}");
    }
    Ok(())
}

fn collect_board_tokens(
    boards_arg: Option<&str>,
    boards_file: Option<&Path>,
) -> Result<Vec<String>> {
    match (boards_arg, boards_file) {
        (Some(_), Some(_)) => Err(anyhow!("pass exactly one of --boards or --boards-file")),
        (None, None) => Err(anyhow!("pass exactly one of --boards or --boards-file")),
        (Some(s), None) => Ok(s
            .split(',')
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect()),
        (None, Some(path)) => {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?;
            Ok(text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(str::to_string)
                .collect())
        }
    }
}

fn normalize_board_token(token: &str) -> String {
    if !token.is_empty() && !token.contains(char::is_whitespace) && token.len().is_multiple_of(2) {
        token
            .as_bytes()
            .chunks(2)
            .map(|c| std::str::from_utf8(c).unwrap_or(""))
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        token.to_string()
    }
}

fn fmt_sig(x: f64, sig: usize) -> String {
    if x == 0.0 {
        return "0".to_string();
    }
    let d = sig as i32 - 1 - x.abs().log10().floor() as i32;
    let d = d.max(0) as usize;
    format!("{:.*}", d, x)
}
