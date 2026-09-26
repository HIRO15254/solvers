# 旧 GTO Wizard library の条件追跡: 2026-09-27

公式の過去記事から、NL500 Simple 75bb の公開告知、Strategy table の EV を小数2桁へ
変更した告知、NL500 General の一部をレーキ設定の修正後に再計算した履歴を追加確認した。
**002 / 017 / 019 の取得済み node を特定の計算版へ結び付ける証拠は得ていない。**
[既存の条件監査](../reference-condition-audit.jp.md)の認定を変更せず、
`condition_match=unverified`、`quality_status=not_evaluated`、`acceptance=null`、
`comparison_threshold=null`を維持する。本記録は校正入力でも作業状態の台帳でもない。

公式本文の確認日、短い抜粋、主張の適用範囲、保存観測の byte identity は
[evidence.json](evidence.json)に収録した。Web 本文の全体や取得時の HTML bytes は保存していない。
記事の表示公開日は、本文の全更新履歴や取得 solution の計算日を意味しない。

その後、root の別の IAB 接続で002の `R2-R7` を閲覧し、正の range weight を持つ3 combo の
Fold EV がいずれも0と表示されることを確認した。これは今回の action EV 欄で
decision node 直前を基準とする解釈を支持する。下記の追加観測と
[fold-ev-observation.json](fold-ev-observation.json)に、手作業の転記として分離した。

## 追加した一次事実

