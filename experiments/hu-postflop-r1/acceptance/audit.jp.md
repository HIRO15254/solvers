# R1：事前条件と受入範囲の監査

3つの合成fixtureについて、比較前に固定された内部品質条件と完全一致条件が存在する。
最終source06の比較はその条件を維持し、保存後の品質と時間・メモリの改善を確認できた。
一方、外部参照の条件照合と許容差の校正を含む正式なT1-06認定は、この証拠だけでは成立しない。
**総合 `quality_status=not_evaluated`、R1全体の完了は未認定**とする。

この文書と[criterion-index.json](criterion-index.json)は測定後の証拠索引である。
今回発行した索引を比較前の`threshold_version`として遡及させない。

## 比較前に実際に固定されていた条件

| 記録 | UTC | 根拠 |
|---|---|---|
| baseline-only pilot作成 | 2026-09-25 16:21:07 | [元pilot](../pipeline/evidence-vm05/pilot.json) |
| 初回比較plan固定 | 2026-09-25 16:22:24 | [元plan](../pipeline/evidence-vm05/plan.json) |
| 初回candidate比較開始 | 2026-09-25 16:24:08 | [元comparison](../pipeline/evidence-vm05/comparison.json) |

source02 archive `5edec6be…b4b8fc6`のREADME、runner、3設定を原文で選定保存した。
runner SHA-256 `8876065d1cc89ba94064dc2d3da4832059aac49ddab153d3688ab6711b6e28ca`は
pilotとplanの記録に一致する。原文は[元snapshotのREADME](predeclared-source02/README.md)と
[source manifest](../validation/updated/source-manifest.json)へ結び付けた。

- 3つの有限・no-rake・chip-EVゲーム、pot20 / stack60、F32 / 8threads / Full保存。
- 厳密な内部条件は`NashConv < 0.04 chips`。この定和ゲームに限り開始pot比の
  `Exploitability < 0.1%`と等価。CLI既定値や、外部EV許容差を校正した意味ではない。
- baseline pilotの停止反復に合わせた100 / 100 / 50反復。対象・target・check間隔は変更しない。
- 各ケースbaseline→candidateを3回、全18 solve。summaryの指定fieldと保存nodeの
  tree / strategy / EV export bytesは完全一致を要求。公開exportのzero-own-reach行の省略限界は維持する。
- timeout/resource stopを成功扱いせず、全工程時間・process peak・summary・容量を別々に記録する。

source03から最終source06へはcandidate identity、固定configの配置path、作成時刻だけが変わり、
元pilot・baseline・条件とconfig bytesは同じだった。[最終report](../pipeline/current-report.md)の18件と
別途事前freezeした保存後audit18件では、全9組の保存policy EV/BR/gain/NCが完全一致し、NCは0.04未満。
保存後の判定を保存前metadataで代用していない。

この事実は限定した比較の適格性を支える。しかし原runnerも総合`quality_status`を`not_evaluated`に保っており、
外部参照の総合受入を行った記録ではない。ロードマップの0.1% / 教師0.02%という暫定候補を
一般条件へ一律採用したとは扱わない。時間やメモリの最低改善率は事前に定めていないため、
観測した改善率から後付けの合格値を作らない。

## 要件ごとの証拠と未認定範囲

| 要件 / 作業 | 支えられる観測 | この索引で認定しない範囲 |
|---|---|---|
| F1-01 / T1-01 | 独立HU oracle回帰、取得したRiver診断記録 | 日常/拡張の外部fixture全条件、外部許容差、24候補の数値受入 |
| F1-02 / T1-02/03 | 共通境界設計とsource06のStud/Draw/split/utility 11testの成功 | 全variantの実用solverや任意情報構造の汎用compiler |
| F1-03 / T1-04 | recall・観測・写像の境界、不可逆な記憶喪失の拒否 | 別の粗密抽象化成果物と受入を自動認定しない。独立レビューへ接続する |
| F1-04 / T1-05 | source06全工程・保存profile、source03の別phase計測、6回の復元、HU 3→6反復test | phaseごとのRAM、一般的な中断後継続の同値性、通常Flop全範囲 |
| F1-05 / T1-06 | 事前に固定された合成fixture基準と現在の証拠索引 | 外部校正版、対象全体・品質・費用を含む正式な総合認定 |
| F1-06 / T1-07 | 自己完結入力・成果物の回帰、summary時間・RAM・保存容量の実測 | 全条件でのサイズ減少。今回の`.sol`容量は増加している |
| F1-07 / T1-08 | source06で同じ品質の時間・RAM比較、全9組の保存後値完全一致 | 他ゲーム・一般range・NoRivers等への外挿、母集団全体への有意差 |

具体的な参照hashと根拠を[索引](criterion-index.json)の`requirement_evidence`に固定した。
これはLinearの各作業の進捗表ではなく、証拠の範囲を整理したもの。

## checkpointの追加反復とR1 / R2の境界

計測campaignの6回のresumeは完了済みの反復上限から復元するもので、追加反復は0。
それとは別に、[postflop_run_reuse.rs](../../../crates/cli/tests/postflop_run_reuse.rs)の
`resumed_early_stop_evaluates_the_new_iteration`はraked River F32を3反復で早期停止し、
resumeで6反復へ進める。checkpoint eventは`[3,6]`となり、再構築・復元したsolverとの
EV / BR / 全保存policy量子化値を照合する。
[source06のworkspace test出力](../validation/vm06-source06/checks/03-workspace-test/stdout.log)で成功を確認した。
`final_partial_chunk_reuses_checkpoint_and_preserves_results`と、F32/I16のborrowed checkpointが
既存wire payloadと一致する2testも成功している。

この3→6testは外部signalによる中断でも、同じK反復の一気通貫solveとの同値比較でもない。
`resume_equivalence_kuhn`は成功しているがKuhnであり、NLHEの代用にはしない。
signal付きの`a_canceled_heads_up_solve_closes_as_canceled_and_resumes`もKuhnを使い、通常testではignored。

[R1票](../../../docs/plans/r1-execution-plan.jp.md)と[全体計画](../../../docs/plans/solver-implementation-plan.jp.md)の
F1-04/T1-05は保存・再開後の値と資源・roundtripを求める。
長時間jobの監視・中断・再開という運用の受入はF2-04/T2-04に明記される。
追加反復を伴う証拠が皆無とは言わず、今回の限定的な回帰成功と、R2の運用範囲を分ける。
必要な追加証拠は、事前固定した小River / F32・I16で、同じK反復の一気通貫と
途中の協調停止→再開のfull state / profile / EV / BRを照合し、停止・cleanup記録を保持する試験である。
この提案自体を新しいR1終了条件へ黙って追加しない。

## 次の基準発行に必要なこと

外部参照の両range、全後続木、utility/rake、単位・EV基準、表示丸め・solution精度が揃うcaseから、
baselineのみで数値誤差と参照差の性質を確認する。外部EV許容差・必須check・適用scope・
不等号・欠測処理・validator hashを**そのcandidate比較前**に発行する。
一般和へ零和用pot比を流用せず、seat gain / NashConvを別に扱う。
既存の未知項目は`null` / `not_evaluated`に残す。

将来の追加記録はこの索引へhash付きで添付できるが、既存runや判定は上書きしない。
新しい正式な受入判断には別の版とレビューが必要である。
