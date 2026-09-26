# 外部参照の条件監査: 001 / 002 / 017 / 019

2026-09-25 UTCに、保存済み観測・診断・現行実装と下記の公式資料を再確認した。
これは条件監査の証拠であり、作業状態の台帳でも閾値の校正版でもない。
**4件とも、この監査による外部品質認定は行わない。** `condition_match=unverified`、
`quality_status=not_evaluated`、`acceptance=null`、`comparison_threshold=null`を維持する。
参照元のbyte identity、確認済み事実、条件付き推論、未取得項目は
[機械可読記録](reference-condition-audit.json)に分けた。元のobserved/configは変更していない。

## 確認した範囲

| case | 保存証拠で確認できること | この監査で認定しないこと |
|---|---|---|
| HU-R0-001 | [R0台帳](../../../docs/plans/hu-postflop-r0/cases.csv)は100bb、NL50 General、`Cash6m50zGeneral`、Flop `Ks7h2d`、5%・cap 4bbの候補。 | 台帳のhistorical_evidenceは同一ゲームの証明ではない。両range・全継続木・精算・当該版の品質の照合。 |
| HU-R0-002 | [observed](HU-R0-002/observed.json)は100bb NL500 General、River `Ks7h2d3c8d`、pot 5.5bb、両者残97.5bb。両range原文と132 decision menus、261 terminal edges、393 public nodesの観測graph閉包を保存。 | graph閉包はterminal utility・全hand policyの証明ではない。この監査は新しいVM実行結果の認定を含まない。 |
| HU-R0-017 | [observed](HU-R0-017/observed.json)は75bb NL500 Simple、River `KhKc5s2d3s`、pot 20.5bb、残65bb。28 decision menus。 | 一部の深い枝はmenuだけ取得し、strategy/EVが欠測。表示0%を厳密なzero reachと認定しない。 |
| HU-R0-019 | [observed](HU-R0-019/observed.json)は75bb NL500 Simple、River `Qs7h2c4d9s`、pot 40.5bb、残55bb。12 decision menus。previewの5%・cap 0.6bbとaccuracy 0.2–0.3%を保存。 | previewは個別Riverの残差・version・精算規則の証明ではない。全hand/action profileも未取得。 |

017/019は[source03の診断](vm06-river-report.md)で実exportの全menu/actor/pot等を照合済み。
表示root EVとの差が0.005bb未満だった事実も同報告の範囲である。この差の小ささから
レーキ規則・EV基準・参照精度を逆算したり、後から0.005bbを合格閾値にしたりしない。
observedの古い`missing_fields`文言だけで現在のmenu閉包を判断せず、後続の
`tree_observation`、002の`menu_integrity_summary`、実export照合の対象範囲を区別する。

## レーキ: 実装の事実と診断仮定

