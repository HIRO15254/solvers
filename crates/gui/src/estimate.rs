//! Background dense-arena size estimate for the Setup tab: wraps
//! `multiway::estimate_dense_arena` on its own thread -- tree enumeration on
//! a huge full-tree config can take a few seconds, so this must never block
//! the UI thread -- using the same debounced `std::thread` + `mpsc` pattern
//! `results.rs`'s "Evaluate EVs" uses for its own background evaluation.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use multiway::DenseArenaEstimate;

use crate::model::Model;

/// Minimum wall-clock gap between two dispatched estimate computations, even
/// if the model keeps changing every frame.
const DEBOUNCE: Duration = Duration::from_secs(1);

/// `run.max_memory_bytes = None` mirror of `cli::session::DEFAULT_MEMORY_LIMIT`
/// (private to that module): the engine's dense-arena budget when Advanced
/// mode leaves `run.max_memory_mib` at its `0` ("unset") sentinel.
const ENGINE_DEFAULT_MEMORY_LIMIT_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// Reply from one background estimate computation, tagged with the model
/// hash it answers so a stale reply for a since-edited model can be dropped.
type EstimateReply = (u64, Result<DenseArenaEstimate, String>);

/// Debounced, background-threaded `estimate_dense_arena` runner for the
/// Setup tab's tree/memory estimate panel (Auto: inside the derived-settings
/// summary; Advanced: its own collapsible section).
#[derive(Default)]
pub struct EstimatePanelState {
    last_hash: Option<u64>,
    last_dispatch: Option<Instant>,
    pending_hash: Option<u64>,
    receiver: Option<Receiver<EstimateReply>>,
    /// Most recent reply, tagged with the model hash it answers -- render
    /// code compares this against the *current* model hash to tell a fresh
    /// result from a stale one still on screen while a new one computes.
    pub result: Option<EstimateReply>,
}

impl EstimatePanelState {
    pub fn new() -> Self {
        Self::default()
    }

    /// `true` while a background computation is in flight for a model the
    /// caller hasn't seen a reply for yet -- drives the panel's spinner.
    pub fn is_pending(&self) -> bool {
        self.pending_hash.is_some()
    }

    /// Call once per frame: polls any in-flight computation, then dispatches
    /// a fresh one when `model` changed since the last dispatch (debounced
    /// to at most once per second).
    pub fn update(&mut self, model: &Model) {
        self.poll();

        let Ok(toml_text) = crate::model::model_to_toml(model) else {
            // Mid-edit invalid config (e.g. an unparsable range): nothing to
            // hash or estimate against. Leave whatever result is already on
            // screen rather than clearing it out from under the user.
            return;
        };
        let hash = hash_str(&toml_text);
        if self.pending_hash == Some(hash) || self.last_hash == Some(hash) {
            return;
        }
        if let Some(last_dispatch) = self.last_dispatch
            && last_dispatch.elapsed() < DEBOUNCE
        {
            return;
        }
        self.dispatch(model, hash);
    }

    fn dispatch(&mut self, model: &Model, hash: u64) {
        let config = match crate::model::model_to_solve_config(model) {
            Ok(config) => config,
            Err(error) => {
                self.result = Some((hash, Err(error)));
                self.last_hash = Some(hash);
                self.last_dispatch = Some(Instant::now());
                return;
            }
        };
        let cli::config::GameSection::PreflopMultiway(game_config) = config.game else {
            unreachable!("model_to_solve_config always emits PreflopMultiway")
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result =
                multiway::estimate_dense_arena(&game_config).map_err(|error| error.to_string());
            let _ = sender.send((hash, result));
        });
        self.receiver = Some(receiver);
        self.pending_hash = Some(hash);
        self.last_dispatch = Some(Instant::now());
    }

    fn poll(&mut self) {
        let Some(receiver) = self.receiver.as_ref() else {
            return;
        };
        match receiver.try_recv() {
            Ok((hash, result)) => {
                self.result = Some((hash, result));
                self.last_hash = Some(hash);
                self.pending_hash = None;
                self.receiver = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.pending_hash = None;
                self.receiver = None;
            }
        }
    }
}

