# SOL-1 HU Postflop候補の選定と多様性

## 1件の計算範囲

各caseの`street`・`board`・`postflop_actions`は**計算を始める局面**を示す。Flop開始caseは、そのFlopの1ノードだけではなく、以後のbet/raise/all-in分岐、Turn/Riverのchanceと継続、末端までを含むHU Postflopゲーム木を対象とする。Turn/River開始caseも、それぞれの開始局面から末端までの継続木を対象とする。`tree_access=取得可能`は画面で後続をたどれる見込みの判定であり、全ノード・全menuの取得や比較の完了を意味しない。

初回検証計画はFlop開始に加えてTurn/River開始も別のシチュエーションとして含める。日常8件のうちFlop開始は4件であり、**日常8件すべてがFlop起点の全ツリー**という構成ではない。高速近似の「1ノードの各action EV」は初回の返却範囲であり、厳密CFRの計算範囲を1ノードへ縮める指定ではない。

## 選定結果と判定境界

2026-09-25にログイン済みのGTO Wizard既存ライブラリーを画面で閲覧し、[cases.csv](cases.csv)にV1〜V8各3件、計24件を登録した。23件は`catalog_checked`、`HU-R0-008`は`unavailable`であり、後者を確認済み件数に算入しない。全件について`source_url`、`checked_at`、solutionプリセット、元の卓人数（6max 23件、HU 1件）、HUの2席、Preflopの全action、開始street/board、開始pot、初期/残りstackを確認した。URLは閲覧時に表示されたgametype・depth・history spot・action・boardのパラメーターから記録し、`HU-R0-001`はURLを開き直してboardと手番が再現されることを確認した。`HU-R0-003`のURLは、Flop開始spotを表示していても先の`flop_actions`等が残るため、R1で開始spotを再選択する。

`daily+extended`の8件は拡張24件に含まれる。日常のstreet配分はFlop 4・Turn 3・River 1。V8ではsqueeze後Riverの`HU-R0-022`より、dead moneyのないSB limp・BB check Flopの`HU-R0-023`を日常代表に選んだ。このため当初案のFlop 3・Turn 3・River 2から変更した。別streetへ進んだ場合も、開始boardとPostflop履歴が異なる問題として別IDにした。同一問題内の複数hand/actionを件数に算入していない。

**登録件数と枠配分は24件だが、利用できる確認済み解は23件であり、rakeなし枠をまだ満たさない。現行cash解のrake徴収条件もsolution別に未確認のため、SOL-1の受入条件全体は未達**。本台帳の全行の`comparison_scope`は`pending`であり、R1の同条件比較を認定していない。全数値転記、固定fixture化、新規solve、credits消費は行っていない。

## 多様性表

| 軸 | 確認済み分布 | 不足・R1での確認 |
|---|---|---|
| 枠・suite | V1〜V8各3件。日常8件、拡張24件 | 各枠の代表は日常に含まれる |
| rakeプリセット | 確認済みcash: NL50 General 11、NL500 General 4、NL500 Simple 6、NL50 GG General 1、NL1k GG General 1。HU SnG ante 1はアクセス不可 | cashの率・capはライブラリーのRakeヘルプで確認。no-flop-no-dropなどsolution別の徴収条件は未確認。HU SnGのpot内rakeなしはトーナメント形式からの推定で、解へのアクセスも不足。ステークス名だけを別設定として数えない |
| stack | 浅い20bb 2件・40bb 1件、75bb 6件、標準100bb 12件、深い150bb 1件・200bb 2件 | HU SnGの20bbには0.125bb/人のanteがあり、cash 20bbとpot・rangeが異なる |
| Preflop Action | SRP 12、IP側aggressor 3bet 3、OOP側aggressor 3bet 3、4bet 3、squeeze 1、limp 1、iso 1 | V8のsqueezeにはfoldしたUTGのdead money 2bbがある。単純なHUのopen-callへ置換しない |
| street | 拡張Flop 12・Turn 6・River 6。日常Flop 4・Turn 3・River 1 | V8の日常を再現性優先でFlopへ変更。Flop候補をRiverで代用していない |
| board | high/low、connected、paired、monotoneを含む | suitとcard removalをR1で固定する |

