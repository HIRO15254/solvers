# VM06 最終 source06：品質・性能比較

固定した3つの合成ゲームで、保存済み戦略の品質を保ちながらメモリと全工程時間を削減した。
最終製品コード source06 の Flop 中央値は **12.133 → 10.149秒（−16.4%）**、root OS peak は
**731.2 → 438.5 MB（−40.0%）**。summary 読込みは **0.588 → 0.0290秒（20.24倍）**だった。
一方、`.sol` は **131,744 → 176,833 bytes（+34.2%）**に増えた。
Turn は時間−12.4%、root peak−28.8%。River の時間差は変動に比べて小さく、高速化の根拠としない。

評価は **限定条件付きで共有可能**。これは最終 source06 binary を実際に測定した比較である。
[以前の source03 比較](vm06-report.md) は別の履歴として維持し、その数値を混ぜていない。
外部参照24条件の一致・受入はこの実験から認定しない。
数値の正本は [機械可読 report](current-report.json)、再計算は [独立 validator](current-verification.py)。

## 固定条件と source / host の対応

- baseline `9632d8b` / `.sol` v1：source archive `fdd8c1c0…33a0197`、binary `77418ab6…cdeb8`。
  candidate source06 / `.sol` v3：archive **`f241167a9c765b839cbe560ec66c9c490a0b0193d8f23ba8de768c65eaaf043c`**、
  binary **`5baae6ebe438bc0205968587a7447599a15832411aeab1558cfecc1dd7293031`**。
  final binary は `/opt/r1/target/current-recovery/release/solvers`。
- AMD EPYC 7B12、8 logical CPU、同一 boot `159efb96-10fb-4ce4-bb0f-bc2b27ee618f`。
  baseline は比較用の新build、candidate はCPU変更後の新recovery buildへhashで対応づけた。
  [build検証](../saved-profile/vm06-build-verification.py) の歴史的role名
  `source06_solvers_not_paired_benchmark` は source03 比較で未使用だった意味であり、今回はそのbinaryを明示選択して測定した。
  破損した旧build stage12/13は引き続き認定から除外する。
- DCFR / F32 / 8 threads / no rake / chip EV / Full保存、pot 20 / effective stack 60。
  元のbaseline pilotを再使用し、追加pilotも品質target変更も行っていない。
  旧source03 planからの差分はcandidate identity、固定configの配置path、作成時刻だけ。
  config bytes、反復100 / 100 / 50、`NashConv < 0.04 chips`、check間隔、木・range・順序・resource boundsは同じ。
- River / Turn / Flop 各baseline→candidateを3回、全18 solve。公開木は27 / 1,305 / 21,618 nodes、
  保存node数は10 / 580 / 14,410。Flopは狭いrangeと後続streetのcheck-downを使う合成条件。
  [固定planとconfig](evidence-vm06-current/records/current/frozen/plan.json) を参照。
- 性能比較は2026-09-25 **18:27:46–18:30:05 UTC**、保存後auditは **18:30:09–18:30:22 UTC**。
  採用した性能stage間に重複はなく、source03比較とbuildは終了済み、保存後auditは性能比較の後に実施した。
  cacheはwarm / uncontrolled。1台・各版3回の範囲であり、母集団全体への有意差を主張しない。

## 保存済み戦略の品質

保存前 `run.json` のgain・NC・反復をsummaryと照合し、全9組でsummaryのEV・gain・NCと、
全保存nodeのtree / strategy / EV exportがbyte hashまで一致した。
保存後はu16 policyを全nodeへ復元してEV / BRを計算し直した。保存前metadataを品質判定の代用にしていない。

| ケース | 反復 | 保存前NC（両版一致） | 保存後NC（両版一致） | 保存後 `NC / (2 × pot) × 100` |
|---|---:|---:|---:|---:|
| River | 100 | 0.02975005890 | 0.02974775824 | 0.074369% |
| Turn | 100 | 0.02882798513 | 0.02883824007 | 0.072096% |
| Flop | 50 | 0.03669892417 | 0.03670024872 | 0.091751% |

