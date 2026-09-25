//! Versioned progress and result summaries for sampled multiway solves.
//!
//! The heads-up [`crate::MetricsRow`] contract remains frozen.  Multiway
//! games are sampled, general-sum profiles, so their diagnostics are arrays
//! and intentionally avoid the `exploitability` / `nash_conv` vocabulary.

use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const MULTIWAY_SCHEMA_VERSION: u16 = 3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Estimate {
    pub mean: f64,
    /// Standard error of the reported point-estimate candidate. For a
    /// maximum across deviation candidates, this alone does not reconstruct
    /// the selection-adjusted interval; consumers must read `ci95` directly.
    pub stderr: f64,
    /// Approximate sampling interval, including a simultaneous-candidate
    /// correction when the metric selects the largest tested deviation.
    pub ci95: [f64; 2],
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiwayStreetVisitCounts {
    pub preflop: u64,
    pub flop: u64,
    pub turn: u64,
    pub river: u64,
}

/// Strategy sources on held-out baseline trajectories only. These are
/// decision-visit counts, not unique infosets or action-value accuracy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiwayPolicyCoverage {
    pub decision_visits: u64,
    pub stored_strategy_visits: u64,
    pub uniform_fallback_visits: u64,
    pub average_strategy_visits: u64,
    pub current_strategy_visits: u64,
    pub regret_fallback_visits: u64,
    pub decision_visits_by_street: MultiwayStreetVisitCounts,
    pub stored_strategy_visits_by_street: MultiwayStreetVisitCounts,
    pub uniform_fallback_visits_by_street: MultiwayStreetVisitCounts,
    pub average_strategy_visits_by_street: MultiwayStreetVisitCounts,
    pub current_strategy_visits_by_street: MultiwayStreetVisitCounts,
    pub regret_fallback_visits_by_street: MultiwayStreetVisitCounts,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiwaySeatMetrics {
    pub seat: u8,
    pub profile_ev: Option<Estimate>,
    pub average_positive_regret: f64,
    pub strategy_drift_l1: f64,
    pub deviation_gain_lower_bound: Option<Estimate>,
    /// None for older rows and rows without a held-out profile evaluation.
    #[serde(default)]
    pub candidate_policy_coverage: Option<MultiwayPolicyCoverage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiwayMetricsRow {
    pub schema_version: u16,
    pub phase: String,
    pub sweeps: u64,
    pub traversals: u64,
    pub elapsed_secs: f64,
    pub infosets: u64,
    pub memory_bytes: u64,
    pub traversals_per_second: f64,
    /// See `multiway::solver::SolverState::hand_updates`. Rows written before
    /// this field existed decode as `0`.
    #[serde(default)]
    pub hand_updates: u64,
    #[serde(default)]
    pub hand_updates_per_second: f64,
    pub seats: Vec<MultiwaySeatMetrics>,
}

impl MultiwayMetricsRow {
    pub fn sampling(num_seats: usize) -> Self {
        Self {
            schema_version: MULTIWAY_SCHEMA_VERSION,
            phase: "sampling".to_string(),
            sweeps: 0,
            traversals: 0,
            elapsed_secs: 0.0,
            infosets: 0,
            memory_bytes: 0,
            traversals_per_second: 0.0,
            hand_updates: 0,
            hand_updates_per_second: 0.0,
            seats: (0..num_seats)
                .map(|seat| MultiwaySeatMetrics {
                    seat: seat as u8,
                    profile_ev: None,
                    average_positive_regret: 0.0,
                    strategy_drift_l1: 0.0,
                    deviation_gain_lower_bound: None,
                    candidate_policy_coverage: None,
                })
                .collect(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent<'a> {
    sequence: u64,
    event: &'a str,
    #[serde(flatten)]
    row: &'a MultiwayMetricsRow,
}

pub struct MultiwayMetricsWriter {
    file: File,
    next_sequence: u64,
}

impl MultiwayMetricsWriter {
    pub fn create_or_append(path: &Path) -> io::Result<Self> {
        let next_sequence = if path.exists() {
            BufReader::new(File::open(path)?)
                .lines()
                .map_while(Result::ok)
                .filter_map(|line| serde_json::from_str::<serde_json::Value>(&line).ok())
                .filter_map(|event| event.get("sequence")?.as_u64())
                .max()
                .map_or(0, |last| last.saturating_add(1))
        } else {
            0
        };
        Ok(Self {
            file: OpenOptions::new().create(true).append(true).open(path)?,
            next_sequence,
        })
    }

    pub fn append(&mut self, row: &MultiwayMetricsRow) -> io::Result<()> {
        let event = ProgressEvent {
            sequence: self.next_sequence,
            event: &row.phase,
            row,
        };
        let mut line = serde_json::to_string(&event).map_err(io::Error::other)?;
        line.push('\n');
        self.file.write_all(line.as_bytes())?;
        self.file.flush()?;
        self.next_sequence = self.next_sequence.saturating_add(1);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_round_trips_without_heads_up_names() {
        let row = MultiwayMetricsRow::sampling(9);
        let json = serde_json::to_string(&row).unwrap();
        assert!(!json.contains("nashConv"));
        assert!(!json.contains("exploitability"));
        assert_eq!(
            serde_json::from_str::<MultiwayMetricsRow>(&json).unwrap(),
            row
        );
    }

    #[test]
    fn progress_rows_without_policy_coverage_remain_readable() {
        let row = MultiwayMetricsRow::sampling(3);
        let mut json = serde_json::to_value(&row).unwrap();
        for seat in json["seats"].as_array_mut().unwrap() {
            seat.as_object_mut()
                .unwrap()
                .remove("candidatePolicyCoverage");
        }
        let restored: MultiwayMetricsRow = serde_json::from_value(json).unwrap();
        assert_eq!(restored, row);
        assert!(
            restored
                .seats
                .iter()
                .all(|seat| seat.candidate_policy_coverage.is_none())
        );
    }

    #[test]
    fn progress_writer_assigns_monotonic_event_sequences() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("progress.jsonl");
        let row = MultiwayMetricsRow::sampling(3);
        {
            let mut writer = MultiwayMetricsWriter::create_or_append(&path).unwrap();
            writer.append(&row).unwrap();
            writer.append(&row).unwrap();
        }
        let lines: Vec<serde_json::Value> = std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines[0]["sequence"], 0);
        assert_eq!(lines[1]["sequence"], 1);
        assert_eq!(lines[0]["event"], "sampling");
    }
}
