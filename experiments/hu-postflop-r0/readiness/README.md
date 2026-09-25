# SOL-3 readiness観測（2026-09-25）

これはR0のsource候補・hostの取得結果。solverをbuild/実行した証拠ではない。
[記録手順とcase用template](../../../docs/plans/hu-postflop-r0/provenance.md)をR1で使用する。

- source候補: `753139e30a0c4a4fac9e97113ecd005d6355d7a9`。この作業の編集前に `git status --porcelain=v1 --untracked-files=all` が空。HEADはlocal Gitと `origin/main` に存在し、patch・追加sourceなし。`source-manifest.json` のSHA-256（source ID）は `073a5feff3bdde52701f89ff5328b8eda61a84832d56dd3a8c3b5e2a52645d2e`。この記録の後にSOL-3の文書を追加したため、将来のbuild直前にはsourceを再取得する。
- Cargo: `.cargo/config.toml` は `-C target-cpu=native`。`Cargo.lock` のSHA-256は `a8658dd63e76d22e313be59599d59c28fe0778d7cb86127a6b339114b1372fe1`、`.cargo/config.toml` は `144d563007668a15d8bacbadd5b026b3ce346a0e57fa7e73ea8235346090cd21`。user Cargo configは見つからず、`CARGO_NET_OFFLINE=true`、`RUST_LOG=warn`。Cargo build profile/feature/envの確定値はR1 build時に記録する。
- Toolchain: default `stable-x86_64-pc-windows-msvc`、rustc 1.97.0 (`2d8144b78`, 2026-07-07)、cargo 1.97.0 (`c980f4866`, 2026-06-30)。repository内に `rust-toolchain*` はない。
- Binary: `target/debug/solvers.exe`、`target/release/solvers.exe`、`target/debug/solversd.exe`、`target/release/solversd.exe` は見つからなかった。buildを行っていないため、path/hash/build成功は未記入。R1のbuild直後に実値を保存する。
- Host: [host.json](host.json)にCPU 8物理/16論理core、RAM 34,275,098,624 bytes、観測時の空きRAM 7,227,535,360 bytes、C:空き84,012,253,184 bytes、OS/電源を記録。CIMと `systeminfo` はaccess denied、`wmic` は未導入だったためWindows APIと`.NET DriveInfo`を使った。GPUはCPU-only測定では不要。

R0-05へ: 観測時の空きRAMは約6.73 GiBで、同時にmemory load 78%。この値は固定の上限ではない。初回solve前に `pwsh -NoProfile -File tools/host_probe.ps1 -Volume C:`（実際の出力先volumeに変更）で空きRAM・page file・diskを再取得し、OS/他process用の余裕を引いた資源枠、threads、同時実行数を決める。仮想化・job制限がある場合はその上限も確認する。

検証時の資源観測: `cargo test --workspace` の並列compileはWindowsのページングファイル不足（OS error 1455）で失敗した。同じsourceで `CARGO_BUILD_JOBS=1` にして再実行すると全workspace testが通過した。これはsolverのpeak memory測定ではなく、R0-05でbuild/testの同時実行枠を決める材料である。
