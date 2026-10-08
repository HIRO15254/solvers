# P1 DCFR係数の掃引とPDCFR+試作（2026-10-07）

状態: 計測完了。問いは3つ。
- 既定DCFR（`alpha 1.5, beta 0, gamma 3`）より少ないiterationで0.1% potに達する係数があるか。あるなら、木によらず遅くならないか。
- PDCFR+（予測付きDCFR+、T11）はDCFRより速いか。
- 旧i16 storage（両arena i16）の精度床は、resetを既定でやめたこと（PF4）とCFR計算のf32化（PF5）のどちらで変わったか。

関連は[P1性能計画](../../../docs/plans/p1-performance.jp.md)第3節、Linear SOL-15。
前回の掃引（[0.1% potまでの収束](../convergence-20261007/README.md)）は`beta`・`alpha`を1つずつ動かし、一貫して勝る設定が無かった。
今回は`alpha`を下げて`beta`を正にする組合せと、3-bet potの木を加えた。
再現状態は`verified`（記載scriptで計測したprogressと集計を保持）。集計は[result.json](result.json)。

## 条件

- GCP c2d-highcpu-32 Spot（europe-west4-a、AMD EPYC 7B13、16 core/32 thread、62 GiB）、VM `p1perf-6`（2026-10-07）。
  同じVMの受入計測は[T9・T10受入](../accept-t9-t10-20261007/README.md)。
- binaryは`31a8b7a`（T9、`cfr_precision`既定f32）の`solvers solve`、32 threads、f32 storage（記載のあるrunを除く）。
- 木（`configs/`）。どれもNL50 rake 5% cap 4 BB、6max 100bb。
  - Turn・Flop1・gtow_b: [収束計測](../convergence-20261007/README.md)と同じ（BTN vs BB SRP、`Ks 7h 2d`、Turnは`3c`）。
  - River（`c_river`）: 同じSRPのriver `9s`、bet 33/75/150/all-in。
  - Flop2・Turn2（`c_flop2`・`c_turn2`）: BTN vs BB 3-bet pot（pot 22.5 BB）、`Jh Th 8c`（Turn2は`2s`）。
  - Flop3（`c_flop3`）: SRPのwet board `9h 8h 6c`、gtow_bに近いmenu（IP 33/75、OOP 33、Turn/River 50/100、all-in）。f32 storage 8.7 GB。
- 指標は`(NashConv/2)/開始pot×100`。到達iterationは検査点の間を対数線形に補間した値（`check_every`はRiver 5、他の小さい木10、gtow_b・Flop3 25）。
- `scripts/sweep.py`（sweep1）: 13通りの係数をTurn・Flop2で0.05%まで解き、上位4つと既定をFlop1で0.1%まで解いた。
- `scripts/sweep2.py`（sweep2）: sweep1の勝者の近傍16通りを5つの木（Turn・Flop1・Flop2・Turn2・River）で0.05%まで解いた。
- `scripts/pe.sh`・`pf.sh`・`ph.sh`: 候補をgtow_b（f32・i16-f32avg）とFlop3で確かめた。
- `scripts/sweep3.py`: 別のspot（Flop4〜6）とi16系storageで既定とs2を比べた。
- `scripts/sweep4.py`: 旧i16のTurnで`pow4_reset`と`cfr_precision`を切り替えた。
- `scripts/local_j.py`: 旧i16のRiver・Flop2・Flop1を手元の共有PC（6 threads）で解いた。
  VMが6時間で自動削除されたため、VM用に用意した`scripts/sweep5.py`の定義を手元で使った。
  binaryは`782bb6a`以後のrelease build（T11試作のbuildで、環境変数が未設定なら`782bb6a`と出力が一致する）。
- T11（PDCFR+）: 試作`d9c35f6`（branch `s3-p1-pdcfr-proto`、差分は`scripts/pdcfr-proto-d9c35f6.patch`）。
  環境変数`SOLVERS_P1_PDCFR="alpha,gamma"`で切り替え、f32 storageに3つ目のarena（直前の瞬間regret）を足す。
  手元の共有PC（Windows）で、`configs/t11_turn.toml`（Turn、8 threads）と`configs/t11_flop2.toml`（Flop2、8 threads）を解いた（`scripts/t11_measure.py`）。

## 結果

sweep2（0.1%到達iterationの既定比。geo・worstは5つの木の幾何平均と最大）:

