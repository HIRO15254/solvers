//! Machine-cache file naming for EHS² tables.
use super::{CACHE_FORMAT_VERSION, Ehs2Params};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Cache file for one EHS² table, or `None` when there is no cache root.
///
/// The name carries everything the table's content depends on, so tables for
/// different bucket counts coexist instead of overwriting each other. A
/// fixed name would make a K=128 run and a K=256 run rebuild in turn,
/// forever.
pub(crate) fn cache_path(root: Option<&Path>, params: Ehs2Params) -> Result<Option<PathBuf>> {
    named(
        root,
        "ehs2",
        &format!(
            "v{}-f{}-t{}-r{}.postcard",
            CACHE_FORMAT_VERSION, params.flop_buckets, params.turn_buckets, params.river_buckets
        ),
    )
}

fn named(root: Option<&Path>, directory: &str, file: &str) -> Result<Option<PathBuf>> {
    let Some(root) = root else {
        return Ok(None);
    };
    let directory = root.join(directory);
    std::fs::create_dir_all(&directory)
        .with_context(|| format!("creating the cache directory {}", directory.display()))?;
    Ok(Some(directory.join(file)))
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
        let path = cache_path(Some(directory.path()), params())
            .unwrap()
            .expect("a root was set");
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.contains("f128"), "{name}");
        assert!(name.contains("t128"), "{name}");
        assert!(name.contains("r256"), "{name}");
        assert!(name.contains(&format!("v{CACHE_FORMAT_VERSION}")), "{name}");
        assert!(path.parent().unwrap().is_dir(), "the directory is created");

        let other = cache_path(
            Some(directory.path()),
            Ehs2Params {
                river_buckets: 128,
                ..params()
            },
        )
        .unwrap()
        .unwrap();
        assert_ne!(path, other);
    }
}
