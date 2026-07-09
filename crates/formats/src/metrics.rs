//! JSONL metrics: one `MetricsRow` object per line, appended and flushed
//! immediately so a killed run's file is always valid up to its last
//! completed row.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

/// One row of solver progress, written at every exploitability check.
/// Deliberately flat (no nested game-specific fields) so
/// `tools/plot_convergence.py` can read any run's file without knowing
/// what game or algorithm produced it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MetricsRow {
    pub iteration: u64,
    pub elapsed_secs: f64,
    pub expl_p0: f64,
    pub expl_p1: f64,
    pub nash_conv: f64,
}

/// Appends one JSON object per line to a metrics file, flushing after
/// every row.
pub struct MetricsWriter {
    file: File,
}

impl MetricsWriter {
    /// Opens `path` for appending, creating it if it doesn't exist.
    /// Reused across `solve`/`resume`/`bench` runs against the same path:
    /// rows from an earlier (possibly killed) run stay in the file.
    pub fn create_or_append(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(MetricsWriter { file })
    }

    pub fn append(&mut self, row: &MetricsRow) -> io::Result<()> {
        let mut line = serde_json::to_string(row).map_err(io::Error::other)?;
        line.push('\n');
        self.file.write_all(line.as_bytes())?;
        self.file.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_path(name: &str) -> std::path::PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "formats-metrics-test-{}-{}-{}",
            std::process::id(),
            id,
            name
        ))
    }

    #[test]
    fn rows_round_trip_and_append_across_writers() {
        let path = temp_path("metrics.jsonl");
        let _ = std::fs::remove_file(&path);

        let rows = [
            MetricsRow {
                iteration: 100,
                elapsed_secs: 0.5,
                expl_p0: 0.1,
                expl_p1: 0.2,
                nash_conv: 0.3,
            },
            MetricsRow {
                iteration: 200,
                elapsed_secs: 1.0,
                expl_p0: 0.05,
                expl_p1: 0.05,
                nash_conv: 0.1,
            },
        ];
        {
            let mut w = MetricsWriter::create_or_append(&path).unwrap();
            w.append(&rows[0]).unwrap();
        }
        // Simulate a restarted run appending more rows to the same file.
        {
            let mut w = MetricsWriter::create_or_append(&path).unwrap();
            w.append(&rows[1]).unwrap();
        }

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);
        for (line, expected) in lines.iter().zip(rows.iter()) {
            let parsed: MetricsRow = serde_json::from_str(line).unwrap();
            assert_eq!(parsed, *expected);
        }
        let _ = std::fs::remove_file(&path);
    }
}
