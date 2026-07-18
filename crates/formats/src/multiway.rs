//! Versioned progress and result summaries for sampled multiway solves.
//!
//! The heads-up [`crate::MetricsRow`] contract remains frozen.  Multiway
//! games are sampled, general-sum profiles, so their diagnostics are arrays
//! and intentionally avoid the `exploitability` / `nash_conv` vocabulary.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const MULTIWAY_SCHEMA_VERSION: u16 = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Estimate {
    pub mean: f64,
    pub stderr: f64,
    pub ci95: [f64; 2],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiwaySeatMetrics {
    pub seat: u8,
    pub profile_ev: Option<Estimate>,
    pub average_positive_regret: f64,
    pub strategy_drift_l1: f64,
    pub deviation_gain_lower_bound: Option<Estimate>,
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
                })
                .collect(),
        }
    }
}

pub struct MultiwayMetricsWriter {
    file: File,
}

impl MultiwayMetricsWriter {
    pub fn create_or_append(path: &Path) -> io::Result<Self> {
        Ok(Self {
            file: OpenOptions::new().create(true).append(true).open(path)?,
        })
    }

    pub fn append(&mut self, row: &MultiwayMetricsRow) -> io::Result<()> {
        let mut line = serde_json::to_string(row).map_err(io::Error::other)?;
        line.push('\n');
        self.file.write_all(line.as_bytes())?;
        self.file.flush()
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
}
