# 配布バイナリのCPU基準（SOL-40）

2026-10-15。配布物のx86-64ビルドをどのCPU基準（`-C target-cpu`）で作るかを決めるための計測。
ローカル参考値（共有PCのため雑音が大きい）。

## 問い

`.cargo/config.toml`の`target-cpu=native`は配布できない。x86-64（基準）、x86-64-v2、x86-64-v3、nativeで、
P1の1反復の時間と解の一致を比べる。

## 条件

- source: `3bd466e9`。P1の計算crate（hu-postflop・hu-engine・nlh・spot・economics）に未コミットの差分なし。
- 機械: Intel Core i7-10700KF（8 core / 16 thread、AVX2あり・AVX-512なし）、32 GB、Windows。他の作業と共用。
- toolchain: rustc 1.97.0。
- build: `RUSTFLAGS="-C target-cpu=<v>" cargo build --release -p hu-postflop --example p1_bench --target-dir target/cpu/<v>`
- 計測: [`run.py`](run.py)。`p1_bench`をwarmup 2回の後に計測。各条件を3 round、round毎にvariantの順を回した。
  - `flop_4bp`（469,808 node）16 threads 60反復、`flop_3bp`（2,222,550 node）16 threads 20反復、`flop_4bp` 1 thread 8反復。
  - storageはf32とi16。

## 結果（1反復の中央値、秒。括弧はx86-64比）

| 条件 | x86-64 | x86-64-v2 | x86-64-v3 | native |
|---|---|---|---|---|
| flop_3bp 16t f32 | 0.499 | 0.479 (0.96) | 0.411 (0.83) | 0.411 (0.82) |
| flop_3bp 16t i16 | 0.576 | 0.464 (0.81) | 0.440 (0.76) | 0.480 (0.83) |
| flop_4bp 1t f32 | 0.328 | 0.343 (1.05) | 0.257 (0.78) | 0.278 (0.85) |
| flop_4bp 1t i16 | 0.418 | 0.351 (0.84) | 0.376 (0.90) | 0.386 (0.92) |
| flop_4bp 16t f32 | 0.052 | 0.098 (1.90) | 0.070 (1.34) | 0.087 (1.68) |
| flop_4bp 16t i16 | 0.095 | 0.098 (1.03) | 0.061 (0.64) | 0.076 (0.80) |

- **NashConvは全条件で4 variantがbit一致**（`results.json`の`nashConv`）。CPU基準は解を変えない。
- 雑音: 同一条件のx86-64の3回でも最大1.9倍の幅がある（flop_4bp 16t f32: 0.097/0.052/0.049）。1反復が0.1秒未満のflop_4bp 16tは判断に使わない。
- 最も安定したflop_3bp 16tでは、v3がx86-64の0.76〜0.83倍でnativeと同等、v2は0.81〜0.96倍。

## 判断（利用者決定 2026-10-15）

- Windows・Linuxのx86-64は、**x86-64-v3を通常の配布物、x86-64-v2を`compat`配布物**として2種を出す。
- Intel macOSはv2の1種、Apple Siliconは`apple-m1`（targetの既定）。
- 適用先: `.github/workflows/release.yml`、[配布計画](../../../docs/plans/release-v0.1.jp.md)。

## 制限

- 共有PCの参考値であり、性能受入の証拠ではない。AMD CPU・AVX-512搭載機・macOSは測っていない。
- 再現状態: `partial`（記載手順で再実行できるが、雑音のため数値は再現しない）。

## manifest

| file | SHA-256 |
|---|---|
| `run.py` | `72603252656871f49f8804ed2665d8f9cddd061e9265d6f553b0bf1520d52a9b` |
| `results.json` | `94a009eed9f9d751caa204239039a0724b473fd032dc6028ac1a7ef2ed963f5e` |
