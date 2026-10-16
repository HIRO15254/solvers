# solvers — NLH HU Postflop Solver

English | [日本語](README.jp.md)

A No-Limit Hold'em solver for heads-up postflop spots (flop, turn, river).

## Start

### Windows

Double-click `start-gui.cmd`. The GUI opens in your browser.

If Windows shows a warning, choose **More info** → **Run anyway**. Alternatively,
right-click the downloaded zip → **Properties** → tick **Unblock**, then extract it again.

If it doesn't start, or stops right away with an error, use the `-compat` zip.
That build is for older CPUs without AVX2.

### macOS

Open **Terminal**, type `cd ` (with a space after `cd`), drag this folder onto the
Terminal window, and press Enter. Then run these two lines. The GUI opens in your browser.

```sh
xattr -dr com.apple.quarantine .
sh start-gui.sh
```

The first line lifts macOS Gatekeeper's block on unsigned apps for this folder only.

### Linux

Run `sh start-gui.sh` in this folder. If it doesn't start, use the `-compat`
build. It is for older CPUs without AVX2.

## Solve in the GUI

1. In **Setup**, describe the spot. For a first try, you can keep the example
   that is already filled in. **Validation** on the right shows how much memory
   the solve needs.
2. Press **Solve** at the top right.
3. Watch the progress in **Runs** and wait until the solve finishes.
4. Open **Results** to see the strategy and EVs.

Keep the terminal window that opened at startup open while you use the GUI;
closing it stops the solver. Your results stay in `runs/` inside this folder.

## More

For the CLI and more, see the
[user guide](https://github.com/HIRO15254/solvers/blob/main/docs/user-guide.jp.md) (Japanese).

## License

MIT OR Apache-2.0, at your option. Third-party notices are in
`THIRD-PARTY-LICENSES.txt`.
