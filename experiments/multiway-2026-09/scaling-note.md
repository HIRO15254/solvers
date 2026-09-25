# 旧Multiway長時間運用調査から保持する結果

状態: **2026-09-09〜10の過去記録**。現在の作業・資源判断は[開発状態](../../docs/status.jp.md)に従う。
旧VM/boot diskの現在の存在、当時の料金、過去の予算から新しい実行を許可しない。

| 確認したこと | 限界と参照 |
|---|---|
| state4の24対8 vCPU trainingは55.090対61.247秒、speedup 1.112 | この測定からcore数に比例する高速化を期待しない。[state4](state4-boundary.md) |
| 12,297,431 blocks、2,171,445,586 bytesのsolutionを書き出した | metadataと先頭/末尾ページの検査であり全ページの再読込ではない。[round 5](multiway-convergence-round5-20260909/README.md) |
| regretとaverage traversalの並列化で同条件の数値・fingerprint一致 | 各条件1回の測定。stage間のsweep数が異なるため直接のthread scaling比較ではない。[詳細](multiway-convergence-round5-20260909/parallel-traversal.md) |
| 初期化・復元・snapshot・出力が大きな時間/メモリを消費した | 後続で[production初期化](production-initialization-20260910/README.md)・[checkpoint writer](checkpoint-write-20260910/README.md)・[compact drift](strategy-drift-20260910/README.md)を測定した。旧「次の改善」を現行taskとしない |
| Draw-aware/EHSはseed 0の限定比較で平均weighted MAE/RMSEが0.141068/0.268747対0.115510/0.241222 | candidateのtable準備57.722秒対control 0.973秒。cold cache/harnessを含む単一seedで、品質認定やtraining-onlyの速度比較ではない。追加実験は停止した |

資源評価で残す注意点は、policy arena payloadとprocess全体のRAMを分けること。
worker scratch、driftの保持容量、checkpoint/solution構築時の同時allocation、allocatorが
保持する領域はarena上限だけでは評価できない。phaseごとの時計・process peakと出力同一性を使う。
compact driftのcapacity削減79.16%もprocess lifetime peakの同率低下を意味せず、実測peakはほぼ不変だった。

再開用checkpointと閲覧用solutionの役割、保存状態の同一性を区別する。
checksumは保管同一性の検査であり、ゲーム上の正しさや戦略品質の証明ではない。
ベンダーのiteration/nodeや所要日数を、このsolverのsweep・品質保証へ換算しない。
旧クラウド実行の費用・運用順・現在形のVM記述はここから除き、現行計画と混同しない。
