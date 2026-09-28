# VM20: 単一 CPU profile の回収と検証

2026-09-28 UTC の実験。原キャンペーンは **failed** のままであり、16/32 worker の性能比較は成立していない。回収した既存 `perf.data` の追加読取りにより、**narrow・16 worker・N64・round 0 の1件**について物理 leaf の排他的集計を検証できた。新しい solver 実行・`perf record` 実行は各0件。本番への変更採用も0件である。

[独立レビュー](posthoc-result-review.json)は、小さな SDK 記録・source・解析報告・archive manifest の62入力を固定し、割合の計算、転送記録、削除を照合している。完全な状態ストリームや native binary の検証は GCP 上で実施し、このレビューではローカル native 実行・archive 展開・cloud API を行っていない。

## 原実験と失敗の境界

VM19 の固定 deployment を変更せず、新しい VM20 上で build と core tests を実行した。過去の VM19 binary/proof の流用ではない。[template review](template-review.md)参照。VM20 は同じ instance ID `2562385330130146252` のまま `e2-standard-2 → e2-highcpu-32 → e2-standard-2` と変更した。

build wrapper は exit 0。測定では perf preflight と narrow/expanded の canonical 2件、計3 stage が completed になった。最初の `narrow-r0-w16` は solver child が完了したが、後続の text decode が失敗し、残り7 profile stage は skipped になった。原 `script.stdout.log` は63,286,615 bytes、`dump.stdout.log` は71,438,336 bytesで、後者が64 MiB上限を超えた。proof 全体も301,460,126 bytesとなり240 MiB上限を超え、原 `retained.json` は存在しない。[原実行記録](measure-status03.stdout.log)、[部分メタデータ](partial-metadata01.stdout.log)参照。

追加 reader は別ディレクトリへ出力し、既存 decoder の失敗や原証拠を書き換えていない。元の137ファイルと `perf.data` を終了時にも再照合し、変更なしを確認した。demangle・inline・callchain の表示を無効にした既存データの読取りは、外側120秒・2 GiBの制限内で5.245秒、exit 0で終了した。終了後の `MainPID=0`、空の `ControlGroup`、`ActiveState=inactive` を確認してから archive を作成した。[制御レビュー](posthoc-control-review.json)、[実行](posthoc01.result.json)、[停止確認](posthoc-quiescence01.stdout.log)参照。

## 回収できた観測

CFR 区間・対象 PID の sample は3,687件、profile 全体は3,892件、差205件は集計対象区間/PID外である。CFR の period 合計は38,010,307,986。61個の排他的 leaf の各々で period/sample が10,309,278と一致し、この記録では sample 比率と period 比率が一致した。分母には unknown 717件も含める。

| 物理 leaf の正確な分類 | samples | CFR 内割合 |
|---|---:|---:|
| unknown libc leaf | 717 | 19.4467% |
| `PostflopEvaluator` の `TerminalEvaluator::eval` | 583 | 15.8123% |
| `holdem::kernel::compact_compat_sums` | 558 | 15.1343% |
| `engine::solver::cfr_pass` の物理 symbol | 492 | 13.3442% |
| `engine::storage::normalize_columns` | 259 | 7.0247% |
| `crossbeam_epoch::with_handle` の `rayon_core` specialization | 179 | 4.8549% |
| kernel `__pv_queued_spin_lock_slowpath` | 155 | 4.2040% |
| その他の物理 symbol | 744 | 20.1790% |

正確な mangled symbol と互いに重ならない集合は[独立レビュー](posthoc-result-review.json)に保存した。Rayon の generic producer/join/job symbol はインライン化された solver の処理を含み得るため、Rayon 全体を overhead と扱わない。unknown libc を allocation・copy・lock に帰属させる根拠もない。kernel spin の sample だけから発生元のアプリ処理は特定できない。これらは on-CPU sample の所在であり、経過時間の割合や削除可能なコストではない。

