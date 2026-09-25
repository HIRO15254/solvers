# VM06：source03 / `.sol` v3 と baseline の比較

固定した3つの合成ゲームでは、保存済み戦略の品質を保って Flop の全工程時間とメモリが減った。
Flop の中央値は **12.225 → 10.150秒（−17.0%）**、root の OS peak は
**731.5 → 449.6 MB（−38.5%）**。summary 読込みは **0.588 → 0.0227秒（25.9倍）**だった。
一方、`.sol` は **131,744 → 176,834 bytes（+34.2%）**に増えた。
River / Turn の時間差は反復間の変動が大きく、安定した高速化とは結論しない。

評価は **限定条件付きで共有可能**。これは source03 の性能測定であり、source06 の最終 binary の
性能測定を代用しない。外部参照24条件の一致・受入も認定しない。
数値と検査結果は [機械可読 report](vm06-comparison-report.json)、再計算は
[独立 validator](vm06-comparison-verification.py) を正本とする。

## 条件と証拠

- baseline は `9632d8b`、source archive SHA-256 `fdd8c1c0…33a0197`、`.sol` v1。
  比較候補は source03 `ac5d493a…dd89970`、binary `a74e873b…fb0521`、`.sol` v3。
  完全な hash と build の対応は report の `source_scope` にある。
- AMD EPYC 7B12 / 8 logical CPU、同一 boot `159efb96-10fb-4ce4-bb0f-bc2b27ee618f`。
  Spot 再起動前の Intel build cache は使わず、CPU・boot・source・compiler の build 前後記録を照合した。
- DCFR / F32 / 8 threads / no rake / chip EV / Full保存、pot 20 / effective stack 60。
  River / Turn / Flop の反復上限は baseline pilot により100 / 100 / 50に固定し、
  `NashConv < 0.04 chips`、check間隔、木・range・並列条件を変更していない。
  各ケース baseline→candidate を3回、全18 solve。OS cache は warm / uncontrolled。
- 公開木は River 27 nodes（10保存）、Turn 1,305（580保存）、Flop 21,618（14,410保存）。
  Flop は狭い range と後続 street の check-down を使う合成条件である。
  [固定 config / plan](evidence-vm06-v3/records/performance/pipeline/frozen/vm06-v3/plan.json) を参照。
- 性能比較は2026-09-25 **18:04:58–18:07:16 UTC**、保存後 audit は **18:12:53–18:13:06 UTC**。
  採用した性能 stage の時間区間に重複はなく、audit の実行時間を性能測定へ混ぜていない。

3つの retention を横断して全1,128 payloadの size / SHA-256、compact copy、stage recordと出力、
固定条件・source・binary の参照を検証した。全18 solve、各 export、6回のresumeと各出力、
BLAKE3計測、保存後auditを合わせ175 supervisor stageが正常完了し、cleanup / identity維持を確認した。
generic retainer の `ready=false` は変更していない。別 bundle に置かれた必要参照を解決した scoped 検証であり、
OS上の Python / Rust / Cargo の記録hashに対応する実行ファイル本体はローカル回収されていない。

## 保存済み戦略の品質

保存前は `run.json` の最終 gain / NC と artifact summary が一致し、全9組で
summary の EV・gain・NC、および全保存ノードの tree / strategy / EV export の全 byte hash が一致した。
export の対象は元runのartifactであり、resumeは完了した反復上限の復元のみ（追加反復0）で6件とも一致した。
中断途中からの学習再開の検証ではない。

保存後の品質は、保存前メタデータとは別に u16 policy を全ノードへ復元して EV / BR を再計算した。
評価用 source06 は共通 helper を baselineへ移植した v1 reader と、v3 reader の2 binaryを使う。
その [recovery build検証](../saved-profile/vm06-build-verification.py) は成功した新8 stageだけを採用し、
破損が残る旧buildのstage12/13を除外している。source06 auditorのload時間を性能差としては採用しない。

| ケース | 保存前NC（両版一致） | 保存後NC（両版一致） | 保存後 `NC / (2 × pot) × 100` |
|---|---:|---:|---:|
| River | 0.02975005890 | 0.02974775824 | 0.074369% |
| Turn | 0.02882798513 | 0.02883824007 | 0.072096% |
| Flop | 0.03669892417 | 0.03670024872 | 0.091751% |

保存後18件すべてで有限の `gain = BR − EV`、`NC = gain₀ + gain₁ < 0.04` を再計算した。
9組は EV / BR / gain / NC の数値が **完全一致**し、近似一致の許容差は使う必要がなかった。
保存前と保存後の小差は量子化後の再評価として分離している。
[audit記録](../saved-profile/evidence-vm06-audits/records/audits/audits/audits.json) と
[固定audit plan](../saved-profile/evidence-vm06-audits/records/audits/plan/plan.json) が各artifactへの対応を持つ。

baseline は元のTOML、candidate は正規化TOMLを保存するため、**raw config hash は全9組で不一致**。
既知fixtureの明示的default補完後の意味・木・range・反復・target・経済条件は一致し、未知fieldは拒否している。
各artifactのembedded config BLAKE3はそのrun.tomlに一致する。VM上の独立した `b3sum` の
binary identity・入力SHA-256・command・stdoutを検証したものであり、ローカルでBLAKE3を再演算したとは主張しない。

## 全工程時間とメモリ

