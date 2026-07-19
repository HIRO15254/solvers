//! Directory loader: walks a corpus of hand-history and summary `.txt`
//! files and merges them into one [`Tournament`] per tournament id.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::hand_history::parse_hand_history;
use crate::model::{DateTime, GameType, Hand, ParseError, TournamentSummary};
use crate::summary::parse_summary;

/// One tournament's merged data: its summary (if a summary file was
/// loaded), plus every hand played (deduplicated by hand id, sorted
/// ascending by `(played_at, id)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tournament {
    pub id: u64,
    pub name: String,
    pub game: GameType,
    pub summary: Option<TournamentSummary>,
    pub hands: Vec<Hand>,
}

/// Every tournament found while loading a directory or file list, sorted
/// by earliest activity (the summary's start time or the first hand's
/// timestamp, whichever is earlier), then by id.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TournamentSet {
    pub tournaments: Vec<Tournament>,
}

/// Errors from [`load_dir`] / [`load_files`]: I/O failures, parse failures
/// (wrapping [`ParseError`] with the offending path), and cross-file
/// merge inconsistencies.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("I/O error reading {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse {}: {source}", path.display())]
    Parse {
        path: PathBuf,
        #[source]
        source: ParseError,
    },
    #[error(
        "{}: not a recognized hand-history or summary file (first non-empty line: {first_line:?})",
        path.display()
    )]
    UnknownFileKind { path: PathBuf, first_line: String },
    #[error("{}: file is empty", path.display())]
    EmptyFile { path: PathBuf },
    #[error(
        "tournament #{id}: duplicate summary file (a summary was already loaded; found another at {})",
        path.display()
    )]
    DuplicateSummary { id: u64, path: PathBuf },
    #[error(
        "tournament #{id}: game type conflict (already {existing:?}, but {} says {found:?})",
        path.display()
    )]
    GameTypeConflict {
        id: u64,
        existing: GameType,
        found: GameType,
        path: PathBuf,
    },
}

enum FileKind {
    HandHistory,
    Summary,
}

fn sniff_kind(path: &Path, text: &str) -> Result<FileKind, LoadError> {
    match text.lines().find(|line| !line.trim().is_empty()) {
        Some(line) if line.starts_with("Poker Hand #") => Ok(FileKind::HandHistory),
        Some(line) if line.starts_with("Tournament #") => Ok(FileKind::Summary),
        Some(line) => Err(LoadError::UnknownFileKind {
            path: path.to_path_buf(),
            first_line: line.to_string(),
        }),
        None => Err(LoadError::EmptyFile {
            path: path.to_path_buf(),
        }),
    }
}

/// One tournament's data while it's still being assembled from possibly
/// several files.
struct Building {
    name: String,
    game: GameType,
    summary: Option<TournamentSummary>,
    hands: HashMap<u64, Hand>,
}

/// Loads an explicit list of `.txt` files (hand-history and/or summary,
/// mixed freely) and merges them by tournament id.
pub fn load_files<I: IntoIterator<Item = PathBuf>>(files: I) -> Result<TournamentSet, LoadError> {
    let mut building: HashMap<u64, Building> = HashMap::new();

    for path in files {
        let text = std::fs::read_to_string(&path).map_err(|source| LoadError::Io {
            path: path.clone(),
            source,
        })?;
        match sniff_kind(&path, &text)? {
            FileKind::HandHistory => {
                let hands = parse_hand_history(&text).map_err(|source| LoadError::Parse {
                    path: path.clone(),
                    source,
                })?;
                for hand in hands {
                    let tournament_id = hand.tournament_id;
                    match building.get_mut(&tournament_id) {
                        Some(entry) => {
                            if entry.game != hand.game {
                                return Err(LoadError::GameTypeConflict {
                                    id: tournament_id,
                                    existing: entry.game,
                                    found: hand.game,
                                    path: path.clone(),
                                });
                            }
                            entry.hands.entry(hand.id).or_insert(hand);
                        }
                        None => {
                            let mut hands = HashMap::new();
                            let name = hand.tournament_name.clone();
                            let game = hand.game;
                            hands.insert(hand.id, hand);
                            building.insert(
                                tournament_id,
                                Building {
                                    name,
                                    game,
                                    summary: None,
                                    hands,
                                },
                            );
                        }
                    }
                }
            }
            FileKind::Summary => {
                let summary = parse_summary(&text).map_err(|source| LoadError::Parse {
                    path: path.clone(),
                    source,
                })?;
                match building.get_mut(&summary.id) {
                    Some(entry) => {
                        if entry.summary.is_some() {
                            return Err(LoadError::DuplicateSummary {
                                id: summary.id,
                                path: path.clone(),
                            });
                        }
                        if entry.game != summary.game {
                            return Err(LoadError::GameTypeConflict {
                                id: summary.id,
                                existing: entry.game,
                                found: summary.game,
                                path: path.clone(),
                            });
                        }
                        entry.name = summary.name.clone();
                        entry.summary = Some(summary);
                    }
                    None => {
                        building.insert(
                            summary.id,
                            Building {
                                name: summary.name.clone(),
                                game: summary.game,
                                summary: Some(summary),
                                hands: HashMap::new(),
                            },
                        );
                    }
                }
            }
        }
    }

    let mut tournaments: Vec<Tournament> = building
        .into_iter()
        .map(|(id, entry)| {
            let mut hands: Vec<Hand> = entry.hands.into_values().collect();
            hands.sort_by_key(|a| (a.played_at, a.id));
            Tournament {
                id,
                name: entry.name,
                game: entry.game,
                summary: entry.summary,
                hands,
            }
        })
        .collect();

    tournaments.sort_by_key(|a| (earliest_activity(a), a.id));

    Ok(TournamentSet { tournaments })
}

/// The earliest known timestamp for a tournament: its summary's start
/// time, or its first (chronologically earliest) hand's timestamp,
/// whichever is earlier. At least one of the two is always present.
fn earliest_activity(t: &Tournament) -> DateTime {
    match (&t.summary, t.hands.first()) {
        (Some(summary), Some(hand)) => summary.started_at.min(hand.played_at),
        (Some(summary), None) => summary.started_at,
        (None, Some(hand)) => hand.played_at,
        (None, None) => unreachable!("a Tournament is only created from a hand or a summary"),
    }
}

/// Recursively walks `path` (std only), loading every `*.txt` file found
/// (hand-history and summary files may be mixed across subdirectories).
pub fn load_dir(path: &Path) -> Result<TournamentSet, LoadError> {
    let mut files = Vec::new();
    collect_txt_files(path, &mut files)?;
    files.sort();
    load_files(files)
}

fn collect_txt_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), LoadError> {
    let entries = std::fs::read_dir(dir).map_err(|source| LoadError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| LoadError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_txt_files(&path, out)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("txt") {
            out.push(path);
        }
    }
    Ok(())
}