現実装の[terminal構築](../../../crates/holdem/src/postflop.rs#L1346)は
開始potと両者の開始後拠出の合計を`TerminalDescriptor.pot`に入れる。
[PercentCapRake](../../../crates/game/src/payoff.rs#L64)はそのtotal potへ率を掛けてcapを適用し、
[PayoffPipeline](../../../crates/game/src/payoff.rs#L172)はfold/showdownともnet potを分配する。
Riverではno-flop-no-dropによる免除はない。未call額をrake baseから除く処理や追加のchip丸めはない。
これは現runtimeの事実であって、旧GTO Wizard libraryの精算仕様を確認したものではない。

002のrootで2bbをbetし相手がfoldすると、現runtimeのbaseは`5.5 + 2 = 7.5bb`、
rakeは`min(7.5 × 0.05, 0.6) = 0.375bb`。未call額を除くmatched-pot方式を仮定すると
baseは5.5bb、rakeは0.275bbで、差は0.1bbになる。開始局面基準のbettorの持帰りは
それぞれ`7.5 − 0.375 − 2 = 5.125bb`と`7.5 − 0.275 − 2 = 5.225bb`。
同様にbet 8.5bb→foldではtotal方式はcap 0.6bb、matched方式は0.275bbである。
これらは観測menu上での条件付き算術例で、参照側の方式を推定するためのEV fittingではない。

017/019では開始potだけでそれぞれ`20.5 × 0.05 = 1.025bb`、
`40.5 × 0.05 = 2.025bb`とcap 0.6bbを超える。したがって、**両方式とも全fold/showdown
terminalで0.6bbを徴収し、追加丸め・免除・既徴収額の相違がない**という仮定の下でのみ、
matched/totalの違いがこの診断ゲームのpayoffに影響しない。この仮定では効用和も
開始pot−0.6bbで一定になるが、参照側の定和性や零和Exploitabilityまで認定しない。
fold免除などの相違を、このcap算術では取り除けない。

公式[How To Build Custom Solutions](https://help.gtowizard.com/how-to-build-custom-solutions/)
のmatched potとhand当たりcapの説明は、確認時の**Preflop Tree Builder → Solution Setup → Rake**
にある。現行custom builderの説明を、取得した旧General/Simple libraryの実装保証へ転用しない。

## 参照accuracyと表示精度

公式[解一覧](https://blog.gtowizard.com/status-and-info-about-our-solutions/)は6max Generalを
0.075–0.3% pot、Simpleを0.2–0.3% potと説明し、NL500の5%・cap 0.6bb/handを掲載する。
同ページの公開日は2021-03-29だが、本文の更新履歴と取得solutionへの版対応は未取得。
FAQにはGeneral主要spotの0.2%という記載もある。
[2022-08-09の更新告知](https://blog.gtowizard.com/multitabling-new-solutions/)はGeneralの多数spotの
再計算を説明する。系列名と記事日だけでは特定の取得nodeのsolve versionを固定できない。

公式[How Solvers Work](https://blog.gtowizard.com/how-solvers-work/)の一般定義は、各seatの
最適応答によるEV改善を平均し、開始potと比較するもの。
[Understanding Nash Distance](https://blog.gtowizard.com/understanding-nash-distance/)は
solution全体の指標であり、使われない枝は早く計算を止め、strategy/EVの精度が低くなり得ると説明する。
**推論:** 全体で小さい残差であっても、その値を小さいreachの枝へ条件付けた局所上限と同一視できない。
取得したRiverの開始range・木・精算・継続profile・残差の分母と版を特定する必要がある。

公式[AI Benchmarks](https://blog.gtowizard.com/gto-wizard-ai-benchmarks/)にはGeneral/Simpleの
事前CFR計算に加え、Riverをリアルタイムで0.1% Nash Distanceへ再solveするという説明がある。
これは採用した個別responseがその版・設定で完了したという証拠ではない。系列previewを無条件に
River boundへ変換することも、この0.1%を新たな参照誤差上限に採用することもしない。
002には低頻度枝が不正確かもしれないUI warningも保存されている。

0.01bbのEV表示刻みと0.1 percentage pointの頻度表示刻みは観測事実である。
正確な丸め規則・内部精度・収束残差とは別であり、半刻みの区間を校正済み許容差にしない。
自作の小fixtureに対するoracle誤差や保存量子化の回帰testも、任意の外部caseの数値誤差保証にはならない。

## 001の歴史的記録とEV基準

001の台帳がリンクする[2026-07のFlop記録](../../hu-postflop-reference/cases/ks7h2d-flop/README.md)は
`Cash6mGeneral_6mcEVR25`、**cEV・rakeなし・Single Size**で、台帳のNL50 Generalとは異なる。
さらに旧記録自身がraise木の近似を記している。boardとSRP履歴が同じでも、旧range、近似tree、
旧pass/borderline labelを001の同条件証拠へ移せない。002の旧River記録もnavigationの参考に限る。

自作の公開chip EVは[subgame_ev_offset](../../../crates/cli/src/postflop_setup.rs#L141)で
「開始局面以降の持帰りpot − 開始後の追加投入」へ戻している。
100 chips/BBのroot値は`public_ev_chips / 100`であり、旧契約の`+ starting_pot/2`を再び足さない。
BRとprofile EVへ同じoffsetを加えるのでseat gainは変わらない。ICMはutility単位のoffsetであり、
chipを加えたりchip potで割ったりしない。

[保存per-hand値](../../../crates/cli/src/sol.rs#L244)も各nodeで元のsubgame開始基準を維持する。
同じchip utility・同じ条件付きcontinuationに対し、node直前基準へ変えるなら、そのseatが
subgame開始後にすでに投入した`c_i(node)`を加える必要がある。
公式[EV Relativity](https://blog.gtowizard.com/what-is-expected-value-in-poker/)は、decision基準の
fold=0と過去投資を含むstack基準を区別するが、今回の各UI欄の厳密な基準を特定する資料ではない。
root、hand、action、後続nodeの値を同じoffsetで無条件に比較しない。

後日の[002のFold EV観測](condition-followup-20260927/README.jp.md)では、Riverで2bbを投入済みの
BBの正weight 3 comboについて、`Bet 2 → Raise 7`後のaction欄がFold EV 0を表示した。
これは当該欄のdecision-node基準を支持する追加証拠である。他の表示欄・取得版・参照残差や
精算規則まで確定するものではなく、元の取得記録と上記監査時点の欠測は書き換えない。

017/019の診断summaryは**保存前live平均profile**のEV/BRを保存metaから読み直したもの。
`.sol`の量子化後policyをBR再評価した値とは異なる。自作の別saved-profile auditが成功しても、
未取得の外部policyや外部条件の認定にはならない。

## 品質比較に必要な追加証拠

[測定仕様](../../../docs/plans/hu-postflop-r0/measurement-protocol.md)と
[prospective比較器の契約](../acceptance/external-contract.md)に従い、同一有限ゲームの条件証拠、
個別参照版・精度と表示丸め、seat/utility/EV基準、独立に確認したcorrectness baselineと数値誤差を揃える。
その後に根拠のあるseat別EV marginと内部NC/gain条件を、候補の比較前に固定する。
この監査JSONはそのcalibration入力ではなく、`confirmed`の承認証拠も生成しない。
不足値を系列accuracy・観測EV差・表示桁数で埋めない。

Node profileを取り込む場合は、全decisionのseat/combo/action IDと順序、元の確率・精度・版、
root rangeとcard removal、chance、全継続menu、terminal utility、node到達質量、EV基準を固定する。
単なるCopy rangeや集計頻度ではpolicyを復元できない。欠測・表示0%・真のzero reachを区別し、
未取得枝を一様戦略やゼロEVで補わず、profile固定後の同じ有限ゲームでEV/BRを再評価する設計とする。
この段落は必要条件の設計メモであり、取り込み実装や取得完了を主張しない。
