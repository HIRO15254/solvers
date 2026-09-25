# SOL-1 HU Postflop候補の選定と多様性

## 1件の計算範囲

各caseの`street`・`board`・`postflop_actions`は**計算を始める局面**を示す。Flop開始caseは、そのFlopの1ノードだけではなく、以後のbet/raise/all-in分岐、Turn/Riverのchanceと継続、末端までを含むHU Postflopゲーム木を対象とする。Turn/River開始caseも、それぞれの開始局面から末端までの継続木を対象とする。`tree_access=取得可能`は画面で後続をたどれる見込みの判定であり、全ノード・全menuの取得や比較の完了を意味しない。

初回検証計画はFlop開始に加えてTurn/River開始も別のシチュエーションとして含める。日常8件のうちFlop開始は4件であり、**日常8件すべてがFlop起点の全ツリー**という構成ではない。高速近似の「1ノードの各action EV」は初回の返却範囲であり、厳密CFRの計算範囲を1ノードへ縮める指定ではない。

## 選定結果と判定境界

2026-09-25にログイン済みのGTO Wizard既存ライブラリーを画面で閲覧し、[cases.csv](cases.csv)にV1〜V8各3件、計24件を登録した。24件すべて`catalog_checked`であり、元の卓人数はいずれも6max。全件について`source_url`、`checked_at`、solutionプリセット、HUに残る2席、Preflopの全action、開始street/board、開始pot、初期/残りstackを確認した。URLは閲覧時に表示されたgametype・depth・history spot・action・boardのパラメーターから記録し、`HU-R0-001`はURLを開き直してboardと手番が再現されることを確認した。`HU-R0-003`のURLは、Flop開始spotを表示していても先の`flop_actions`等が残るため、R1で開始spotを再選択する。

`daily+extended`の8件は拡張24件に含まれる。日常のstreet配分はFlop 4・Turn 3・River 1。V8ではsqueeze後Riverの`HU-R0-022`より、dead moneyのないSB limp・BB check Flopの`HU-R0-023`を日常代表に選んだ。このため当初案のFlop 3・Turn 3・River 2から変更した。別streetへ進んだ場合も、開始boardとPostflop履歴が異なる問題として別IDにした。同一問題内の複数hand/actionを件数に算入していない。

**SOL-1の候補選定・基本条件確認・多様性・R1引き渡しの受入条件を満たす。** Cash cEV（rakeなし）を含む24件が確認済みである。rakeありのcash解について、各solutionの徴収条件とutilityの厳密な対応はR1で確認する。SOL-4の[測定仕様](measurement-protocol.md) §1で`comparison_scope`を同一ゲーム候補21件・参考比較3件へ暫定区分した。これはSOL-1の閲覧事実や、同条件比較を認定するものではない。全数値転記、固定fixture化、新規solve、credits消費は行っていない。

## 多様性表

| 軸 | 確認済み分布 | 不足・R1での確認 |
|---|---|---|
| 枠・suite | V1〜V8各3件。日常8件、拡張24件 | 各枠の代表は日常に含まれる |
| rakeプリセット | 確認済みcash: cEV（rakeなし）1、NL50 General 11、NL500 General 4、NL500 Simple 6、NL50 GG General 1、NL1k GG General 1 | rakeありの率・capはライブラリーのRakeヘルプで確認。GGは3bet以降のPreflop potにも課金する点で通常cashと異なると公式説明で確認。各solutionとの厳密な対応はR1で確認。cEV Single Sizeと他のmulti-size解はbet menuが異なる。ステークス名だけを別設定として数えない |
| stack | 浅い20bb 2件・40bb 1件、75bb 6件、標準100bb 12件、深い150bb 1件・200bb 2件 | 20bb cEVはSingle Sizeであり、20bb NL50 Generalとmenu・rangeが異なる |
| Preflop Action | SRP 12、IP側aggressor 3bet 3、OOP側aggressor 3bet 3、4bet 3、squeeze 1、limp 1、iso 1 | V8のsqueezeにはfoldしたUTGのdead money 2bbがある。単純なHUのopen-callへ置換しない |
| street | 拡張Flop 12・Turn 6・River 6。日常Flop 4・Turn 3・River 1 | V8の日常を再現性優先でFlopへ変更。Flop候補をRiverで代用していない |
| board | high/low、connected、paired、monotoneを含む | suitとcard removalをR1で固定する |

