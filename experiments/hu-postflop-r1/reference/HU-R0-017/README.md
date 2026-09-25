# HU-R0-017: River 参照メニューと診断入力

2026-09-25 UTC に既存の解決済み GTO Wizard library を閲覧し、BB/OOP と
BTN/IP の range を UI の Copy 操作で取得した。[observed.json](observed.json) の
URL は [R0 catalog](../../../../docs/plans/hu-postflop-r0/cases.csv) の原文を保持する。
この記録を保存する agent は収集担当 root agent から UI 観測値を受け取り、
ブラウザーの操作・navigation は行っていない。新しい solve も実行していない。

Cash 6max 75bb Simple NL500 GTO/GTO。BTN が 2.5bb open、BB が 10bb に
3bet、BTN が call。Flop `Kh Kc 5s` と Turn `2d` はいずれも check/check、
River `3s` 開始時の pot は 20.5bb、残 stack は双方 65bb、最初の actor は BB。

[OOP range](oop-range.txt) と [IP range](ip-range.txt) は Copy 原文を並べ替えず、
重みを再正規化せずに保存し、各ファイルへ末尾 LF を1個だけ加えている。
原文の文字数は OOP 5492、IP 3584。収集担当は CUA clipboard 変数と保存原文の
文字数・FNV-1a 32-bit（`a57cfbc7` / `f8ea9c1a`）を照合し、保存担当も
ファイルから同値を再計算した。FNV は転送照合用であり、証拠ファイル自体の
SHA-256 は末尾 LF を含めて [range-integrity.json](range-integrity.json) に残している。

非零 combo は OOP 352、IP 232。重み合計はそれぞれ `30.5171114` と
`53.4459435` で、UI の weighted combos 表示 `30.5` / `53.4` と0.1刻みで整合する。
表示の combos は非零 combo の個数と混同しない。カード順序を無視した重複、
同一カードの二重使用、board 衝突、非有限・非正の重みはなかった。

両 range の正の重みを持つ組は81,664組、そのうち互換組72,304組、非互換組9,360組。
互換 joint mass は `1353.45025728126031`、制限しない積の合計は
`1631.01581166760590`。カードを共有しない組の `w_oop × w_ip` を Decimal 80桁で
合計した値で、追加の正規化やchance重みは入れていない。これは raw range の
算術的整合の検査であり、参照サービス内部の元精度を保証するものではない。

保存済み menu は BB root、BB check 後の BTN、BB bet 7bb に対する BTN と、
bet 7 → raise 19 → raise 39.5 → all-in 65 の応答列、bet 7 → raise 26 後のBB、
bet 20.5 → raise 42 → all-in 65 の応答列、root all-in後、checkからの各aggressive応答列を含む28ノード。
最初の2ノードでは check、bet 7 / 20.5、all-in 65bb、3ノード目では
fold、call、raise 19 / 26、all-in 65bb を確認した。Raise 額は UI label として保持し、
実装の raise-to / 追加額への変換を確定したとは扱わない。
全decision menuの接続は閉じているが、完全な手別strategy/EVや精算条件まで取得したとは扱わない。

17:26 UTC 頃の追加観測では、BTN raise 19 と BB raise 39.5 の表示頻度は0%。
最後の BB fold/call menu では UI が解なし・意味のあるrangeなしと表示し、戦略・EVは欠測だった。
Menu が見えることと、その枝の戦略・EVが得られることを区別する。表示上の0%から
厳密な reach zero を証明したり、欠測を一様戦略・ゼロEVで補完したりしない。
保存したroot rangeは、これらの枝へ移動する前に取得した原文のままである。

Bet 7 → raise 26 後の BB menu は fold、call、all-in 65（付随するUI表示54%）。
Live URL の `history_spot=13`、`river_actions=R7-R26` を確認し、reload後も
解なし・意味のあるrangeなしの表示だった。この枝でも戦略・EVは欠測として保持する。
親では BTN raise 26 の頻度が19.5%と表示されており、子で解が提供されない理由との
関係は未解明である。これを zero reach と断定しない。

17:31–17:37 UTC 頃には bet 20.5 後の BTN が fold/call/raise 42/all-in 65、
raise 42 後の BB が fold/call/all-in 65、その all-in 後の BTN が fold/call と確認した。
付随するUI表示は最初の raise 42 が35%、all-in が72%、BBの all-in が22%。
これらはメニューの記録で、枝の戦略・EVは保存していない。

17:52–17:55 UTC 頃の追加観測では、rootから直接 all-in 65 を選んだ BTN の
top menu は fold/call。下方には重複するCall表示があったため、この画面の頻度・EVは採用しない。
Check → bet 7 後の BB は fold/call/raise 19/raise 26/all-in 65 を screenshot で確認した。
後者の読み込み済み統計値はまだ読んでいない。

同時間帯の BB check 後の BTN は、check 62.8%、bet 7 が3.7%、bet 20.5 が33.5%、
all-in 65 が0%、表示EVは BB 4.89bb / BTN 15.01bb。これらは
`auxiliary_node_ui` に補助的な表示原値として保存し、River root の値・品質判定と区別する。

17:54–17:57 UTC 頃に check → bet 7 の raise 19 / 26 / all-in 65 分岐とその子孫、
check → bet 20.5 の raise 42 / all-in 65 分岐とその子孫、check → all-in 65 の
応答について12メニューを追加した。各top menuはlive URLと更新後の木に対応していた。
遷移中はAborting overlayが頻繁に現れ、統計が前nodeのまま残ることがあったため、
これらの画面から頻度・EVを取得していない。対称性による補完ではなく、各menuの実観測を保存した。

