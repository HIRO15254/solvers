# Multiway Preflop ベンチマーク総合カタログと開発判断

2026-09-09 時点の設定と証拠を整理した。Goal は、GTOW と概ね同じ戦略傾向を得ることと、
強いマシンを効率よく使い、長時間計算を安全に保存・再開できることの両方を維持する。
小さいベンチマークで速いことや、単一 seed の頻度が近いことだけでは達成としない。

## 実験中断とコミット境界（2026-09-09）

ユーザー指示により、進行中だったEHS2 / draw-aware比較の完了を区切りに実験を中断した。
その後はローカルでコミット内容の検証だけを行い、新しい実験やVM起動は行っていない。
実験は、ユーザーから再開の指示があるまで再開しない。draw-awareの1 seed結果は探索的な記録に留める。

| コミット | 確定した内容 |
|---|---|
| `a4b70c3` | Windowsでのdaemonプロセステストを実行可能にする修正 |
| `3977361` | RNG列を維持した、サンプラーの割当削減 |
| `7d244b9` | state4の条件付きweight・独立平均・相手人数別cache、評価／停止判定、関連仕様とfixture |
| `59aca87` | 巨大mwsolのstreaming出力、checkpoint監査・出力計測ツール、互換性の説明 |
| `aee43a5` | 数値を維持するregret／average走査の並列化と4条件の実測記録 |

各コミットの候補ソースを隔離し、`cargo fmt --all --check`、警告を拒否するworkspace/all-targets
Clippy、`cargo test --workspace`を通した。最後のworkspace検証は720件成功、30件ignoreで、
監査／出力exampleの13件も成功した。規範の23見出しと実装guideの対応を照合した。
最後の初回検証はディスク容量不足でPDB生成に失敗したが、生成キャッシュを整理し、
ソースを変更せず再検証して成功した。検証記録とコミット対応は
`runs/multiway-convergence-round5-20260909/commit-staging/commit-summary.json`に保存した。

**未コミットで保持:** draw-aware、average samplingの研究候補、Tree並列化、compact drift、
`drop(prior)`によるメモリ寿命変更、および残りの実験driver・横断的な文書整理。
このカタログ・設定系統・実行台帳・設定索引は文書整理の作業用であり、まだコミットしていない。
開始時のソース37ファイルはbyte単位で保持されている。現在の作業treeには未検証のdraftが残るため、
上記の合格は各コミットに入れた内容に対するもの。

Preflop-only exportと現行runtimeの保存範囲の不一致は、実装guideに未解決として明記した。
収束精度・大規模並列効率・長時間RAMのGoal全体を達成したとは扱わない。

## 文書と再現用入力

| 文書 | 内容 |
|---|---|
| [設定系統](multiway-benchmark-families-2026-09-09.md) | canonical fixture 7件、経済条件、木、抽象化、GTOW参照範囲、旧研究の境界 |
| [実行台帳](multiway-benchmark-runs-2026-09-09.md) | 実行した seed・sweeps・batch・threads・K・評価条件、成否、証拠へのリンク |
| [全件索引](multiway-benchmark-config-index-2026-09-09.md) | 同じ内容の複製をまとめた、保存済み全設定ファイルの一覧 |
| [JSON索引](multiway-benchmark-config-index-2026-09-09.json) | 完全な明示TOML設定、SHA-256、保存済み研究manifestのargv・source/binary・評価条件 |
| [Goalと長時間運用の記録](multiway-scaling-long-run-2026-09-09.md) | 商用ソルバーの公開情報、性能・保存・RAMの証拠と未解決点 |

索引の対象は、現存する `examples` / `runs` の Multiway 設定と実際に残っている
`target/multiway-*-smoke/real` 系の実行設定、および削除直前のGit revision
`db930159e661c2ccd490884d70de2b4092713355` にある July 研究設定である。
現存284ファイル、履歴17ファイルの計301ファイルを記録した。17件には実験manifestも含む。
内容SHA-256では223種類であり、意味の異なるゲームが223種類あるという意味ではない。
コメント・運用条件・seedの違いも区別される。明示TOML設定のhashでは217種類であるが、
省略値やCLI上書きを含む実行上の等価性は表さない。
4件の空TOMLと1件の空manifestは回収不能な入力として列挙し、成功した実験に数えない。

ビルドtree、単体テストが作る一時fixture、`.cache`のsource複製、同じsourceを含む
圧縮archive、他solverの設定は重複・対象外として除外する。Julyのretired rollout/full-recall
設定は歴史資料であり、現行productionの選択肢へ復活させていない。

