# HU Postflop参照候補（P1品質検証の入力）

[製品定義](../../products.jp.md)第4節のP1品質検証に使う、GTO Wizard既存解の候補台帳である。
2026-09-25に旧R0（Linear SOL-1）で選定した24件を、2026-10-04の再構築で本書へ移した。
選定時の手順書・工程記録はgit tag `archive/pre-two-products-2026-10-04`の`docs/plans/hu-postflop-r0/`にある。

**状態: 候補のみ。** 開始spotと基本条件を画面で確認しただけで、range・全後続menu・頻度・EVの転記、
fixture化、自作solverとの比較はまだ行っていない。外部比較の合否判定に使える件数は0件である。

**利用者決定（2026-10-09）**: fixture化して比較に使う参照は、24件のうち下の6件に絞る（Linear SOL-8）。
Turn・River開始は実務で稀なので、Flop開始に固定する。Preflopの経緯の多様性を取り、rakeなし（cEV）を含め、
stackは100bb以上を複数含める（6件中5件）。残りの18件は候補として残すが、利用者が追加を決めるまで取得・照合しない。

| case | stack | Preflopの経緯 | 参照ライブラリ（GTOW） | rake |
|---|---|---|---|---|
| HU-R0-001 | 100bb | SRP（BTN open、BB call） | `Cash6mSimple_6mcEVR25`（cEV Single Size） | なし |
| HU-R0-013 | 100bb | IP側3bet（CO open、BTN 3bet、CO call） | `Cash6m50zSimple_SimpleIP` | NL50 5% cap 4bb |
| HU-R0-018' | 100bb | OOP側3bet（CO open、BB 3bet 13.5、CO call） | `Cash6m500zSimple_SimpleIP` | NL500 5% cap 0.6bb |
| HU-R0-021 | 100bb | 4bet（BTN open、BB 3bet、BTN 4bet、BB call） | `Cash6m50zSimple_SimpleIP` | NL50 5% cap 4bb |
| HU-R0-023 | 100bb | limp（SB limp、BB check） | `Cash6mSimple_6mcEVR25` | なし |
| HU-R0-008 | 20bb | SRP（BTN open、BB call） | `Cash6mSimple_6mcEVR25` | なし |

**参照ライブラリの決定（2026-10-09、利用者）**: 台帳のGeneral（3〜6 size）は全木の再現にf32で約295 GBが要り、
GTOWが一部の部分木を公開しない（HTTP 204）ため使わない。rakeありの3bet・4bet potはSimplified（IP側が1〜2 size、
OOP側はnodeごとのGTOW AIのsize）、SRPとlimpはcEV Single Size（両者1 size）で取る。rakeありSRPは木が大きいので含めない。
全nodeは取らず、木をほぼ完全に再現し、いくつかのnodeで照合する。HU-R0-018は75bbのSimplifiedが無い（HTTP 403）ので、
同じboard（Ad8d3d）の100bb NL500 Simplifiedに置き換えた（018'）。limpはrakeありSimpleに無いのでcEVで取る。
取得・fixture・比較の手順と結果は[SOL-8 fixture](../../../experiments/hu-postflop-reference/fixtures-2026-10/README.md)に置く。

選定は、Flop開始のcaseのうち、Preflopの経緯（SRP・IP側3bet・OOP側3bet・4bet・limp）とrake（NL50・NL500・cEV）が
重ならないように取った。台帳のsqueezeとisoはTurn・Riverの開始なので含まない。ブラインド同士のSRP（HU-R0-004）、150bb（HU-R0-012）も含まない。

Ks7h2dの既存記録（[Flop](../../../experiments/hu-postflop-reference/cases/ks7h2d-flop/README.md)、
[River](../../../experiments/hu-postflop-reference/cases/ks7h2d3c8d-river/README.md)）は、GTOWのcEV Single Size 100bbの解を参照している。
HU-R0-001の参照と同じライブラリだが、fixtureはSOL-8の取得から作り直す。

## 構成

[cases.csv](cases.csv)の24件は、多様性の枠V1〜V8に3件ずつ割り当てた。`suite = daily+extended`の8件が
日常用（Flop 4・Turn 3・River 1）、全24件が大きな変更後の拡張用（Flop 12・Turn 6・River 6）である。

| 軸 | 分布 |
|---|---|
| 卓 | 全件6max cash |
| stack | 20bb 2、40bb 1、75bb 6、100bb 12、150bb 1、200bb 2 |
| Preflopの経緯 | SRP 12、IP側3bet 3、OOP側3bet 3、4bet 3、squeeze 1、limp 1、iso 1 |
| rake preset | cEV（rakeなし）1、NL50 General 11、NL500 General 4、NL500 Simple 6、NL50 GG General 1、NL1k GG General 1 |
| board | high/low、connected、paired、monotoneを含む |

画面で確認したrakeは、NL50 5% cap 4bb、NL500 5% cap 0.6bb、NL50 GG 5% cap 8bb、NL1k GG 5% cap 1bb。
徴収条件（no flop no drop等）はsolutionごとに未確認で、GGはPreflopの3bet以降にも課金する。

## 共通Inputへの対応

各行は[`solvers.nlh/v1`](../../nlh-input-v1.jp.md)のゲーム記述へそのまま写せる。

| cases.csvの列 | `solvers.nlh/v1` |
|---|---|
| `table_players`、`stack_bb` | `[table] players`、`stack_bb` |
| `preflop_actions`（例 `UTG F; HJ F; CO F; BTN R2.5; SB F; BB C`） | `[spot] line`のPreflop部（`BTN r2.5, BB c`） |
| `postflop_actions`（例 `Flop X-X; Turn X-X`） | `[spot] line`のPostflop部（`BB x, BTN x / BB x, BTN x`） |
| `board` | `[spot] board` |
| `pot`、`effective_stack` | lineから導出（照合用の値として使う） |
| `rake_description` | `[economics.rake]`（徴収条件の確認後） |

## 比較に使う前に確認すること

`comparison_scope`の`same_game_candidate`（21件）は同一ゲームを再現できる見込みがある候補、
`diagnostic_only`（HU-R0-006、022、024）は条件差の解消が要る参考候補である。どちらも同一ゲームの認定ではない。
HU-R0-022のfoldしたUTGのdead moneyは、lineから開始potを導出する新Inputで表現できる。残る障害はrakeの徴収条件である。

各caseを比較に使う前に、両者のcombo range（重み・正規化）、Preflop/Postflopの全履歴、各nodeと後続streetの
bet/raise/all-in menu、rakeの率・cap・徴収条件、EVの基準点と表示精度、参照版・取得日時を取得し、
自作の入力と一致することを確認する。一致しない項目がある比較は診断値として扱い、合否に使わない。
頻度の差だけで、EVがほぼ等しい混合戦略を不合格にしない。

過去の参照実験（[hu-postflop-reference](../../../experiments/hu-postflop-reference/README.md)、2026-07）は
当時の条件の記録であり、現行解の確認証拠として再利用しない。
