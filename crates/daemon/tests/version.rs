use std::process::Command;

#[test]
fn version_includes_revision() {
    let prefix = format!("solversd {} (", env!("CARGO_PKG_VERSION"));
    for flag in ["--version", "-V"] {
        let output = Command::new(env!("CARGO_BIN_EXE_solversd"))
            .arg(flag)
            .output()
            .expect("run solversd version");
        assert!(
            output.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        let version = stdout.trim_end();
        assert!(version.starts_with(&prefix), "{version}");
        let hash = version
            .strip_prefix(&prefix)
            .unwrap()
            .strip_suffix(')')
            .expect("version ends after the git revision");
        assert!(
            hash == "unknown"
                || (hash.len() >= 8 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())),
            "invalid git revision: {hash}"
        );
        assert!(output.stderr.is_empty());
    }
}