| 係数（alpha, beta, gamma） | Turn | Flop1 | Flop2 | Turn2 | River | geo | worst | 0.05% geo | 0.05% worst |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 既定（1.5, 0, 3） | 723 it | 246 it | 209 it | 230 it | 541 it | 1 | 1 | 1 | 1 |
| 1.25, 0.5, 4 | 0.89 | 0.74 | 0.98 | 1.05 | 0.66 | 0.853 | 1.05 | 0.785 | 0.99 |
| 1.25, 0.25, 5 | 0.92 | 0.70 | 0.90 | 1.20 | 0.65 | 0.854 | 1.20 | 0.811 | 1.04 |
| 1.25, 0.25, 4 | 0.92 | 0.71 | 0.91 | 1.21 | 0.65 | 0.860 | 1.21 | 0.810 | 1.04 |
| 1.25, 0.5, 3 | 0.90 | 0.76 | 1.01 | 1.06 | 0.66 | 0.864 | 1.06 | 0.789 | 1.01 |
| 1.25, 0.5, 5 | 0.90 | 0.75 | 0.99 | 1.10 | 0.66 | 0.865 | 1.10 | 0.802 | 1.02 |
| 1.25, 0.25, 3 | 0.93 | 0.74 | 0.94 | 1.25 | 0.67 | 0.884 | 1.25 | 0.826 | 1.08 |
| 1.25, 0.5, 2 | 0.95 | 0.85 | 1.12 | 1.16 | 0.70 | 0.942 | 1.16 | 0.859 | 1.14 |
| 1.25, 0.25, 2 | 0.99 | 0.84 | 1.06 | 1.35 | 0.71 | 0.969 | 1.35 | 0.900 | 1.20 |
| 1.4, 0.25, 3 | 0.95 | 0.88 | 1.04 | 1.13 | 0.95 | 0.985 | 1.13 | 0.907 | 1.03 |
| 1.4, 0.5, 3 | 0.93 | 0.96 | 1.07 | 1.06 | 0.93 | 0.990 | 1.07 | 0.936 | 1.07 |
| 1.1, 0.5, 3 | 1.40 | 0.76 | 1.20 | 2.42 | 0.66 | 1.155 | 2.42 | 1.042 | 2.01 |
| 1.1, 0.25, 3 | 1.81 | 1.34 | 1.57 | 1.63 | 0.63 | 1.316 | 1.81 | 1.210 | 1.93 |
| 1.25, 1.0, 3 | 未達 | 3.61 | 3.34 | 4.68 | 1.89 | 3.885 | 8.30 | 4.568 | 7.35 |
| 1.4, 1.0, 3 | 未達 | 4.45 | 3.99 | 5.90 | 2.10 | 4.491 | 8.30 | 4.996 | 7.45 |
| 1.1, 1.0, 3 | 未達 | 2.87 | 3.16 | 未達 | 1.26 | 4.767 | 26.11 | 5.185 | 17.50 |

- 未達は上限iteration（Turn 3000、Turn2 3000）の2倍として平均に入れた。
- sweep1（Turn・Flop2の幾何平均）では（1.25, 0.25, 3）が0.936で1位、（1.25, 0.5, 3）が0.951で2位。
  Flop1では既定比0.739・0.760だった。`alpha 2`はどれも1.2倍以上遅い（`raw/sweep1.*`）。
- 評価を含めた1 iterationの時間は、係数による差が各木で±5%程度に収まった。0.1%到達の時間比はiteration比とほぼ同じ。

大きい木（`scripts/pe.sh`・`pf.sh`・`ph.sh`、32 threads。検査点（25 iterationごと）での初到達iteration／solve経過秒）:

| 木・storage | 目標 | 既定 | s2（1.25, 0.5, 4） | 時間比 |
|---|---|---:|---:|---:|
| gtow_b f32 | 0.1% | 500 it／733.5 s | 325／464.7 s | 0.63 |
| gtow_b f32 | 0.05% | 725／1,045.0 s | 425／589.1 s | 0.56 |
| gtow_b i16-f32avg | 0.1% | 500／885.9 s | 300／517.7 s | 0.58 |
| Flop3 f32 | 0.1% | 400／248.0 s | 300／187.2 s | 0.75 |
| Flop3 f32 | 0.05% | 575／353.5 s | 425／262.2 s | 0.74 |

- s2の曲線は単調に下がった。gtow_bではsweep1の勝者（1.25, 0.25, 3）が675 iteration・980.4秒と既定より遅かった。
  150→200で0.528→0.584%、350→400で0.124→0.557%と、途中で大きく戻った（`raw/conv/c_gtowb_best.progress.jsonl`）。
- peak RSSは係数によらず、gtow_bでf32 28.2 GB、i16-f32avg 22.9 GB、Flop3で12.2 GB。

別のspotとstorage（`scripts/sweep3.py`、32 threads。補間iterationの既定→s2）:

| 木 | storage | 0.1% | 比 | 0.05% | 比 |
|---|---|---:|---:|---:|---:|
| Flop4（monotone `Ks 9s 4s`、Flop1と同じmenu） | f32 | 283.9→201.6 | 0.71 | 424.1→285.1 | 0.67 |
| Flop5（4-bet pot `Qd 8c 3h`、pot 48.5 BB、SPR 1.6） | f32 | 368.1→302.5 | 0.82 | 485.0→448.8 | 0.93 |
| Flop6（Flop1を200bbにした木） | f32 | 239.5→179.1 | 0.75 | 359.7→271.6 | 0.76 |
| Turn | i16-f32avg | 849.7→628.9 | 0.74 | 1,175.7→912.1 | 0.78 |
| Flop1 | i16-f32avg | 254.7→189.4 | 0.74 | 372.6→267.5 | 0.72 |

