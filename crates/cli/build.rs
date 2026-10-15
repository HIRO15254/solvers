use std::path::Path;
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn watch_git_path(name: &str) {
    if let Some(path) = git(&["rev-parse", "--git-path", name]) {
        let mut path = Path::new(&path);
        // Packed refs have no loose file yet. Watch its nearest existing
        // directory rather than a missing file, which Cargo treats as dirty.
        while !path.exists() {
            let Some(parent) = path.parent() else {
                return;
            };
            path = parent;
        }
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

fn main() {
    // Disable Cargo's default whole-package scan even in source archives.
    println!("cargo:rerun-if-changed=build.rs");
    watch_git_path("HEAD");
    if let Some(reference) = git(&["symbolic-ref", "--quiet", "HEAD"]) {
        watch_git_path(&reference);
    }
    if let Some(path) = git(&["rev-parse", "--git-path", "packed-refs"])
        && Path::new(&path).is_file()
    {
        println!("cargo:rerun-if-changed={path}");
    }
    let hash = git(&["rev-parse", "--short=8", "HEAD"]).unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=SOLVERS_GIT_HASH={hash}");
}