当該1件の CFR wall は2.431898761秒、process CPU は38.218711726秒、比は15.715585。これは単一実行の CPU 使用の観測であり、1 worker に対する speedup ではない。32 worker profile、expanded profile、反復比較は欠けているため、比例スケール・性能改善・並列化の原因特定には使えない。off-CPU、帯域、cache、callchain の完全性も判定していない。

## 同一性と品質の検証範囲

固定 reader の plan/build/measurement/preflight/canonical 検査に加え、failed profile の完了済み child supervisor、32 logical CPU の runtime、16 worker、affinity、制限時刻、binary/perf identity を検査した。同条件 canonical と profile の全状態81,414,344 bytesおよび品質 JSON 670 bytesは一致した。同入力の一致を、外部参照との一致や exploitability 目標到達とはみなさない。

| 対象 | SHA256 |
|---|---|
| native binary（1,388,792 bytes） | `5e0e61d88bd40cac51979c4ace1fe20aacd2cef6cc233cd2a6c0a80cf5c94200` |
| 原 `perf.data`（1,167,196 bytes） | `c2d8d4739d7c50bde2fc591b98cd82d24f0b1ab75db43308b817e26daddbef7e` |
| 全状態 | `8adfe1abed9baeb61828a035e557c4011ad33fadaf264b7746018172d59095d1` |
| 全品質 JSON | `7af8b32289142b35c2d22087ceb22d998f55e7c4420418c74cc6b4d0142922f9` |
| 追加 reader（18,949 bytes） | `b3d128bd15ae6c0ca9bac70d2cdde9e92bfde68de75afdcb96e6959e840fed9e` |
| GCP 解析報告（43,797 bytes） | `f966862cf89a2c736816bb9611132d5d6204066402bc7f42e39132cda93d800e` |

[GCP 解析報告](posthoc-report.json)に source・plan・binary・状態・品質・sample census の対応がある。原 campaign の completed 判定を後から補完したものではない。

## 保全と削除

回収 archive は172,135,766 bytes、SHA256 `7831f67fb0cf528aeae045ee185162414fdfe4e334f5be0bfabd1dbeee4f5f0c`。645 payload filesと埋込 manifest 1件の計646 regular membersを持ち、256 MiB圧縮上限内である。原 failed stage の大きい plaintext を含む全137 proof filesと、別の `recovery/posthoc` を保存した。manifest の対応を原 inventory、追加 reader、追加報告と照合した。[GCP archive 記録](flop-cpu-profile-recovery01.json)、[member manifest](flop-cpu-profile-proof01.tar.gz.manifest.json)参照。

このディレクトリの `flop-cpu-profile-proof01.part00`〜`part03` の4分割を転送し、SDK終了後に連結ストリームのサイズ・SHAを照合した。[download-check.json](download-check.json)は0.64077秒で成功し、ローカル展開は行っていない。4つのpartもこの変更セットのGit保管対象に含める。小さな記録だけではarchive本体を復元できないため、partとmanifest・hashを一緒に保全する。

同一 instance の削除 operation `4620273976060693135` は **2026-09-28T05:01:08.496Z DONE**、当初の絶対 STOP `05:14:20Z` より791.504秒前。boot disk削除を指定し、05:02:03〜05:02:08Z の project `solvers-abstraction-20260723`・name prefix `solvers-r1-` の instances/disks/addresses照会はすべて空だった。disk ID `5203462124618404812` の作成から不在確認までの上限は1,961.355068秒で、24時間以内。原 STOP の延長はない。[削除 operation](cleanup-operations01.stdout.log)、[disk](disk-before-delete01.stdout.log)、[独立レビューの cleanup](posthoc-result-review.json)参照。

これらは実行・転送・資源削除の証拠であり、請求書や正確な請求額の確認ではない。費用予約の照合・返却は別の予算台帳で扱う。本報告による新たな実装採用や性能認定はない。