2026-09-25にGTO Wizardの既存解ライブラリーの`Rake`ヘルプを画面で開き、NL50 5% cap 4bb、NL500 5% cap 0.6bb、NL50 GG 5% cap 8bb、NL1k GG 5% cap 1bbを確認した。`rake_description`へこの画面確認を記録した。徴収条件は同ヘルプに表示されず、選んだsolutionごとの確定は残る。[GTO Wizardの公式解一覧](https://blog.gtowizard.com/status-and-info-about-our-solutions/)はrate/capを記載し、[公式のGG解紹介](https://blog.gtowizard.com/multitabling-new-solutions/)はGGがPreflopの3bet以降にもrakeを取る点を説明する。通常cash解のno-flop-no-dropとの対応はsolutionごとに未確認。`HU-R0-008`のpot内rakeなしはHU SnGというトーナメント形式からの推定で、画面でのrake/utility設定確認は残る。[公式紹介](https://blog.gtowizard.com/simplified-solutions-and-a-new-interface/)はHU SnG GeneralにPostflop解があることを説明している。

## 枠ごとの採用と残る差

| 枠 | 採用した実在候補 | 選定目的とのずれ・障害 |
|---|---|---|
| V1 | BTN対BB、100bb SRP。Flop high、River、Flop connectedの3件 | この枠は全てcash。NL50とNL500のcap差は画面で確認。徴収条件は未確認 |
| V2 | SB対BB SRP。Flop、paired Turn、connected River | NL50 GGのcapは画面で確認。Preflopの徴収条件はsolution別に未確認 |
| V3 | 確認済みは20bb cash Turn、40bb cash FlopのSRP。20bb HU SnG Flopはアクセス不可 | HU SnGはante 0.125bb/人、pot 4.25bb、pot内rakeなしと推定。ただしGeneralのPostflop戦略・EVにはPremium Tournament以上が必要。cashとはutility・rangeが異なり、all-in境界の数値は未取得。日常`HU-R0-007`はraked cash |
| V4 | 200bb Turn/River、150bb HJ対BB Flop | 深いstackと高SPRはある。後続streetの全menuはR1で取得 |
| V5 | CO対BTN 100bb General、HJ対BTN 75bb SimpleのIP側3bet | 75bb Simpleのmenu差を同一ゲーム比較へ混ぜない |
| V6 | BTN対BB paired Turn/River、CO対BB monotone FlopのOOP側3bet | 75bb Simple NL500に集中。stack/rakeを変える代替はR1で検討 |
| V7 | BTN対BB、HJ対BTNの75bb Simpleと100bb Generalの4bet | narrow rangeとcall/fold境界の数値は未取得。40bb Simpleでは4bet shortcutが無効だった |
| V8 | squeeze後HU River、SB limp/BB check Flop、SB limp/BB iso Turn | 異なる到達を3種確認。squeezeのdead moneyとrake判定が同条件比較の障害 |

## 取得可能性とR1への引き渡し

確認済み23件では画面の履歴ツリーで開始nodeのbet/check/all-inと後続streetを閲覧でき、Strategy + EV表示で頻度・EV欄を確認できたため、`tree_access`、`frequency_access`、`ev_access`を`取得可能`とした。`HU-R0-001`ではRanges tabのCopy機能を直接確認し、`range_access=取得可能`。他22件の両者combo range抽出は個別に試していないため`要確認`。`HU-R0-008`は再訪時にPremium Tournament以上へのupgrade表示が出たため、全access欄を`取得不可`とした。いずれも全数値転記の完了を意味しない。

R1は各行の`source_url`からsolutionと開始spotを再確認し、両者combo range重み・正規化、card removal、pot/dead money、rake/utility、全Preflop/Postflop履歴、各nodeと後続streetのbet/raise/all-in menu、hand/combo別action頻度・EV、EV基準点・表示精度・参照版を取得する。`missing_fields`と`next_action`は行ごとの取得項目と比較障害を示す。`same_game_candidate`はR1の完全再現認定ではないが、現時点ではその候補区分にも進めていない。[検証計画](../../validation.jp.md)の固定情報とEV基準に従って監査する。

GTO WizardのMTT ChipEV 20bb（8max）は画面上で`preflop only`と表示され、Postflop候補に算入しなかった。HU SnG 20bb Generalはライブラリー上にあるが、現在のログインではPostflop戦略・EVにアクセスできない。利用できる別の契約・ログインはないと2026-09-25に利用者が確認した。HU SnG 20bbのSingle Size解ではPreflopを閲覧できたものの、Flop選択は新規AI solveへ進むため代替に採用しなかった。Cash Heads-upのSingle SizeにはcEVプリセットがあるが、画面で確認したものはPreflop解であり、Multi Sizeの既存Postflop解はNL500 rakeだった。確認した範囲に利用可能なrakeなしPostflop代替はない。

R0-06へ「HU SnGの既存解アクセス不足、pot内rakeなし・utilityの直接確認不足、cash解の現行徴収条件未確定」を未完了理由として渡す。過去の[HU参照実験](../../../experiments/hu-postflop-reference/README.md)も現行解の確認証拠として再利用しない。