18件すべてで値は有限、`gain = BR − EV`、`NC = gain₀ + gain₁ < 0.04`。
全9組で保存後EV / BR / gain / NCが **完全一致**し、近似一致の許容差は不要だった。
保存前と保存後の小差は量子化後の再評価として別に記録する。
[全audit結果](evidence-vm06-current/records/current/audits/audits.json) と
[固定audit plan](evidence-vm06-current/records/current/audit-plan/plan.json) は元artifactへhashで対応する。

baselineは元TOML、candidateは正規化TOMLを保存するため、raw config hashは全9組で不一致。
既知fixtureの明示defaultを補った意味・木・range・経済条件は一致し、未知fieldは拒否した。
各artifactのembedded config BLAKE3はそのrun.tomlに一致する。BLAKE3はVM上の独立した
pinned `b3sum` のcommand / binary / 入力SHA-256 / stdoutを照合したもので、offlineでBLAKE3を再演算したとは主張しない。

6回のcheckpoint resumeもsummaryと全profileが一致した。これは完了済み反復上限を復元する追加反復0の検証であり、
途中中断からCFRを続行した証拠ではない。

## 全工程時間とメモリ

時間はsupervisorのprocess生成前からroot回収・子孫消滅までで、初期化、CFR、品質確認、checkpoint、`.sol` exportを含む。
表は中央値 `[最小, 最大]`、各版n=3。負の変化率は削減。「中央値の比」と「同じ反復番号の対ごとの比」は別に計算する。

| ケース | baseline 秒 | candidate 秒 | 中央値の変化 | 対ごとの変化%：中央値 `[最小, 最大]` |
|---|---:|---:|---:|---:|
| River | 0.254 `[0.254, 0.265]` | 0.253 `[0.206, 0.258]` | -0.4% | -0.4 `[-22.4, +1.4]` |
| Turn | 2.187 `[2.149, 2.263]` | 1.915 `[1.846, 1.951]` | -12.4% | -13.8 `[-15.6, -10.9]` |
| Flop | 12.133 `[11.843, 13.059]` | 10.149 `[10.105, 10.312]` | -16.4% | -15.0 `[-22.6, -14.3]` |

メモリはMB = 1,000,000 bytes。root OS peakはLinux `wait4.ru_maxrss`、sampled treeは
観測時点のprocess tree RSS合計の最大値であり、異なる指標である。

| ケース / 指標 | baseline MB | candidate MB | 中央値の変化 | 対ごとの変化%：中央値 `[最小, 最大]` |
|---|---:|---:|---:|---:|
| River / root OS peak | 25.6 `[24.4, 25.7]` | 25.6 `[25.1, 25.8]` | +0.0% | +0.5 `[+0.0, +2.6]` |
| River / sampled tree | 11.0 `[11.0, 11.0]` | 10.7 `[10.6, 10.8]` | -2.6% | -2.1 `[-3.5, -2.1]` |
| Turn / root OS peak | 51.4 `[51.1, 51.5]` | 36.6 `[36.6, 36.6]` | -28.8% | -28.8 `[-29.0, -28.5]` |
| Turn / sampled tree | 51.7 `[49.7, 51.8]` | 34.6 `[33.0, 35.6]` | -33.0% | -31.3 `[-36.1, -30.3]` |
| Flop / root OS peak | 731.2 `[731.2, 731.8]` | 438.5 `[433.5, 450.4]` | -40.0% | -40.0 `[-40.7, -38.5]` |
| Flop / sampled tree | 731.9 `[731.7, 732.3]` | 436.4 `[430.6, 448.7]` | -40.4% | -40.4 `[-41.2, -38.7]` |

root peakにはfork/exec前や回収子孫のhigh-water値が含まれ得る。約25–27 MBのfloorが見える
Riverや短いsummaryの小差は、solver自身のメモリ差として解釈しない。
sampled peakは短いpeakを見逃す下界。Flopでは両指標とも約40%削減で同方向だった。

## summary読込みと保存サイズ