17:58–18:00 UTC 頃には残る `bet 20.5 / all-in 65`、`bet 7 / all-in 65`、
`bet 7 / raise 26 / all-in 65`、`bet 7 / raise 19 / all-in 65` のfold/call menuを
top menuとlive URLで確認した。表示残call額は順に44.5、58、39、46bbである。
読み込み中の統計は採用しない。これにより記録済みaggressive actionに続く未取得menuは0となった。

BB root の表示頻度は check 71.2%、bet 7 が14.4%、bet 20.5 が5.5%、all-in が8.8%。
合計99.9%は表示丸めを含む値として原値を保持し、100%へ補正しない。
表示 EV は BB 7.68bb / BTN 12.22bb、equity は38.5% / 61.5%。
EV の0.01bb刻みは表示解像度であり、solver の収束精度や比較合格値ではない。

Rake 5%・cap 0.6bb は R0 catalog 情報で、この取得では再確認していない。
Fold 時の徴収、uncalled 額、丸め、厳密な EV 基準点、solver version、内部精度、
当該解の停止条件・精度値も未確認。EV の和から rake 仕様を逆算して確定しない。
`condition_match=unverified`、`quality_status=not_evaluated`、`acceptance=null`、
`comparison_threshold=null` を維持し、品質判定は作成していない。

再検査は repository root から
`python experiments/hu-postflop-r1/reference/HU-R0-017/check_ranges.py`。
この検査は保存 byte、取得時の転送チェック、Decimal 算術、表示値との整合確認であり、
solver 実行・完全な公開木の一致・参照品質の合格判定を含まない。

## 診断用入力と照合

[diagnostic.toml](diagnostic.toml)は100 chips = 1 BB、pot 2050、残stack 6500、
min bet 100の自己完結したRiver入力である。両rangeは保存原文をそのまま埋め込む。
OOP=BB／IP=BTN、preflopの最後のraiseはBBなので`preflop_aggressor="oop"`とする。
このscriptはcbet/donk条件を使わない。

診断ではUI額をstreet累計のraise-toとして解釈する。DSLは初回betを700/2050/6500、
700に対するraiseを1900/2600/6500、2050に対するraiseを4200/6500とする。
700→1900の後は`aggressions == 2 && to_call == 1200`で3950/6500を許可し、
他のraiseはall-in 6500だけとする。Aggression上限はbet込み4回。
28 decision menusから80 action edges、53 terminal、81 public nodesが導かれる。
これは観測graphの期待数であり、未実行solverの測定結果ではない。

Rakeには全fold/showdown terminalでmatched potの5%、cap 60 chips、追加丸めなしという
**未認定の診断仮定**を置く。現runtimeはuncalled額を含む総拠出potに課率するが、
このcaseでは開始potだけで`2050 × 0.05 = 102.5 > 60`となるため、全terminal徴収という
仮定の下ではどちらでもcap 60になる。この限定同値は旧libraryの徴収規則を認定せず、
root EVの和をレーキ仮定の根拠にもしない。

10000 iterations、100 iterationsごとのcheck、NashConv 0.1 chips、soft max_time 30秒、
F32、1 thread、chance並列depth 0は019と同じ診断予算である。参照への合格閾値ではなく、
固定性能campaignのbaselineや判定値は変更しない。019専用cloud runnerは変更していない。

[check_diagnostic.py](check_diagnostic.py)は次の検査を分けて行う。

- `--check-inputs`: TOML、保存rangeのhash/原文/重み/board、固定DSLと診断予算、
  全menuのactor・閉包・接続・count・表示call額を検査する。Rust parserやsolverは実行しない。
- 実行後: 実際の`tree --node all`に全history・actor・action順・pot・street・storedが一致し、
  summaryが81 public/28 stored nodesでFullであることを検査する。
- EV診断: 公開summaryの保存量子化前のlive平均profile EVをBBへ換算し、root表示EVとの差を示す。
  保存戦略の再評価ではない。自身のNCとpot比は記録し、参照精度・合格閾値・acceptanceはnullのままとする。

入力検査とvalidator test:

```text
python experiments/hu-postflop-r1/reference/HU-R0-017/check_ranges.py
python experiments/hu-postflop-r1/reference/HU-R0-017/check_diagnostic.py --check-inputs
python -m unittest discover -s experiments/hu-postflop-r1/reference/HU-R0-017 -v
```

実行する段階で使う例（未実行の手順）:

```text
solvers validate experiments/hu-postflop-r1/reference/HU-R0-017/diagnostic.toml
solvers solve experiments/hu-postflop-r1/reference/HU-R0-017/diagnostic.toml --out runs/hu-r0-017-diagnostic --sol-streets full
solvers export runs/hu-r0-017-diagnostic/solution.sol tree --node all --output runs/hu-r0-017-diagnostic/tree.json
solvers export runs/hu-r0-017-diagnostic/solution.sol summary --output runs/hu-r0-017-diagnostic/summary.json
python experiments/hu-postflop-r1/reference/HU-R0-017/check_diagnostic.py --tree runs/hu-r0-017-diagnostic/tree.json --summary runs/hu-r0-017-diagnostic/summary.json --run-config runs/hu-r0-017-diagnostic/run.toml --source-id SOURCE_SNAPSHOT_ID --output experiments/hu-postflop-r1/reference/HU-R0-017/diagnostic-report.json
```

[test_check_diagnostic.py](test_check_diagnostic.py)の28行は手で記述したvalidator fixtureであり、
solver実行結果ではない。入力検査やtestの成功を参照とのゲーム一致やEV精度の成功とはしない。