2026-09-25にGTO Wizardの既存解ライブラリーの`Rake`ヘルプを画面で開き、NL50 5% cap 4bb、NL500 5% cap 0.6bb、NL50 GG 5% cap 8bb、NL1k GG 5% cap 1bbを確認した。`rake_description`へこの画面確認を記録した。徴収条件は同ヘルプに表示されず、選んだsolutionごとの確定は残る。[GTO Wizardの公式解一覧](https://blog.gtowizard.com/status-and-info-about-our-solutions/)はrate/capを記載し、[公式のGG解紹介](https://blog.gtowizard.com/multitabling-new-solutions/)はGGがPreflopの3bet以降にもrakeを取る点を説明する。通常cash解のno-flop-no-dropとの対応はsolutionごとに未確認。`HU-R0-008`のcEVはライブラリー画面で確認し、[公式説明](https://blog.gtowizard.com/introducing-nodelocking/)がCash cEVを「no rake」と定義している。[Single Size解の公式告知](https://blog.gtowizard.com/single-size-solutions-are-live-new-pricing-50x-more-solutions/)は、以前Preflopのみだった枠にもPostflopの継続木を追加したと説明する。実際にこのFlopからBB check、BTN bet 2.6、BBのfold/call/raiseまで画面でたどった。

## 枠ごとの採用と残る差

| 枠 | 採用した実在候補 | 選定目的とのずれ・障害 |
|---|---|---|
| V1 | BTN対BB、100bb SRP。Flop high、River、Flop connectedの3件 | この枠は全てcash。NL50とNL500のcap差は画面で確認。徴収条件は未確認 |
| V2 | SB対BB SRP。Flop、paired Turn、connected River | NL50 GGのcapは画面で確認。Preflopの徴収条件はsolution別に未確認 |
| V3 | 20bb cash NL50 Turn、20bb cash cEV Single Size Flop、40bb cash NL50 FlopのSRP | cEV Single Sizeのbet menuはmulti-size解と異なる。浅いstackのall-in境界の数値は未取得。日常`HU-R0-007`はraked cash |
| V4 | 200bb Turn/River、150bb HJ対BB Flop | 深いstackと高SPRはある。後続streetの全menuはR1で取得 |
| V5 | CO対BTN 100bb General、HJ対BTN 75bb SimpleのIP側3bet | 75bb Simpleのmenu差を同一ゲーム比較へ混ぜない |
| V6 | BTN対BB paired Turn/River、CO対BB monotone FlopのOOP側3bet | 75bb Simple NL500に集中。stack/rakeを変える代替はR1で検討 |
| V7 | BTN対BB、HJ対BTNの75bb Simpleと100bb Generalの4bet | narrow rangeとcall/fold境界の数値は未取得。40bb Simpleでは4bet shortcutが無効だった |
| V8 | squeeze後HU River、SB limp/BB check Flop、SB limp/BB iso Turn | 異なる到達を3種確認。squeezeのdead moneyとrake判定が同条件比較の障害 |

## 取得可能性とR1への引き渡し

確認済み24件では画面の履歴ツリーで開始nodeのbet/check/all-inと後続分岐を閲覧でき、Strategy + EV表示で頻度・EV欄を確認できたため、`tree_access`、`frequency_access`、`ev_access`を`取得可能`とした。`HU-R0-008`ではFlop後の複数decision nodeもたどった。`HU-R0-001`ではRanges tabのCopy機能を直接確認し、`range_access=取得可能`。他23件の両者combo range抽出は個別に試していないため`要確認`。いずれも全数値転記の完了を意味しない。

R1は各行の`source_url`からsolutionと開始spotを再確認し、両者combo range重み・正規化、card removal、pot/dead money、rake/utility、全Preflop/Postflop履歴、各nodeと後続streetのbet/raise/all-in menu、hand/combo別action頻度・EV、EV基準点・表示精度・参照版を取得する。`missing_fields`と`next_action`は行ごとの取得項目と比較障害を示す。`same_game_candidate`はR1で照合する候補区分であり、完全再現の認定ではない。[検証計画](../../validation.jp.md)の固定情報とEV基準に従って監査する。

GTO WizardのMTT ChipEV 20bb（8max）は画面上で`preflop only`と表示され、Postflop候補に算入しなかった。HU SnG 20bb Generalはライブラリー上にあるが、現在のログインではPostflop戦略・EVにアクセスできない。Cash 6max cEV Single Sizeの既存Postflop解を確認できたため、`HU-R0-008`をこちらへ差し替えた。画面の`AI solve`は別の新規solveへのリンクであり、既存のSingle Size Postflop解の不在を意味しない。

R0-06へ本票の選定結果と、R1で確認する「rakeありcash解の現行徴収条件とutility、Single Sizeのmenuとlocal実装との一致」を渡す。過去の[HU参照実験](../../../experiments/hu-postflop-reference/README.md)も現行解の確認証拠として再利用しない。
