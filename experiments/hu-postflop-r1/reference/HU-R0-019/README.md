# HU-R0-019: River参照の取得記録

T1-01の部分取得。既存の解決済みsolutionを手動ペースのUI操作で閲覧し、両レンジをCopy操作で取得した。
[観測値と欠測](observed.json)、[BB/OOP range](oop-range.txt)、[BTN/IP range](ip-range.txt)を保持する。
原始combo重みは再正規化していない。BB/BTNの順は候補CSVのseat一覧順とは異なるため明示する。

固定Riverの12決定nodeのmenuを閲覧で確認した。rake徴収規則と個別nodeの参照精度は
未照合であり、同一ゲームにも数値合格にも認定しない。
library previewと[公式対応表](https://blog.gtowizard.com/status-and-info-about-our-solutions/)で
NL500 Simple 75BBの率5%・cap 0.6BB、accuracy表示0.2–0.3% potを確認した。
[accuracyの一般定義](https://blog.gtowizard.com/how-solvers-work/)はseat別BR gainの平均を開始potで割った値だが、
[riverの再solveの説明](https://blog.gtowizard.com/gto-wizard-ai-benchmarks/)もあるため、preview値をこのnodeの実測精度とはしない。
旧libraryでのfold時徴収・uncalled額の処理順・丸めと内部versionは未確認である。
root EVの和が39.90bbである事実から、レーキ率やcapを逆算して確定しない。
表示の小数桁は参照solver自身の収束精度を表さない。zero-frequency actionにも表示されたEVを保持する。

## 診断用入力と照合

[diagnostic.toml](diagnostic.toml)は100 chips = 1 BBとして、開始pot 4050、残stack 5500、
min bet 100を使う自己完結したRiver configである。両rangeはCopy原文を前後の改行以外
変更せず埋め込み、重みを再正規化しない。OOP=BB／IP=BTN、preflop aggressor=IPである。

DSLの`1350c`／`3700c`はstreet内の**累計raise-to額**であり、追加額やpot比ではない。
初回betは1350／all-in 5500、1回目aggressionへのraiseは3700／5500、
2回目へのraiseは5500、street上限はbetを含め3回とする。
既知のmenuを出発点とし、実際のtreeが一致することは実行後のexportで検査する。
全12 decision menuの接続から32 action edge、21 terminal、全33 public nodeを導出できる。
これは観測graphの期待値であって、未実行solverの実測node数ではない。

Rakeには次の**未認定の診断仮定**を置く。

- 全fold／showdown terminalでmatched potの5%、cap 60 chipsを徴収する。
- 追加の丸めは適用しない。旧libraryでの実際の徴収条件・丸めは未確認のままである。
- 現runtimeの`percent-cap`は未call額を含む総拠出potへ課率する。matched pot専用optionはない。
  ただし本caseは開始potだけで`4050 × 0.05 = 202.5 > 60`となるため、全terminal徴収という
  仮定の下ではmatched potと総拠出potのどちらでもcap 60になる。このcaseだけの限定同値であり、
  一般のuncalled額処理を認定するものではない。

現行custom builderの[公式rake説明](https://help.gtowizard.com/how-to-build-custom-solutions/)は
matched potとhand当たりcapを述べるが、旧Simple libraryの実装保証として流用しない。
参照root EV総和を、この仮定の根拠にも検証結果にも使わない。

Run設定の10000 iteration／30秒、100 iterationごとのcheck、NashConv 0.1 chipsは
診断用の計算予算・停止値である。参照との合格閾値ではなく、固定性能campaignのconfigも変更しない。

[check_diagnostic.py](check_diagnostic.py)はPython標準libraryだけを使い、次を区別する。

- `--check-inputs`: configのTOML、range原文・SHA256・combo数・重み合計・board衝突、
  観測graphの閉包と数を検査する。RustのDSL parserやsolverは実行しない。
- 実行後: 実際の`tree --node all` JSONの全history・actor・action順序・pot・street・保存有無を
  観測値へ照合し、summaryの全node数／保存node数も検査する。
- EV診断: 公開summaryのlive平均profile EVを100で割ってBBへ戻し、表示参照値との差をseat別に出す。
  保存戦略を再評価した値ではない。自身のNashConvと開始pot比も記録するが、参照accuracyはnullのまま、
  `condition_match=unverified`、`quality_status=not_evaluated`、`acceptance=null`を維持する。

実行する段階で使う例（repo rootから。以下は準備手順であり実行済みの証拠ではない）:

```text
solvers validate experiments/hu-postflop-r1/reference/HU-R0-019/diagnostic.toml
solvers solve experiments/hu-postflop-r1/reference/HU-R0-019/diagnostic.toml --out runs/hu-r0-019-diagnostic
solvers export runs/hu-r0-019-diagnostic/solution.sol tree --node all --output runs/hu-r0-019-diagnostic/tree.json
solvers export runs/hu-r0-019-diagnostic/solution.sol summary --output runs/hu-r0-019-diagnostic/summary.json
python experiments/hu-postflop-r1/reference/HU-R0-019/check_diagnostic.py --tree runs/hu-r0-019-diagnostic/tree.json --summary runs/hu-r0-019-diagnostic/summary.json --run-config runs/hu-r0-019-diagnostic/run.toml --source-id SOURCE_SNAPSHOT_ID --output experiments/hu-postflop-r1/reference/HU-R0-019/diagnostic-report.json
```

Validator自身の軽量testは`python -m unittest discover -s experiments/hu-postflop-r1/reference/HU-R0-019`。
[test_check_diagnostic.py](test_check_diagnostic.py)の手で記述したtree行はvalidator用fixtureであり、
実solverの出力に偽装しない。入力検査やvalidator testの成功だけでtree一致・EV精度を宣言しない。

この参照データは検証用に取得した。利用者の全面的なファイル転送許可に基づき、
同じGCP projectの実験VMへ診断入力として転送できる。通常のbuild source archiveには含めず、
転送する場合は入力のhashと実行結果を別途記録する。
