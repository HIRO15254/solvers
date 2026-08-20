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

use abstraction::{CACHE_FORMAT_VERSION, Ehs2Params};
use anyhow::{Context, Result};

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

/// Cache file for one EHS² table, or `None` when there is no cache root.
///
/// The name carries everything the table's content depends on, so tables for
/// different bucket counts coexist instead of overwriting each other. A
/// fixed name would make a K=128 run and a K=256 run rebuild in turn,
/// forever.
pub fn ehs2_table(params: Ehs2Params) -> Result<Option<PathBuf>> {
    let Some(root) = root() else {
        return Ok(None);
    };
    let directory = root.join("ehs2");
    std::fs::create_dir_all(&directory).with_context(|| {
        format!(
            "creating the abstraction cache directory {}",
            directory.display()
        )
    })?;
    Ok(Some(directory.join(format!(
        "v{}-f{}-t{}-r{}.postcard",
        CACHE_FORMAT_VERSION, params.flop_buckets, params.turn_buckets, params.river_buckets
    ))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> Ehs2Params {
        Ehs2Params {
            flop_buckets: 128,
            turn_buckets: 128,
            river_buckets: 256,
        }
    }

    /// The file name must distinguish every table the bucket counts can
    /// produce, or two runs quietly invalidate each other's cache.
    #[test]
    fn the_file_name_carries_the_bucket_counts_and_format_version() {
        let directory = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("SOLVERS_CACHE_DIR", directory.path()) };
        let path = ehs2_table(params()).unwrap().expect("a root was set");
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.contains("f128"), "{name}");
        assert!(name.contains("t128"), "{name}");
        assert!(name.contains("r256"), "{name}");
        assert!(name.contains(&format!("v{CACHE_FORMAT_VERSION}")), "{name}");
        assert!(path.parent().unwrap().is_dir(), "the directory is created");

        let other = ehs2_table(Ehs2Params {
            river_buckets: 128,
            ..params()
        })
        .unwrap()
        .unwrap();
        assert_ne!(path, other);
        unsafe { std::env::remove_var("SOLVERS_CACHE_DIR") };
    }
}
