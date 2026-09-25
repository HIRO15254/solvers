# HU Postflop R0 → R1 引継ぎ（R0-06 / SOL-6）

この文書はR0で決めた入口、未決事項の所有作業と判断時期を示す。票の状態・担当・実行時のblocked-byは[Linear管理先](../../status.jp.md)を正本とする。2026-09-25の受入照合と版は[readiness報告](../../../experiments/hu-postflop-r0/readiness/report-2026-09-25.jp.md)に記録した。R0は参照候補と測定・実行方法の準備であり、solver品質や全24件の比較成功を認定しない。

## R1へ渡す入口

| 作業 | 入力と最初の実行内容 | 完了前に必要な確認 |
|---|---|---|
| T1-01 参照取得・fixture | [24件の台帳](cases.csv)と[多様性・取得境界](coverage.md)を使い、各`source_url`から開始spotを再確認する。初回は`HU-R0-019`、次に`HU-R0-007`、`HU-R0-008`。両seatのcombo rangeと正規化、card removal、全Preflop/Postflop履歴、board、pot/dead money、effective stack、rake徴収条件とutility、全後続streetの合法menuと丸め、hand/action頻度・EV・基準点・表示精度・参照版を取得する。 | `same_game_candidate`は21件の候補区分にすぎない。`diagnostic_only`3件を含め全件の条件照合と欠測をcase IDで保存する。再現不能な条件は参考比較へ落とし、固定fixtureと独立oracleの不足ケースを作る。凍結`cfr-ref`本体は変更しない。 |
| T1-02 共通ゲーム境界 | [資産対応表](asset-map.md)の入力→tree→CFR/BR→保存→照会、private/public deal、情報集合、phase、legal action、utility/settlement、chance/card removalの既存境界をNLHEの型へ写す。 | 6max出自でもpostflop HUの現行rake adapterは`players_dealt/players_saw_flop=2`、stack入力は共通effective値。非対称stack、fold済みseat/dead money、private history、Stud/Drawの次元変化・split potを表現できる境界を検査し、必要な実装をT1-03/T2-01へ切り出す。 |
| T1-05 基準測定・ローカル実行 | [測定仕様](measurement-protocol.md)、[結果形式](result-template.json)、[source/host手順](provenance.md)、[manifest案](manifest-template.json)、[初回実行票](local-run-plan.md)を使う。T1-01の固定入力とT1-04の抽象化範囲が揃ったcaseから測る。 | **初回solveの前**に外部監視・停止器、事前tree/資源見積り、全工程の区間計測を小toyで較正する。source・binary・config・hostを実行前後で結び、最終BR、保存・読戻し、resumeを照合。初回は同時1件、threads 8、全工程600秒観測枠を実機直前値で再設定する。`tree.source`入力は事前`validate --write-effective`で正規化する。 |
| T1-06 閾値校正・受入版 | T1-01の参照精度と同一条件、T1-05のrun/資源・保存後の結果、T1-03/04の適用範囲を集める。[測定仕様 §5](measurement-protocol.md#5-レコードと判定)に従い比較開始前の版を発行する。 | 零和の`(g_0+g_1)/2`とrake入り一般和のseat別`g_i`・NashConvを分ける。0.1%/0.02% potは暫定候補。外部EV許容差、表示丸め、数値誤差、頻度診断、zero reach/欠測の扱いと不等号、validator hashを版に固定する。未校正は`not_evaluated`。 |

T1-01とT1-02は並行して開始できる。T1-03はT1-02、T1-04はT1-01/02/03、T1-05はT1-01/04、T1-06はT1-03/04/05を成果上の前提とする。[全体計画](../solver-implementation-plan.jp.md#5-r1-hu検証と汎用ゲーム境界)に従い、前段の全機能が後段の全作業の待ち条件だとは解釈しない。固定fixtureがまだない間も、T1-05の監視器・測定器のtoy較正とT1-02の境界設計は先行可能である。

## 未決事項、判断時期、進められる作業

| 未決事項 | 所有作業・必要時期 | その前に進められる作業 |
|---|---|---|
| raked cashのsolution別徴収条件、utility、両range、全menu、EV基準・精度。Single Size差、squeezeのdead money、6max出自の影響 | T1-01。各caseを`same_game`認定し数値比較する前。表現不能な入力はT2-01へ | 既存候補の取得、cEVを含むfixture設計、T1-02の境界監査 |
| 凍結oracleで未照合のrake、raise、Flop、iso、保存後profile | T1-01がadapter/fixtureを補い、T2-02/T2-03で数値・保存後を検証。各範囲の品質判定前 | 小既存oracleの再実行、T1-05の計測器較正 |
| 監視器・全工程停止、事前tree/メモリ見積り、測定用binaryとsolve直前空きRAM | T1-05。初回本計算の前。`source`、`config`、binary、hostはcaseごとの実値で確定 | T1-01の参照取得、T1-02の設計、toyで監視器の試験 |
| exploitabilityの暫定0.1%/0.02% pot、外部EV許容差、頻度診断・参照丸め | T1-06。基準測定後、改造候補の比較開始前。一般和の閾値は別定義 | T1-05の観測値収集、条件差の記録。合否は保留 |
| GUIの初回範囲、相手profile/lockの意味と適用範囲 | GUIはT2-06の範囲決定時、profile/lockはT4-03〜T4-05の設計時 | R1のCLI fixture・測定・共通境界を進める |
| 1ノードaction EVを返す高速近似の評価方式、数分目標 | T3-00で方式・指標調査、T3-01以降の方式比較前に固定 | HU厳密CFRのR1検証。数分を厳密CFRの合否にしない |
| 有料クラウド費用と支出枠 | T1-05のローカル実測で必要性が出た後、有料実行前に現行単価で見積り・判断 | 通常規模のローカル測定と設計を進める |

## 実行時の注意

`run_status`と`quality_status`を分け、時間切れ・資源超過・参照条件不足を合格にしない。値の単位はchipまたはprize、比率は開始pot、時間は秒、peak process memoryはbyteとOS指標名を記録する。現行HUに公開`evaluate`とaction別Q値出力はないため、保存済みsummaryやnode EVをこれらの代わりにしない。過去の2例と小再現testは現在の24件の同一ゲーム比較や性能の証拠ではない。
