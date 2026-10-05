//! The machine-scoped artifact cache.
//!
//! Abstraction tables are expensive to build (~107 s measured, of which the
//! EHS² sweep is ~102 s) and large (357 MB for the 3-max smoke's table, 543 MB
//! for one built earlier at other bucket counts), but they depend only on the
//! abstraction parameters -- not on the run, and not on the config file that
//! asked for them. So they live once per machine, outside any run directory,
//! and their location is never written into a config: a config that names a
//! local path cannot be sent to another host (see `docs/app-architecture.md`
//! R9 and R10).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Set once from `--cache-dir`, before anything resolves a cache path.
static OVERRIDE: OnceLock<PathBuf> = OnceLock::new();

/// Records the `--cache-dir` value for the rest of the process.
///
/// Later calls are ignored rather than panicking: the flag is read once in
/// `main`, and a second caller would be a bug that should not take down a
/// running solve.
pub fn set_root_override(root: &Path) {
    let _ = OVERRIDE.set(root.to_path_buf());
}

/// The cache root: `--cache-dir`, else `SOLVERS_CACHE_DIR`, else the
/// platform's per-user cache directory.
///
/// Returns `None` when no per-user directory can be determined (no `HOME`,
/// say), in which case callers build without caching rather than guessing a
/// location and writing hundreds of megabytes somewhere unexpected.
pub fn root() -> Option<PathBuf> {
    if let Some(explicit) = OVERRIDE.get() {
        return Some(explicit.clone());
    }
    if let Some(from_env) = std::env::var_os("SOLVERS_CACHE_DIR") {
        return Some(PathBuf::from(from_env));
    }
    platform_cache_dir().map(|base| base.join("solvers"))
}

#[cfg(target_os = "macos")]
fn platform_cache_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Caches"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_cache_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
}

#[cfg(windows)]
fn platform_cache_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
}