| ケース | baseline summary ms | candidate summary ms | 中央値の速度比 | 対ごとの時間変化%：中央値 `[最小, 最大]` |
|---|---:|---:|---:|---:|
| River | 23.455 `[19.491, 26.538]` | 22.647 `[20.876, 23.978]` | 1.04倍 | -9.6 `[-11.0, +16.2]` |
| Turn | 47.118 `[45.231, 56.704]` | 18.395 `[18.093, 22.887]` | 2.56倍 | -61.0 `[-68.1, -49.4]` |
| Flop | 587.550 `[555.197, 661.986]` | 29.023 `[20.342, 32.368]` | 20.24倍 | -95.1 `[-96.3, -95.1]` |

Flop summaryのroot peak中央値は237.0 → 26.4 MBだが、candidateは上記floor付近。
candidate summary全9回とbaseline River全3回ではprocessをサンプルで捕捉できず、生のsampled peakは0だった。
**0は未捕捉であり、0 RAMではない**。reportではそのメモリ変化率をnullに保ち、−100%とは計算しない。

| ケース | baseline `.sol` bytes | candidate `.sol` bytes | 中央値の増加 | 対ごとの変化%：中央値 `[最小, 最大]` |
|---|---:|---:|---:|---:|
| River | 7,472 `[7,472, 7,473]` | 7,609 `[7,609, 7,609]` | +1.8% | +1.8 `[+1.8, +1.8]` |
| Turn | 21,335 `[21,333, 21,335]` | 24,920 `[24,919, 24,921]` | +16.8% | +16.8 `[+16.8, +16.8]` |
| Flop | 131,744 `[131,743, 131,744]` | 176,833 `[176,833, 176,833]` | +34.2% | +34.2 `[+34.2, +34.2]` |

`.sol` v3は部分読込みのためのindex / frameを持ち、圧縮単位変更とのtradeoffがある。
この小さいFlop artifactでは約45 KB増えた。圧縮サイズが全条件で減るという主張はしない。
checkpointはRiver81,323→81,330、Turn356,281→356,288、Flop34,538→34,545 bytes（各+7）だった。

## 証拠範囲と再検証

4つのretentionを横断し、**全2,071 payload**のsize / SHA-256とcompact copyを照合した。
元18 solve・export・6resume、独立BLAKE3計測・18auditを合わせ**175 supervisor stage**の正常終了、
cleanup、input identity維持、stdout / stderr / sample記録、固定条件を確認した。
今回bundleは943 payload / skip0、SHA-256 **`cb4ed8a32bc5ce5b67ebbd18dd17e00262bbd823207f82270a6a92f5ca47a4ee`**。
[retention](evidence-vm06-current/retention.json) と [compact index](evidence-vm06-current/compact-index.json) が
各大artifactのrawbundle内location、hash、availabilityを持つ。

rawbundleはignoredな `runs/r1-cloud/` に保持する。新bundleに加え、`vm06-v3-results.tar.gz`、
`vm06-saved-audits.tar.gz`、`vm06-recovery.tar.gz` とbaseline / source03 / source06 archiveが必要。
Git-backedなcompact記録だけで巨大exportを再hashしたとは主張しない。

```text
python experiments/hu-postflop-r1/pipeline/current-verification.py --out runs/current-reverified.json
```

出力先は新規pathが必要。validatorは保存binaryや保存scriptを実行せず、rawbytesをhashし、
sourceとbuildは既存のscoped build verifierで検査する。reportのruntime gapはPython / Rust / Cargo本体の未回収で、
記録されたhashは維持するが、本体byteの独立再hashはできない。generic retainerの`ready=false`を成功へ書き換えず、
この比較に必要な参照だけを横断解決する。

元campaignのphase値はnullのまま維持し、全工程時間から各phaseを推計していない。
別の計測専用phase campaignと、この無計測binaryの結果は分離する。
今回の証拠は3つのFull / F32 / chip-EV合成fixtureの品質・IO・資源比較であり、
他game・一般range・NoRivers・24外部参照条件の受入には拡張しない。
