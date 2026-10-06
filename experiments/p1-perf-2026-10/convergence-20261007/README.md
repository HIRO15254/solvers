# P1-T4 収束計測: 0.1% potまでの時間（2026-10-06〜07）

状態: 計測完了。問いは「利用者目標のExploitability 0.1% pot（2026-10-07）まで、多core機でどれだけかかり、
scheduleとstorageの選択でどう変わるか」。関連要件は[P1性能計画](../../../docs/plans/p1-performance.jp.md)第3節、
Linear SOL-15。再現状態は`verified`（記載scriptで計測したprogressと集計を保持）。集計は[result.json](result.json)、
各runのExploitability曲線は`raw/`。

## 条件

- GCP c2d-highcpu-32 Spot（europe-west4-a、AMD EPYC 7B13、16 core/32 thread、64 GB）、Debian 12、rustc 1.97.0、
  `target-cpu=native`。2026-10-06 15:02〜19:02 UTC（max-run 4時間で自動削除）。
- source `6234545`（T1〜T3）。`scripts/setup2.sh`で旧`43e97c6`と並べてbuildした。
- `scripts/run_conv.sh`: `solvers solve`を32 threads・停止目標なしで実行し、`progress.jsonl`を回収する。
  指標は`(NashConv/2)/5.5 BB×100`（開始pot比%）。到達iterationは`check_every`（Turn 10、他25）ごとの最初の観測値。
- `scripts/sweep.sh`: 既定DCFRの`alpha`/`beta`/`gamma`/`pow4_reset`を変えた11通り（`configs/sweep/`）。
- `scripts/cli_ab.sh`: CLI全体（`configs/flop_srp1_cli.toml`、4 iteration、checkpoint間隔1秒）を旧新で比べた。
- 木（`configs/conv/`）: 6max 100bb BTN vs BB SRP、NL50 rake 5% cap 4 BB、board `Ks 7h 2d`。
  Turn（`3c`、bet 33/75/150/all-in、59,379 node）、Flop1（bet 33/75/75、raise 3x、cap 2、829,242 node、f32 2.8 GB）、
  GTOWb（Flop IP 33/50/75/125%・OOP 33%、Turn/River 50/100%、all-in、cap 3/2/2、f32 storage 21 GB）。

## 結果

0.1% potへの初到達（iteration／solve経過秒、評価時間を含む）:

| 木 | dcfr（既定） | hs-dcfr | linear-cfr | cfr-plus | dcfr＋i16 |
|---|---:|---:|---:|---:|---:|
| Turn | 770／14 s | 1210／23 s | 1190／22 s | 1520／29 s | 未達（最良0.120%、3000で0.592%） |
| Flop1 | 250／58 s | 250／58 s | 450／102 s | 825／191 s | 300／87 s |
| GTOWb（800まで） | 575／1006 s | 625／1075 s | – | – | 未達（最良0.292%、800で0.385%） |

- 既定DCFRがどの木でも最速。i16（両arena i16）は木によって0.1%に届かず、反復を続けると悪化する。
  GTOWbのi16は1 iteration 2.12秒（f32 1.75秒）、peak RSS 23.1 GB（f32 33.5 GB）。
- `pow4_reset`（4の累乗iterationで平均戦略をreset）の直後に一時悪化する。Turnは1020→1030で0.062→0.107%、
  GTOWbは250→275でほぼ停滞、i16では275で0.35→1.23%。

DCFR係数の掃引（0.1%到達iteration。基準は`alpha 1.5, beta 0, gamma 3, reset有り`）:

| 設定 | Turn | Flop1 |
|---|---:|---:|
| 基準 | 770 | 250 |
| reset無し | 770 | 250 |
| gamma 2 / 4 / 5（5はreset無し） | 790 / 780 / 790 | 300 / 250 / 250 |
| beta 0.5 | 690 | 325 |
| beta −20（負regretをほぼ0に切る、DCFR+相当） | 890 | 325 |
| alpha 1.0 / 2.0 / 3.0 | 未達 / 880 / 1240 | 400 / 400 / 500 |
| alpha 2.0, gamma 4, reset無し | 930 | 375 |
| alpha 2.3, beta −20, gamma 5, reset無し（PDCFR+の係数） | 1080 | 450 |

両方の木で基準より明確に速い設定は無い。reset無しは0.1%到達が同じで、Turnの0.05%到達が1390→1180と早い。

CLI全体（Flop1、4 iteration）: 16 threadsで旧201.7秒・peak RSS 29.7 GB→新24.8秒・4.67 GB、32 threadsで
200.7秒・29.8 GB→24.6秒・4.95 GB。`export strategy/ev`はbyte一致、`summary`はwall_secs以外一致。

## 判断

- scheduleの既定は変えない（掃引で一貫して勝る設定が無い）。
- i16の精度床への対策は試作（`i16-precision-20261007`、branch `s3-p1-i16-proto`）で比べ、利用者決定PF2・PF3
  （計画第1節）により、regretをi16・戦略累積をf32で持つstorageを新しい値として追加し、旧i16も残す。
- 既定の`check_every`25では評価が時間の約8%を占める。

GCP費用はこのVMで約4時間分（Spot）。保持物の識別は[manifest.json](manifest.json)。