```text
python tools/multiway_benchmark_inventory.py --write
python tools/multiway_benchmark_inventory.py --check
python -m unittest discover -s tools/tests -p test_multiway_benchmark_inventory.py -v
```

`--write` は索引だけを更新する。solverやVMを起動しない。`--check` は設定やmanifestの追加・変更で
索引が古くなれば失敗する。元の設定・結果・frozen manifest は索引生成で書き換えない。

## ベンチマークをどう使うか

| 問い | 使用する条件 | 判断に必要な証拠 |
|---|---|---|
| 更新・乱数・再開が正しいか | 2bb push/fold、toy oracle、小さな同条件run | 数値・checkpointの一致、seat/side-pot/rake/actionの契約テスト |
| 実装変更が速いか | 固定state4、同じtree/K/seed/sweeps/batch、評価条件も固定 | training・初期化・評価・保存を分けた時間、RSS、全結果の一致 |
| algorithmが良いか | 同じgameとabstractionでbatch/discount等だけ変更 | 複数paired seed、複数計算量でのGTOW差とheld-out診断、必要時間 |
| abstractionが良いか | Simple部分tree、同じ有効bucket数・sweeps・seedで表現だけ変更 | controlの再現、169-class比較、held-out結果、異なる計算量での曲線 |
| tree/modelが足りるか | cap/limp/menuを明示的に一つずつ変える感度実験 | 観測GTOW menuとの対応、同じ参照nodeでの戦略差。異なるgameのgainを直接順位付けしない |
| 強いマシン・長時間運用に適するか | 同一checkpointまたはfreshな同条件runを8/16/24等で比較 | 進捗/時間・費用、RAM成長、保存peak、数値一致、中断再開と回収の完了 |

研究ハーネスでは、configの `max_time`、停止判定、checkpoint cadence、出力encodingが
production solveと同じようには動かない。実際の上限は記録された `--sweeps`、評価引数、
外側のtimeoutである。TOMLのhashだけを実験全体の識別に使わない。
`uniform-one` と `enumerate-first`、EHS2とdraw-awareが同じconfigを使うこともある。

次の事項を比較表から落とさない。

- source archive、binary、実効config、solver state version、abstractionの識別。
- tree rule、rake規約、位置、stack、limp、streetごとのbucket/cap。
- 学習seed・評価seed・sweeps・batch・threads・評価sample数・deviatorの種類と強さ。
- warm/cold cache、並行プロセス、計測区間、実行の成否、出力のhash。
- GTOWのsolution family、観測したmenuと未観測部分、参照の重み付け。

現行の品質baselineはstate4で統一する。state3の一般postflop profileはbucket cacheの
不具合により品質比較から除外し、checkpointをstate4へ連結しない。
postflop decisionを持たない2bb等の旧結果も、現行の品質を証明する代用にはせず、
その条件での機能・性能の履歴として扱う。
Simpleの845行はStandard Ranges Copyの169-classデータで、Generalの丸め済みActions表示とは別の参照である。
候補deviatorの利得は完全best responseやexploitabilityではない。

## 証拠から分かったこと

| 観測 | 開発判断 |
|---|---|
| 同条件K32でcloud 24 threadsは8 threadsのtrainingに対して約1.112倍 | CPU台数だけを増やす前に並列部分・直列部分・一時領域を測る |
| regret/averageの独立走査を並列化すると、4条件で全数値一致、timed regionは1.07–1.32倍 | 数値を変えない改善として保持。各条件1測定であり再測定・cloud適用は未検証 |
| Simple K128はseed0/32,768 sweepsでK32よりMAE改善なし | K増加を即座に採用せず、表現と必要学習量を分離する。大Kの長時間の可能性は否定しない |
| Simple b1/b4は3seedで平均差が小さくseed依存、b1の費用は高い | b1を既定にしない。複数計算量で品質/時間を見る |
| K256を262,144 sweepsまで延長しても一部handとSB limpに大きな差 | 長時間計算と並行して抽象化・tree/modelの原因切り分けが必要 |
| 大規模exportは初期化145秒・復元157秒・solution構築74秒・write43秒 | writeの並列化だけを優先しない。保存内容の契約と複製も調べる |
| arenaは固定でもdriftの記録と保存用copyが増える | arena bytesとprocess RSSを別々に測り、時間が経つと増える補助領域を改善する |

数値の原本は設定系統・実行台帳から辿れる。この表は異なるgameや計測区間を混ぜた総合順位ではない。

## Goalの再解釈と次の優先順位

Goalの到達点は変更しない。ベンチマークの役割を整理し、次の順序で作業を進める。