旧i16（両arena i16）の精度床（`scripts/sweep3.py`・`sweep4.py`はGCP 32 threads、`scripts/local_j.py`は手元6 threads）:

| 木 | 係数 | `pow4_reset` | CFR計算 | 最良（iteration）、または0.05%到達 | 0.1%到達 |
|---|---|---|---|---:|---:|
| Turn | 既定 | false | f32 | 0.764%（340） | 未達 |
| Turn | 既定 | false | f64 | 0.752%（310） | 未達 |
| Turn | 既定 | true | f64 | 0.120%（1,070） | 未達 |
| Turn | 既定 | true | f32 | 0.074%（1,390） | 1,109 |
| Turn | s2 | false | f32 | 0.561%（420） | 未達 |
| Turn | s2 | true | f32 | 0.050%（1,300） | 1,069 |
| River | 既定 | false | f32 | 0.319%（370） | 未達 |
| River | 既定 | true | f32 | 0.05%到達（1,200） | 527 |
| River | s2 | false | f32 | 0.218%（460） | 未達 |
| River | s2 | true | f32 | 0.05%到達（780） | 381 |
| Flop2（3-bet pot） | 既定 | false | f32 | 0.078%（450） | 287 |
| Flop2（3-bet pot） | 既定 | true | f32 | 0.05%到達（340） | 245 |
| Flop2（3-bet pot） | s2 | false | f32 | 0.05%到達（470） | 221 |
| Flop2（3-bet pot） | s2 | true | f32 | 0.05%到達（320） | 206 |
| Flop1 | s2 | false | f32 | 0.075%（440） | 235 |
| Flop1 | s2 | true | f32 | 0.05%到達（310） | 193 |

- TurnはGCP（上限3000 iteration）、River・Flop2・Flop1は手元（上限3000・2000・1000）。iterationは機械によらない。
- Turnの「既定・true・f64」は、[収束計測](../convergence-20261007/README.md)の旧既定（reset有り）のi16（最良0.120%）を再現した。
- i16の床は、CFR計算のf32化（PF5）ではなく、resetを既定でやめたこと（PF4）で深くなった。
  PF4の根拠はf32とi16-f32avgだけで測っており、旧i16を含んでいなかった。
- reset有りでも、最良点の後は反復とともに悪化する（Turnのs2は3000で0.170%）。0.1%の目標で止めれば影響しない。

T11（PDCFR+、手元8 threads。0.1%到達）:

| 木 | DCFR（既定） | PDCFR+（2.3, 5） | PCFR+（∞, 2） |
|---|---:|---:|---:|
| Turn | 730 it／139.6 s | 1,760／365.3 s | 1,770／375.2 s |
| Flop2（3-bet pot） | 210／94.4 s | 460／283.8 s | – |

- 予測を外して同じ係数にしたDCFR（`alpha 2.3, beta −20, gamma 5`、[収束計測](../convergence-20261007/README.md)）はTurn 1,080 iterationだった。
  予測を加えると遅くなる。
- PDCFR+はf32 storageに1つarenaを足すので、storageは1.5倍（12 byte/要素）になる。
- 環境変数が未設定のとき、試作binaryの出力は`782bb6a`と一致した（Turn6の`.sol` payloadとcheckpointのstorage hash）。

## 判断

- s2（`alpha 1.25, beta 0.5, gamma 4`）の0.1%到達iterationは、f32の10個の木とi16-f32avgの3つの木で既定の0.60〜1.05倍だった。
  遅くなったのは3-bet pot Turnの+5%だけで、そこも0.05%目標では0.99倍。gtow_bでは0.1%までの時間が37%、0.05%までが44%短い。
  利用者決定PF7（2026-10-07）により、P1のDCFR既定をこの値へ変える（T12）。`pow4_reset = false`は変えない。
- `beta 1.0`、`alpha 1.1`、`alpha 2`は遅い。`beta 0.25`は小さい木では速いが、gtow_bで途中に大きく戻るので採らない。
- 0.05%より深い目標は測っていない。
- 旧i16は、resetが無いとTurn・Riverで0.1%に届かない。reset有りはどの木でも遅くならなかった。
  利用者決定PF8（2026-10-07）により、`storage = "i16"`で`pow4_reset`を書かないときだけ既定をtrueにする（T13）。
  f32・i16-f32avgの既定はfalseのまま。
- PDCFR+（T11）は収束が遅く、memoryも増えるので採らない。試作branchはmergeしない。