| 一次資料 | 新たに確認した記述 | 個別取得への限界 |
|---|---|---|
| [2022-02-04 更新告知](https://blog.gtowizard.com/8max-cash-game-straddle-ante/) | NL500 Simple の 75bb を含む20–200bbの全 spot を公開したと記載。Strategy table の EV 表示を小数2桁へ変更したとも明記。 | 017/019 の系列・深さの歴史的存在と、表示桁数の公式説明。内部精度、丸め方式、現在の UI 版、各取得 node の残差は特定しない。 |
| [2021-08-13 更新告知](https://blog.gtowizard.com/new-solutions-aggregation-reports-and-other-improvements/) | NL500 General の CO spot を誤って rake なしで計算したと説明し、preflop を再計算、postflop を後続予定とした。 | **002 は BTN open のため、この CO 不具合が002にあったとは主張しない。** 系列名が同じまま精算設定の修正があった歴史を確認する資料。 |
| [2021-08-24 更新告知](https://blog.gtowizard.com/the-best-solutions-we-ever-had-and-other-improvements/) | 同じ CO spot を全て再計算したと説明。 | 前項の修正完了告知であり、現在取得した General の version ID ではない。 |
| [2020-12-17 旧 solution 説明](https://blog.gtowizard.com/all-you-need-to-know-about-our-solutions/) | preflop / postflop とも PokerStars 500 Zoom の5%、hand 当たり cap 0.6bb。主要 SRP/3BP/4BP は pot の0.4%、まれで range の小さい場面は0.8%という当時の accuracy 説明。 | 旧 library についての一次説明が追加された。ただし未 call 額の除外、rake の丸め・徴収時点・fold 免除を定義しない。この古い accuracy を現在の個別 River の誤差上限へ代入しない。 |

**推論:** 系列名・深さ・表示桁だけでは、比較対象の version と精算仕様を固定できない。
今回の新しい履歴資料はこの理由を具体化するが、同条件比較の不足を埋める証拠ではない。
小数2桁という告知から、最近接丸めや ±0.005bb の誤差区間を導かない。

公式 blog / help を対象に、legacy rake、uncalled / unmatched bet、EV の基準、表示丸め、
個別 node の精度・version を追加検索した範囲では、002 / 017 / 019 に直接結び付く
未 call 額の処理規則、丸め方式、River 残差、版識別子の記述は見つからなかった。
これは今回の検索範囲の結果であり、公開・非公開資料のどこにも存在しないという意味ではない。
現行 custom builder の matched-pot 説明を旧 library へ転用せず、観測 EV の差から条件を逆算しない。

## 初回調査時の取得設計: 002 の深い node で Fold EV の基準を測る

新規 solve を使わず、保存済みの明確な terminal action を読む。
[公式 Study Mode](https://help.gtowizard.com/study-mode/)は Strategy + EV を各 action の EV 表示と説明し、
Summary の EV の単位を bb と説明する。これは取得する UI 面の根拠であって、全欄の EV 基準を
特定する契約ではない。現在の欄でも単位を別途確認する。

[002 observed](../HU-R0-002/observed.json)の `observed_menus[17]` は、River の
`R2-R7`（BB bet 2bb → BTN raise to 7bb）、BB の残 stack 95.5bb と Fold action を保存している。
元転記は `menu-capture-01.txt` の16行目。保存 URL は取得履歴から組み立てたものであり、
当該 node の完全な DOM URL を独立保存した証拠ではない。

1. 接続済みの認証済みブラウザーで、Cash / 6max / NL500 / General / 100bb、
   BTN 2.5bb open → BB call、board `Ks7h2d3c8d`、Flop / Turn とも X-X を UI で再確認する。
   URL だけで系列を認定しない。
2. River `R2-R7` へ進み、actor BB、投入額、残 stack と全 menu を記録する。
   filter を解除し、現在の BB range で正の weight が確認できる具体的 combo を選ぶ。
   表示0%だけから zero reach と決めない。
3. Compare EV / regret 表示ではなく、Strategy + EV / hand detail の **Fold action 自体の EV** を読む。
   表示文字列、符号付きゼロ、単位、combo、range weight、警告、solution preview を保存する。
   root EV、range 平均 EV、hand 平均 EV を代用品にしない。
4. 現在の DOM URL、取得 UTC、画面または UI snapshot、表示可能な version / accuracy 情報を同時保存する。
   別の正の weight の combo でも同じ欄を確認する。空欄・非表示・予想外の値なら結論を保留する。

この node の BB は River 開始後に2bb、hand 開始後に合計4.5bbを投入済みである。
fold で追加の損益調整がないという各仮説では、表示値を次のように区別できる。
次表は取得前に置いた仮説予測であり、精算規則の確定値ではない。

| EV の仮説基準 | Fold EV の予測 |
|---|---:|
| 現在の decision 直前 | 0bb |
| River 開始時 | −2bb |
| hand 開始時の stack | −4.5bb |

Fold は terminal action なので、後続戦略の近似誤差に依存せず基準を調べられる。
ただし端数精度を校正する実験ではなく、この新しい取得の**この欄・node**の基準を識別する実験である。
結果から旧取得の版、他の UI 欄、レーキの未 call 額処理、全 profile や品質合否へ一般化しない。

初回調査を担当した agent の UI 接続一覧は `apps=[]` / `browsers=[]` であり、その context では
実験を実施していない。[evidence.json](evidence.json)の未実施記録はこの初回調査を表す。
この inventory を、後から root が利用できた別の IAB context の接続可否へ一般化しない。

## 追加観測: 002 の R2-R7 における Fold action EV

2026-09-26 UTC 16:11–16:16（JST 2026-09-27 01:11–01:16）に root が新しい IAB tab 5 で
read-only navigation を行った。以下は root が読み取った AX と目視 screenshot の
**手作業の転記**である。この追加ファイルには screenshot binary や原 AX archive を保持しておらず、
それらの source hash も主張しない。値は表示丸め後の数値として転記し、内部精度や元の表示文字列の
byte identity は保証しない。取得 URL は [機械可読記録](fold-ev-observation.json)に収録した。

UI で Cash / 100bb / 6max / NL500 / General / GTO、board `Ks7h2d3c8d`、
BTN 2.5bb open → BB call、Flop / Turn とも X-X、River root pot 5.5bb・両 stack 97.5bbを確認した。
下部の filter ではなく上部 history の BB Bet 2 → BTN Raise 7 をクリックした。
到達 node は BB stack 95.5bb、BTN stack 90.5bb、pot 14.5bbで、BB の menu は
Fold / Call / Raise 17 / Raise 23.5 / Allin 97.5。filter はクリックしていない。

Strategy + EV の Hands における action EV と、その後 Ranges → BB EV で確認した
同じ combo の Range weight・node EV は次のとおり。root は screenshot でも目視確認した。
Range weight の確認は、Fold EV を読んだ combo に正の weight があることの確認に用いる。

| BB combo | Allin 97.5 EV | Raise 23.5 EV | Raise 17 EV | Call EV | Fold EV | Range weight | Ranges の node EV |
|---|---:|---:|---:|---:|---:|---:|---:|
| As8s | 0.56 | 0.49 | 0.29 | 0.76 | 0 | 0.41 | 0.75 |
| Ah8h | 0.25 | 0.37 | 0.25 | 0.72 | 0 | 0.10 | 0.72 |
| Ac8c | 1.02 | 0.84 | 0.70 | 1.15 | 0 | 0.25 | 1.15 |

EV の単位は bb。Range weight と EV は全て丸め後の表示値である。
観測 screenshot に preview / banner warning は表示されていなかった。これは
他の画面や旧取得にも警告がないことを意味しない。

**限定した推論:** BB が River で2bbを投入した後でも、正の weight の3 combo で Fold EV が0。
これはこの node・この action EV 欄が decision node 直前を基準とする解釈を支持し、
取得前に置いた River 開始基準の −2bb、hand 開始基準の −4.5bb とは区別できる。
Ranges の node EV は補助観測として残し、この実験だけでその欄や root EV の基準を認定しない。
個別 solution の timestamp 付き version、残差、レーキ精算に関する追加事実は取得していない。
全 UI 欄の EV 基準、旧取得の版、未 call 額処理、外部品質合否へは一般化しない。

## 追加観測: 019 の root と library preview

root が一時的な IAB tab 6 で [019 の既存 URL](../HU-R0-019/observed.json)を開き、
Cash / 75bb / 6max / NL500 / Simple / GTO、board `Qs7h2c4d9s`、Flop / Turn とも X-X、
River pot 40.5bb・BB / BTN とも残 stack 55bb、BB の Check / Bet 13.5 / Allin 55 を再確認した。
Change から library を開くと、選択行は `75 Simple NL500 GTO GTO -` だった。

preview は Cash 6max、全 available spots、Reports `FLOP` / `TURN`、
rake `5% 0.6BB CAP`、postflop sizings `3-6`、Accuracy `0.2-0.3%` を表示した。
説明は open に対する3bet / fold、4bet all-in なし、BvB limp なし、postflop sizeを減らした系列を示した。
Accuracy の値をクリックしても追加の説明は表示されず、個別 node の残差、精度の指標・分母の説明、
solution version、rake の丸め・除外規則はこの有限の操作経路では取得していない。
**library の一般的な Accuracy 表示を、この River の実測誤差へ代入しない。**
Reports のラベルや今回のクリック結果から、UI 全体で追加情報を取得できないとも結論しない。

[library-preview-observation.json](library-preview-observation.json)は root が伝えた tool display の
**手転記**であり、原 AX・screenshot bytes は保持していない。writer は UI を独立閲覧していない。
取得後の時計実測は UTC `2026-09-26T16:58:17Z`（JST 2026-09-27）だが、個別 tool の時刻と
正確な開始時刻は未取得のため、`recorded_after_utc` のみ記録し `observed_at_utc` は null とした。
一時 tab 6 は閉じ、元の tab 3 は維持した。既存取得の版との同一性や精算仕様の不足は埋まらず、
`condition_match=unverified`、`quality_status=not_evaluated`、`acceptance=null` を変更しない。

同日の有限な公式資料の再確認では、[系列一覧](https://blog.gtowizard.com/status-and-info-about-our-solutions/)が
75bbを含む6max NL500 Simpleの精度をpot比の0.2–0.3%と説明している。
[比較記事のOverview](https://blog.gtowizard.com/gto-wizard-ai-benchmarks/)も事前CFRのGeneral / Simpleを挙げ、
精度の確認先としてsolution selectorのpreviewを案内する。
[2022年の再現比較記事](https://blog.gtowizard.com/why-doesnt-my-solution-match-gto-wizard/)には
NL500 General 2.5xのFlopを開始pot 5.5bbの0.3%で説明する例があるが、019の75bb Simple Riverではない。
これらは系列表示の単位を補強する資料であり、019の個別残差・版・停止記録を提供しない。
40.5bbへ系列の割合を掛けた値を、そのRiverの誤差上限や比較許容差として採用していない。
