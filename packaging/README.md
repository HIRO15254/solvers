# solvers — NLH HU Postflop Solver

English | [日本語](README.jp.md)

This archive is a prebuilt release of the **NLH HU Postflop Solver** (P1): it
solves No-Limit Hold'em spots where two players see the flop, turn or river,
exactly for the bet tree you give it (vector CFR, no card abstraction), and
reports strategies, per-hand EVs and exploitability.

The Multiway Preflop solver (P2) and `solvers derive` are **not** included in
this build; P2 inputs and artifacts are rejected with an explicit error.

Source, issues and full documentation: <https://github.com/HIRO15254/solvers>

## Contents

| File | Purpose |
|---|---|
| `solvers` / `solvers.exe` | command-line solver |
| `solversd` / `solversd.exe` | local job daemon and Web GUI server |
| `ui/` | the Web GUI, served by `solversd` |
| `start-gui.cmd` / `start-gui.sh` | start the daemon and open the GUI in your browser |
| `CHANGELOG.md`, `LICENSE-*`, `THIRD-PARTY-LICENSES.txt` | release notes and licenses |

## Choose and extract a download

Get this archive and `SHA256SUMS` from
<https://github.com/HIRO15254/solvers/releases>.
Names are `solvers-<version>-<target>[-compat]`: Windows
`x86_64-pc-windows-msvc.zip`, Linux `x86_64-unknown-linux-gnu.tar.gz`,
Apple Silicon `aarch64-apple-darwin.tar.gz`, or Intel macOS
`x86_64-apple-darwin.tar.gz` (the target above is preceded by the name/version).
Windows/Linux default archives require **x86-64-v3 (AVX2)**; choose the
`-compat.zip` / `-compat.tar.gz` archive without AVX2 (**x86-64-v2** is
still required). Intel macOS ships v2 only; Apple Silicon uses apple-m1.

Compare the archive's SHA-256 with its exact filename's entry in `SHA256SUMS`
before running: PowerShell `Get-FileHash <archive> -Algorithm SHA256`,
Linux `sha256sum <archive>`, or macOS `shasum -a 256 <archive>`.
Extract the whole archive using Windows **Extract All** or
`tar -xzf <archive>` on Linux/macOS; keep `ui/` beside the binaries.
No Rust, Cargo or Node.js installation is needed.
Requirements and CPU checks:
<https://github.com/HIRO15254/solvers/blob/main/docs/user-guide.jp.md#1-インストールする>.

## Quick start

GUI: run `start-gui.cmd` (Windows) or `sh start-gui.sh` (Linux, macOS) from the
extracted directory. It starts `solversd` with run directories under `runs/`
next to the binaries and opens the GUI. Keep the window open while you use the
GUI; close it (Ctrl-C) to stop the daemon.

CLI:

On Windows PowerShell replace `./solvers` below with `.\solvers.exe`.

```sh
./solvers config new --out spot.toml       # a valid P1 template
./solvers validate spot.toml
./solvers solve spot.toml --out runs/first
./solvers export runs/first/solution.sol summary
```

## Unsigned binaries

The binaries are not code-signed.

- **Windows**: SmartScreen may say "Windows protected your PC". Choose
  *More info* → *Run anyway*, or before the first start run in PowerShell inside
  the extracted directory: `Get-ChildItem -Recurse | Unblock-File`.
- **macOS**: Gatekeeper blocks files downloaded from the internet. Run once in
  Terminal inside the extracted directory: `xattr -dr com.apple.quarantine .`
  Alternatively, after attempting to open it, use **System Settings →
  Privacy & Security → Open Anyway**. Do not disable Gatekeeper globally.
- **Linux**: no extra step.

Verify the download against `SHA256SUMS` on the release page before running.
Only unblock downloads from the trusted release page whose checksum matches.
Keep the daemon on loopback unless you configure TLS; non-loopback binds
require TLS. Treat its bearer token and token-bearing GUI URL as secrets.

## License

MIT OR Apache-2.0, at your option. Third-party notices are in
`THIRD-PARTY-LICENSES.txt`.
