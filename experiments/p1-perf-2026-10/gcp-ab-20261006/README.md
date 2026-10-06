# P1-T4 GCP新旧計測（T1＋T2、2026-10-06）

状態: 計測完了。問いは「T1（席別support）とT2（engine評価pass統合）で、多core機の速度とmemoryがどれだけ変わるか」。
関連要件は[P1性能計画](../../../docs/plans/p1-performance.jp.md)第3節T4、Linear SOL-15。
再現状態は`verified`（記載scriptで計測した生データと集計を保持）。集計は[result.json](result.json)、生データは`raw/`。

## 条件

- GCP c2d-highcpu-32 Spot（us-central1-b、AMD EPYC 7B13、16 core/32 thread、64 GB）、Debian 12、rustc 1.97.0、
  `target-cpu=native`、THP `always`。2026-10-06 13:34〜14:12 UTC（約0.2 USD）。
- 旧は`43e97c6`、新は`e7a4554`（T1 `cf45d5e`＋T2 `31808e8`）。同じVMで`scripts/setup2.sh`により両方をbuildした。
- `scripts/ab.sh`: `p1_bench`をthread数ごとに旧→新の順で交互に実行し、2 round（Flop i16は1 round）の中央値をとる。
  warmup 1 iteration、Turnは5 iteration、Flopは3 iteration、評価1回。
- `scripts/cli_ab.sh`: CLI全体（`solvers solve`、4 iteration、`check_every 2`、checkpoint間隔1秒、16 threads）を
  `/usr/bin/time -v`で計測し、rootの`export strategy/ev/summary`を比べた。
- 木は`configs/`。6max 100bb BTN vs BB SRP（NL50 rake 5% cap 4 BB）、board `Ks 7h 2d`、開始rangeはBTN 436・BB 479 combo。

## 結果

f32ではすべての行で新旧の`nashConv`がf64として一致した（i16はblock scaleの変更で僅差、T1の規範どおり）。

Turn 4 size＋all-in（`turn_multi.toml`、59,379 node）:

| threads | 旧 s/iter | 新 s/iter | 倍率 | 旧 評価 s | 新 評価 s | 旧 peak | 新 peak |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | 1.402 | 0.274 | 5.1 | 2.70 | 0.266 | 637 MB | 230 MB |
| 4 | 0.364 | 0.0706 | 5.2 | 0.684 | 0.068 | 637 MB | 242 MB |
| 8 | 0.190 | 0.0363 | 5.2 | 0.352 | 0.035 | 639 MB | 250 MB |
| 16 | 0.110 | 0.0194 | 5.6 | 0.192 | 0.018 | 640 MB | 257 MB |
| 32 | 0.121 | 0.0172 | 7.1 | 0.232 | 0.018 | 642 MB | 266 MB |

Flop 33/75/75・raise 3x cap 2（`flop_srp1.toml`、829,242 node）:

| threads | 旧 s/iter | 新 s/iter | 倍率 | 旧 評価 s | 新 評価 s | 旧 peak | 新 peak |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | 19.11 | 3.59 | 5.3 | 33.9 | 3.38 | 8.44 GB | 3.08 GB |
| 8 | 2.339 | 0.459 | 5.1 | 4.06 | 0.43 | 8.45 GB | 3.12 GB |
| 16 | 1.333 | 0.234 | 5.7 | 2.09 | 0.22 | 8.46 GB | 3.18 GB |
| 32 | 1.224 | 0.210 | 5.8 | 1.94 | 0.20 | 8.46 GB | 3.29 GB |
| 16（i16） | 1.384 | 0.312 | 4.4 | 2.12 | 0.22 | 4.38 GB | 1.77 GB |
| 32（i16） | 1.298 | 0.258 | 5.0 | 1.95 | 0.20 | 4.38 GB | 1.87 GB |

- 新の並列効率: Flopは1→16 threadsで15.3倍、32 threads（SMT）でさらに10%短縮。Turnは14.1倍、SMTで11%短縮。
- 評価1回の費用はiteration約1回分になった（旧は約1.6〜1.9回分）。

大きな木（新のみ、32 threads、warmup 1＋2 iteration）。旧はf32見積りが64 GBを超え、この機械では確保できない。

| 木 | node | storage | 見積り | peak | build s | 初回 iter s | s/iter | 評価 s |
|---|---:|---|---:|---:|---:|---:|---:|---:|
| 同梱`flop_srp.toml`（33/75全street、cap 3） | 8,704,350 | f32 | 30.5 GB（旧88.4） | 33.3 GB | 6.0 | 12.3 | 2.22 | 2.30 |
| GTO Wizard風B（IP 4 size・OOP 33%、Turn/River 50/100） | 6,088,200 | f32 | 21.1 GB | 23.1 GB | 4.3 | 8.5 | 1.52 | 1.61 |
| GTO Wizard風A（同、Turn/River 33/75/125） | 16,088,610 | i16 | 28.4 GB（f32 56.6） | 33.2 GB | 11.1 | 14.3 | 4.94 | 4.59 |

初回iterationの超過分は新規storageのpage faultである（THPの`madvise`/`always`で差は無かった）。

CLI全体（Flop木、16 threads）:

| | 旧 | 新 |
|---|---:|---:|
| process壁時計 | 198.2 s | 55.4 s |
| うちsolve（`wall`、checkpoint保存を含む） | 103.7 s | 16.2 s |
| peak RSS | 29.7 GB | 11.0 GB |
| checkpoint / `.sol` | 2.47 GB / 210 MB | 1.04 GB / 173 MB |

新のeventsでは、iteration・評価の計算は約1.5秒で、checkpoint保存2回に約37秒、`.sol`出力に約16.5秒を使う。
peak RSSはstorage（2.8 GB）の約3.9倍である。これは保存処理の構造（T3）による。rootの`strategy`・`ev`のCSVはbyte一致、
`summary`は`wall_secs`以外一致。

## profile（新、1 thread、`raw/prof_*.txt`）

Flop: `showdown_kernel` 32%、`fold_kernel` 15%、`cfr_pass` 15%、`normalize_columns` 12%、評価pass 5%、page fault関連約5%。
Turnも同傾向。終端kernelは引き続き約半分を占める。

## 未達・次

- 保存処理（checkpoint・`.sol`）の時間とpeak RSSはT3で扱い、T3後にCLI全体を同条件で再計測する。
- SMTは約10%の短縮があり、threads既定（論理core数）を変える理由は見当たらない。