1. **比較の前提を固定する。** 全設定索引を作り、準備済みmanifestのstatusを実行成功と混同しない。
   新しい表現を試す前に、同じ実行基盤で既存controlの全数値を再現する。
   実験driverにcontrol gateを実装し、手動確認の省略や古い結果の混入を拒否する。
2. **戦略品質と必要計算量を一緒に測る。** Simpleの有効K128でEHS2とdraw-awareのseed0 A/Bを完了した。
   control gateは通過したが、これは表現の診断であり、1seedだけで昇格しない。候補が有望なら0/11/29のpaired seedと
   段階的な長時間runへ拡張し、学習曲線・費用・held-out診断を記録する。
   改善しなくても、より長い同条件baselineとの比較を残し、別の原因へ進む。
3. **大規模運用の直列部分とRAMを改善する。** 公開木の並列materializationはprototypeの一致テストまでで、
   productionへ未統合・速度未計測である。preflightを保持し、8/16/24の同条件で測ってから採否を決める。
   drift用mapとsnapshot/exportの複製を減らし、長時間のRSSと保存時peakを測る。
4. **保存契約の不一致を解消する。** 規範仕様はPreflopのみのsolution exportを求める一方、現在のwriterは
   全streetの正の平均質量を持つblockと公開状態を保存している。これは未解決の仕様/実装不一致である。
   全streetの再開状態はcheckpointに保持し、閲覧用solutionの範囲・metadata・reader・評価・testsを
   規範仕様と同期する必要がある。現状の大きな`.mwsol`を最終設計の必要容量と決めつけない。
5. **数時間の運用を最終的に検証する。** 起動、継続、評価、checkpoint、Spot中断、resume、export、回収を
   一連の実験として行う。小さなテスト、262k到達、大きなファイルの回収だけではこの条件を満たさない。

GTOW一致には未観測のtree/rake規約が残る。解の不一致をすべて収束不足に帰属させず、
有限のモデル差と計算不足を切り分ける。同時に、短いrunだけで大きいモデルや長時間計算を却下しない。

## 費用と実行方法

今回の整理に伴い、実験driverへcontrol gateを実装した。candidateの起動時と結果再利用時に、
controlの成功記録、実行コマンド、binary/config/stdoutのhash、state4、設定・抽象化fingerprint、
全`result`を凍結referenceと照合する。control未完了、失敗、異なる結果、変更済みreference、
残存partial file、古い確認記録ではcandidateを起動しない。gateを持たない既存manifestは従来通り扱う。

索引テスト2件、driverテスト14件、378件のカタログ内ローカルリンクを確認した。
workspace全体のfmt、警告を拒否するClippy、research-draw-abstraction featureを含むtestも成功
（747 passed、30 ignored）。規範仕様23見出しはguideの契約表に対応している。
これは上記のexport仕様不一致や長時間品質の未検証を解決したことは意味しない。
[検証記録](../../runs/multiway-convergence-round5-20260909/benchmark-catalog-validation/summary.json)。

小さい回帰・準備・初期比較はローカルで実行する。cloudはpayload・終了条件・回収手順が揃ってから
起動し、同条件で効率が良い構成に予算を使う。8→24でCPU時間が少ししか減らないrunを繰り返さない。
GCP総予算は$20のままで、disk・通信・未確定請求の余裕を残す。
[予算原簿](multiway-gcp-budget-2026-09-09.md)の計算費用は最終請求額ではない。

既存manifestを再検証する例（実行せず入力・hashだけ確認）:

```text
python tools/run_local_algorithm_screen.py --manifest runs/multiway-convergence-round5-20260909/local-simple-k128-screen/manifest.json
```

draw-awareの実験は、凍結sourceのbuild後にmanifestを作り、control→candidateの順に実行した。
既存結果を上書きせず、complete recordとhashが一致するときだけ再利用する。

```text
python runs/multiway-convergence-round5-20260909/local-draw-abstraction-screen/prepare.py --binary .cache/mw-draw-research-20260909-source/target-draw-research/release/examples/mw_draw_abstraction_research.exe --write
python tools/run_local_algorithm_screen.py --manifest runs/multiway-convergence-round5-20260909/local-draw-abstraction-screen/manifest.json --run
```

この文書はGoal達成の宣言ではない。未検証の長時間品質・cloud scaling・メモリ・保存契約を残したまま
完了扱いにしない。以降の実行で状況が変われば、raw evidenceを保持し台帳・索引・判断を更新する。
draw-aware A/B はこのペア完了後、ユーザー要請により追加実験を停止している。