時間は supervisor のprocess生成前からroot回収・子孫消滅まで。初期化、CFR、品質確認、checkpoint、
`.sol` exportを含む。中央値 `[最小, 最大]`、各版 n=3。変化率は負が削減を表す。
「中央値の比」と「同じ反復番号の対ごとの比」は別に計算した。

| ケース | baseline 秒 | candidate 秒 | 中央値の変化 | 対ごとの変化%：中央値 `[最小, 最大]` |
|---|---:|---:|---:|---:|
| River | 0.196 `[0.192, 0.253]` | 0.194 `[0.188, 0.195]` | −0.9% | −0.5 `[-25.8, +1.3]` |
| Turn | 2.096 `[1.667, 2.224]` | 1.975 `[1.841, 2.207]` | −5.8% | −0.8 `[-5.8, +10.5]` |
| Flop | 12.225 `[12.156, 12.629]` | 10.150 `[10.144, 10.511]` | −17.0% | −16.5 `[-19.6, -14.0]` |

メモリは MB = 1,000,000 bytes。root OS peak は Linux `wait4.ru_maxrss`、sampled tree peak は
supervisorの観測時点におけるprocess tree RSS合計の最大値であり、同じ指標ではない。

| ケース | root peak：baseline MB | root peak：candidate MB | sampled tree：baseline MB | sampled tree：candidate MB |
|---|---:|---:|---:|---:|
| River | 25.924 `[24.584, 25.940]` | 25.928 `[25.244, 25.985]` | 11.088 `[11.059, 11.158]` | 10.387 `[10.318, 10.748]` |
| Turn | 51.208 `[51.159, 51.462]` | 36.880 `[36.516, 36.901]` | 51.823 `[51.741, 51.847]` | 34.623 `[31.986, 36.217]` |
| Flop | 731.521 `[730.993, 732.402]` | 449.589 `[436.871, 450.327]` | 732.164 `[731.619, 733.188]` | 444.740 `[434.782, 445.841]` |

root peakの中央値は Turn −28.0%、Flop −38.5%。Flopはsampled treeでも−39.3%で同方向だった。
root peakにはfork/exec前や回収子孫のhigh-water値が含まれ得るため、約25–27 MBのfloorが見える
Riverや短いsummaryの小差は実際のsolverメモリ差として解釈しない。sampled peakは短いpeakを見逃す下界である。

## summary読込みと保存サイズ

| ケース | baseline summary ms：中央値 `[最小, 最大]` | candidate summary ms：中央値 `[最小, 最大]` | 中央値の速度比 |
|---|---:|---:|---:|
| River | 18.290 `[17.731, 21.356]` | 18.565 `[17.963, 19.385]` | 0.99倍 |
| Turn | 45.034 `[40.852, 53.104]` | 22.767 `[18.562, 22.768]` | 1.98倍 |
| Flop | 587.979 `[543.125, 590.702]` | 22.715 `[21.288, 30.430]` | 25.9倍 |

Flop summaryのroot peak中央値は236.7 → 26.7 MBだが、candidateは上記floor付近にある。
candidate summaryは全9回でprocessがサンプル時点に存在せず、sampled tree peakの生記録が0だった。
**0は観測を逃した値であり、RAM使用量0ではない**。validatorはそのメモリ削減率をnullに保つ。

| ケース | `.sol` baseline bytes：中央値 `[最小, 最大]` | `.sol` candidate bytes：中央値 `[最小, 最大]` | 変化 | checkpoint bytes baseline → candidate |
|---|---:|---:|---:|---:|
| River | 7,473 `[7,473, 7,474]` | 7,607 `[7,605, 7,609]` | +1.8% | 81,323 → 81,330 |
| Turn | 21,337 `[21,335, 21,337]` | 24,919 `[24,919, 24,921]` | +16.8% | 356,281 → 356,288 |
| Flop | 131,744 `[131,743, 131,745]` | 176,834 `[176,833, 176,835]` | +34.2% | 34,538 → 34,545 |

3ケースすべてで`.sol`の保存サイズは増加している。summaryアクセスとFlop実行資源の改善には、このサイズ上の費用がある。
ここに示した3回の範囲だけから、他のrange、木、I16、ICM、rake条件への性能を一般化しない。
通常runの内部phaseはnullのままであり、後続の別phase実験やsource06実測を、この結果へ推計で埋め込まない。

## 再検証

元bundleは `runs/r1-cloud/` に保持する。大artifact / exportはGitに含まれない場合もあるが、
各 [性能retention](evidence-vm06-v3/retention.json)、[audit retention](../saved-profile/evidence-vm06-audits/retention.json)、
[build retention](../saved-profile/evidence-vm06-build/retention.json) のVM path・member・size・hashから解決する。
retentionの再選定でcompact配置が変わっても、採用する元runやartifactのbyteは変更しない。

```text
python experiments/hu-postflop-r1/pipeline/vm06-comparison-verification.py --out runs/r1-cloud/vm06-recheck.json
```

出力先は未存在のファイルを指定する。solverや保存されたscriptは実行しない。
必要bundle欠測、hash不一致、対象run欠測、条件変更、品質不合格、cleanup不全は成功に置き換えずerrorにする。
このreportは3つの固定合成ゲームのsource03比較を検証するもので、Linearの作業状態を管理する文書ではない。