fn hash_str(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// Reply from one background `derive_auto_run` preview, tagged with the
/// model+machine-facts hash it answers.
type AutoPreviewReply = (u64, Result<cli::auto_run::AutoRunDerivation, String>);

/// Debounced, background-threaded `cli::auto_run::derive_auto_run` preview
/// for Auto mode's derived-settings summary panel. Same shape as
/// [`EstimatePanelState`], but the background computation is the bucket-
/// ladder search (itself up to seven `estimate_dense_arena` calls) rather
/// than a single estimate, and the hash also covers the caller-supplied
/// thread count/memory budget.
#[derive(Default)]
pub struct AutoPreviewState {
    last_hash: Option<u64>,
    last_dispatch: Option<Instant>,
    pending_hash: Option<u64>,
    receiver: Option<Receiver<AutoPreviewReply>>,
    pub result: Option<AutoPreviewReply>,
}

impl AutoPreviewState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_pending(&self) -> bool {
        self.pending_hash.is_some()
    }

    /// Call once per frame: polls any in-flight computation, then dispatches
    /// a fresh one when `model`/`threads`/`memory_budget_bytes` changed
    /// since the last dispatch (debounced to at most once per second).
    pub fn update(&mut self, model: &Model, threads: usize, memory_budget_bytes: u64) {
        self.poll();

        let Ok(toml_text) = crate::model::model_to_toml(model) else {
            return;
        };
        let hash = hash_str(&format!(
            "{toml_text}\u{0}{threads}\u{0}{memory_budget_bytes}"
        ));
        if self.pending_hash == Some(hash) || self.last_hash == Some(hash) {
            return;
        }
        if let Some(last_dispatch) = self.last_dispatch
            && last_dispatch.elapsed() < DEBOUNCE
        {
            return;
        }
        self.dispatch(model, threads, memory_budget_bytes, hash);
    }

    fn dispatch(&mut self, model: &Model, threads: usize, memory_budget_bytes: u64, hash: u64) {
        let config = match crate::model::model_to_solve_config(model) {
            Ok(config) => config,
            Err(error) => {
                self.result = Some((hash, Err(error)));
                self.last_hash = Some(hash);
                self.last_dispatch = Some(Instant::now());
                return;
            }
        };
        let cli::config::GameSection::PreflopMultiway(game_config) = config.game else {
            unreachable!("model_to_solve_config always emits PreflopMultiway")
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = cli::auto_run::derive_auto_run(&game_config, threads, memory_budget_bytes)
                .map_err(|error| error.to_string());
            let _ = sender.send((hash, result));
        });
        self.receiver = Some(receiver);
        self.pending_hash = Some(hash);
        self.last_dispatch = Some(Instant::now());
    }

    fn poll(&mut self) {
        let Some(receiver) = self.receiver.as_ref() else {
            return;
        };
        match receiver.try_recv() {
            Ok((hash, result)) => {
                self.result = Some((hash, result));
                self.last_hash = Some(hash);
                self.pending_hash = None;
                self.receiver = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.pending_hash = None;
                self.receiver = None;
            }
        }
    }
}

/// Resolves `run.max_memory_mib` (`0` = unset) into the actual dense-arena
/// byte budget the engine would use -- Advanced mode's estimate-panel
/// comparison target.
pub fn resolve_budget_bytes(max_memory_mib: u64) -> u64 {
    if max_memory_mib == 0 {
        ENGINE_DEFAULT_MEMORY_LIMIT_BYTES
    } else {
        max_memory_mib * 1024 * 1024
    }
}

/// `true` when an estimated arena size exceeds the memory budget it is being
/// compared against. Pulled out as its own pure function so the comparison
/// is unit-testable without the background-thread plumbing around it.
pub fn over_budget(estimated_bytes: u64, budget_bytes: u64) -> bool {
    estimated_bytes > budget_bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_budget_bytes_falls_back_to_the_engine_default_when_unset() {
        assert_eq!(resolve_budget_bytes(0), 4 * 1024 * 1024 * 1024);
        assert_eq!(resolve_budget_bytes(512), 512 * 1024 * 1024);
    }

    #[test]
    fn over_budget_flags_only_when_the_estimate_exceeds_the_budget() {
        assert!(!over_budget(100, 200));
        assert!(!over_budget(200, 200));
        assert!(over_budget(201, 200));
    }

    #[test]
    fn hash_str_is_deterministic_and_change_sensitive() {
        assert_eq!(hash_str("a"), hash_str("a"));
        assert_ne!(hash_str("a"), hash_str("b"));
    }
}
