# solvers

English | [日本語](README.jp.md)

A No-Limit Hold'em solver for heads-up postflop spots (flop, turn, river), with a browser GUI.

## Quick start

Download the file for your OS from the
[Releases page](https://github.com/HIRO15254/solvers/releases), then follow the steps
below. You don't need to install Rust, Node.js or anything else.

### Windows

1. Download `solvers-<version>-x86_64-pc-windows-msvc.zip`.
2. Right-click the zip, open **Properties**, tick **Unblock** near the bottom, and press **OK**.
   If you skip this step, Windows shows a warning when the solver starts.
3. Right-click the zip and choose **Extract All**.
4. In the extracted folder, double-click `start-gui.cmd`. The GUI opens in your browser.

If it doesn't start, or stops right away with an error, start over with the
`-compat` zip. That build is for older CPUs without AVX2.

### macOS

1. Download the file for your Mac:
   - Apple Silicon (M1 or later): `solvers-<version>-aarch64-apple-darwin.tar.gz`
   - Intel: `solvers-<version>-x86_64-apple-darwin.tar.gz`
2. Double-click the downloaded file to extract it.
3. Open **Terminal**, type `cd ` (with a space after `cd`), drag the extracted folder
   onto the Terminal window, and press Enter.
4. Run these two lines. The GUI opens in your browser.

   ```sh
   xattr -dr com.apple.quarantine .
   sh start-gui.sh
   ```

   The first line lifts macOS Gatekeeper's block on unsigned apps for this folder only.

### Linux

```sh
tar -xzf solvers-<version>-x86_64-unknown-linux-gnu.tar.gz
cd solvers-<version>-x86_64-unknown-linux-gnu
sh start-gui.sh
```

If it doesn't start, use the `-compat` build. It is for older CPUs without AVX2.

### Solve in the GUI

1. In **Setup**, describe the spot. For a first try, you can keep the example
   that is already filled in. **Validation** on the right shows how much memory
   the solve needs.
2. Press **Solve** at the top right.
3. Watch the progress in **Runs** and wait until the solve finishes.
4. Open **Results** to see the strategy and EVs.

Keep the terminal window that opened at startup open while you use the GUI;
closing it stops the solver. Your results stay in `runs/` inside the extracted folder.

## More

- [User guide](docs/user-guide.jp.md) (Japanese): requirements, verifying downloads
  with `SHA256SUMS`, the CLI, and how to read results.
- Release downloads contain only the HU Postflop solver (P1). The 2–9 player
  Preflop solver (P2) is available only when you build from source.
- For developers: [docs/README.md](docs/README.md) and the
  [development guide](docs/development.md).

## License

MIT OR Apache-2.0, at your option. Third-party components and their licenses
are listed in [LICENSE-POLICY.md](LICENSE-POLICY.md).
