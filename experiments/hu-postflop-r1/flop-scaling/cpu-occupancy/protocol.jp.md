# Flop並列化のCPU時間・affinity診断

VM15のworker scratch候補は全state/qualityが一致したが、事前の10%短縮条件を満たさなかった。本実験は現行baselineだけを使い、16→32 workersで時間が伸びない要因を絞る診断である。新しい最適化や採否基準は導入しない。解の収束・外部参照品質の認定でもない。

## 固定入力とsource

- 現行solver SHA-256 `69063b31c3433b2eb4798b2f41015311200301a7ba1887b05bbefdb936d4e81a`。凍結oracleを変更しない。
- VM15と同じ2入力 narrow / expanded、全street bettingを含む367,662-node tree、DCFR/F32、chance depth2 / minimum children12、固定16反復。root supportは34/30、63/160 hands。
- 元adapter SHA-256 `63cba37a5b0f79dfed2ca0ddf879daada3f0fee094ee5f287f86bc1f37414d46`から、process CPU時計・個別品質walkのwall時計・timing外のaffinity観測だけを加える。呼出し順・演算・旧JSON・state bytesは保持する。具体的な全source/adapter/controlsは実行前package manifestへ固定する。
- 同一VM・同一bootでRust1.97.0 release/nativeの新規buildを行い、toolchain/Cargo.lock/binary/source/config/CPU topologyを記録する。過去VMのnative binary/cacheは流用しない。

## 条件と順序

親processは32 logical CPUのfull affinityで、guest topologyは16 core /32 logicalを要求する。同じcoreとは `(socket, core)` のID組が同じこと。guestの表示はhostの専有物理coreを証明しない。

各入力について元adapterの1 worker・16反復をcanonicalとし、CFRが4秒未満なら実験を終了する。反復数を結果に応じて増やさない。次にCPU adapterを1 worker・16反復で実行し、clock・状態・品質の較正を行う。この1組は時計による微小な性能摂動を統計的に推定する試験ではない。

診断は各入力×次の3条件×warmup1回と測定3回、計24件。canonical2件と較正2件を合わせ28 solves。条件・入力・roundの具体順序は実行前planに固定する。失敗標本の差替え、完了標本の選び直し、別bootへの継続を行わない。

| 条件 | workers | 子processのCPU affinity |
|---|---:|---|
| 16-full | 16 | 親と同じ32論理CPU |
| 32-full | 32 | 親と同じ32論理CPU |
| 16-onecore | 16 | 各(socket,core)で最小の論理CPU IDを一つ、計16個 |

最後の条件だけ子processにtasksetを適用する。開始・終了時の実affinityをCPU JSONで観測し、期待集合と厳密照合する。親の全32CPU・boot・resource条件は全stageで照合する。単一CPUモデルや単一VMから他機種へ一般化しない。

## 保存と診断値

全stageでglobal combo ID、反復数・次元header、F32 regrets/strategy_sum全streamと両席EV/BR/exploitability bitsをcanonicalと完全比較する。等価な重複stateは比較receiptをfsyncした後だけ削除する。CFV captureは行わず、CFV一致は本診断の検査範囲に含めない。2本のcanonicalはgzipを原本全bytesと照合して保持し、stage完了記録も永続化する。readerは必要なsource・controls・build・全28 stages・receipt・canonicalとそのpinsを確認する。全体未完了は未完了として残し、欠測を埋めない。

時間はCFRと公開品質評価7 traversalsを分け、個別EV/BR/exploitabilityのCPU時間とwall時間も記録する。後者のexploitability API内部3 walksは分割しない。各3標本の値、中央値、max/min、root-process OS peak RSSを記録する。構築・state出力・proof保存をsolver timingへ混ぜない。

`process CPU seconds / wall seconds` はprocess全threadの平均CPU消費量であり、spin・scheduler・allocator等も含む。高い比率だけで有用な並列処理や帯域飽和を証明せず、低い比率から逐次区間と待機・descheduleを区別しない。16-full /16-onecore /32-fullの差は配置に関する参考値であり、SMTだけの因果効果とも扱わない。allocation counter、chance depth変更、他CPUへの変更は混ぜない。

## 有限資源

重いbuild/solve/原本再検証はGCPだけで行う。2 vCPUでbootstrapと依存取得後、同じ停止VMを32へ変更する。buildは240秒、solveは90秒以下、process memory8 GiB、外側memory12 GiB/swap0、free memory2 GiB、disk reserve2 GiB。全体deadlineが先に来れば停止し、成功とは扱わない。

元のcloud STOPは起動要求から35分。実験deadlineは `min(dispatch+16分, STOP−15分)` で、dispatch時の残り実験枠が600秒以下なら開始しない。STOPや実験期限を延長しない。Spot回収後のsolve再開はせず、同じ期限内で2 vCPU回収のみを許す。回収archive上限256 MiB、全転送512 MiBを予約し、回収/hash確認後にVMと唯一の40 GiB diskを削除する。

費用は起動前に最新確認と台帳予約を行う。残$2を超える場合は起動しない。元の$1不確実性余裕、disk24時間、IP、転送を含める。使用量と請求確定は区別し、未請求をゼロ扱いしない。
