//! Preset storage: built-in TOML files embedded at compile time, plus a
//! user-configurable directory of `<name>.toml` files (see
//! `docs/native-gui-plan.md` section E). A preset is a complete
//! `kind = "preflop-multiway"` `SolveConfig` TOML -- the exact thing
//! `solvers solve --config` accepts.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// `(display name, embedded TOML text)`.
pub const BUILT_IN: &[(&str, &str)] = &[
    (
        "6-max cash",
        include_str!("../presets/multiway-6max-cash.toml"),
    ),
    (
        "9-max push/fold",
        include_str!("../presets/multiway-9max-pushfold.toml"),
    ),
    (
        "9-max MTT ICM",
        include_str!("../presets/multiway-9max-mtt-icm.toml"),
    ),
    (
        "9-max research",
        include_str!("../presets/multiway-9max-research.toml"),
    ),
];

/// Default user preset directory: `<exe dir>/presets`.
pub fn default_user_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("presets")
}

/// One entry in the merged preset list shown in the Setup tab's left panel.
#[derive(Clone, Debug)]
pub enum PresetEntry {
    BuiltIn {
        name: &'static str,
        toml: &'static str,
    },
    User {
        name: String,
        path: PathBuf,
    },
}

impl PresetEntry {
    pub fn display_name(&self) -> &str {
        match self {
            PresetEntry::BuiltIn { name, .. } => name,
            PresetEntry::User { name, .. } => name,
        }
    }

    pub fn is_user(&self) -> bool {
        matches!(self, PresetEntry::User { .. })
    }

    pub fn load(&self) -> Result<String> {
        match self {
            PresetEntry::BuiltIn { toml, .. } => Ok((*toml).to_string()),
            PresetEntry::User { path, .. } => std::fs::read_to_string(path)
                .with_context(|| format!("reading preset {}", path.display())),
        }
    }
}

/// Lists built-in presets followed by user presets (`*.toml` files,
/// alphabetized) found directly under `user_dir`. Missing `user_dir` is not
/// an error -- it just contributes no user presets.
pub fn list(user_dir: &Path) -> Vec<PresetEntry> {
    let mut entries: Vec<PresetEntry> = BUILT_IN
        .iter()
        .map(|&(name, toml)| PresetEntry::BuiltIn { name, toml })
        .collect();

    let mut user: Vec<(String, PathBuf)> = std::fs::read_dir(user_dir)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "toml"))
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_stem()?.to_string_lossy().into_owned();
            Some((name, path))
        })
        .collect();
    user.sort_by(|a, b| a.0.cmp(&b.0));
    entries.extend(
        user.into_iter()
            .map(|(name, path)| PresetEntry::User { name, path }),
    );
    entries
}

/// Saves `toml` as `<user_dir>/<name>.toml`, creating `user_dir` if needed.
/// Rejects names that would escape `user_dir` via path separators.
pub fn save(user_dir: &Path, name: &str, toml: &str) -> Result<PathBuf> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        bail!("preset name must not be empty");
    }
    if trimmed.contains(['/', '\\']) || trimmed == "." || trimmed == ".." {
        bail!("preset name must not contain path separators");
    }
    std::fs::create_dir_all(user_dir)
        .with_context(|| format!("creating preset directory {}", user_dir.display()))?;
    let path = user_dir.join(format!("{trimmed}.toml"));
    std::fs::write(&path, toml).with_context(|| format!("writing preset {}", path.display()))?;
    Ok(path)
}

pub fn delete(path: &Path) -> Result<()> {
    std::fs::remove_file(path).with_context(|| format!("deleting preset {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_presets_are_present_and_named() {
        assert_eq!(BUILT_IN.len(), 4);
        for &(name, toml) in BUILT_IN {
            assert!(!name.is_empty());
            assert!(toml.contains("kind = \"preflop-multiway\""));
        }
    }

    #[test]
    fn user_presets_are_listed_alphabetically_after_built_ins() {
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), "zeta", "kind = \"preflop-multiway\"").unwrap();
        save(dir.path(), "alpha", "kind = \"preflop-multiway\"").unwrap();
        let entries = list(dir.path());
        assert_eq!(entries.len(), BUILT_IN.len() + 2);
        assert_eq!(entries[BUILT_IN.len()].display_name(), "alpha");
        assert_eq!(entries[BUILT_IN.len() + 1].display_name(), "zeta");
        assert!(entries[BUILT_IN.len()].is_user());
    }

    #[test]
    fn save_rejects_path_separators() {
        let dir = tempfile::tempdir().unwrap();
        assert!(save(dir.path(), "../escape", "x").is_err());
        assert!(save(dir.path(), "a/b", "x").is_err());
    }
}
