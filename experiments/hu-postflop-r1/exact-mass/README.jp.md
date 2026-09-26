# HU Postflopの微小な互換weightと数値修正コスト

カード除去の減算で正の相手weightが消える問題を、線形時間の評価を保ったまま修正する実験。
対象はkernel入口で表現されている有限・非負のf32 reachであり、solver全体の任意精度化ではない。
数値的な正しさは独立した算術・pairwise回帰で検査し、性能は[固定protocol](protocol.json)に従う
内部NashConv目標への到達時間で比較する。外部参照との品質認定やR1全体の受入とは分ける。

[最終方式・候補04の実測](report04.jp.md)では、32実行の品質・停止軌跡・canonical/stateが一致し、
新旧時間比の幾何平均1.048770、River比1.091399で固定の数値修正費用guardを満たした。
全ケースで時間は増加しており、速度向上やR1全体の受入を意味しない。
費用guardが不成立だった[候補01](report01.jp.md)と[候補03](report03.jp.md)、
全process call数を数えた[診断01](diagnostic/report01.jp.md)も保持する。

## 消失の原因と修正

相手reachの総和をT、heroの2枚を含む相手reachの和をCa/Cb、同一comboのreachをsとすると、
互換massは`T - Ca - Cb + s`となる。例えばheroがAsAh、相手がAsKhに1、KcKdに微小な正weightを
持つとき、AsKhだけがカード衝突で除外される。先にf64で`1 + tiny`を1へ丸めてしまうと、
その後1を引いても合法なKcKdのmassは戻らない。これは役強さの事前計算や手札ID圧縮とは別の問題である。
[元の数値監査](../range-scaling/source06/normalizer-edge-audit.md)と
[設計時の証明](../../../docs/research/2026-09-27-postflop-exact-weight-proposal.jp.md)を参照。

[mass.rs](../../../crates/holdem/src/mass.rs)は、呼出ごとに相手reachを走査してf64で十分かを判定する。
非zero項数をN、f32の指数field Eから得る`shift = max(E - 1, 0)`の最大・最小差をDとすると、
十分条件は`D + 24 + bit_length(N) <= 53`。`bit_length(N) = ceil(log2(N + 1))`であり、
N=0は別扱いで既存のzero-reach演算を使う。NaN・Inf・負値は拒否し、±0は受け入れる。

条件を満たす場合は既存のf64加算・減算順を維持する。満たさない場合はf32を共通の2冪単位の整数に
復号し、必要な幅のu64 / u128 / 5×u64で全体・カード別・同順位group・strict-below prefixのmassを保持する。
最も広い5×u64は`2^-149`単位で、最大1,326項と同一comboの追加1項は288 bitに収まる。
どの整数幅も加算overflow・負の減算を拒否する。
互換massは整数のまま`(T + s) - Ca - Cb`を計算し、loseも`compatible - win - tie`を整数で完了してから、
各massを一度だけnearest/ties-to-evenでf64へ変換する。

同一comboを重複させない固定52枚deckでは、判定と両経路の走査は入力・出力手札数に対してO(n)である。
手札ごとの総当たりfallbackは使わない。整数経路の作業領域と定数費用は増えるため、
この計算量だけから速度やRSSの改善は主張しない。判定は十分条件なので、通常の深いaction reachでも
指数差が広がれば整数経路を使い得る。旧CFV・state・収束軌跡のbit一致は全入力では保証しない。

## 共通経路と丸めの境界

最終方式（候補04）は、compact terminalの整数経路だけで必要な幅を選ぶ。
reachを一走査して非zero項数と正のf32 raw bitsのmin/maxを求め、指数抽出は走査後に行う。
±0を同じzeroとして扱い、負値・非有限値を拒否した範囲ではraw bits順と値・指数順が一致するため、
元と同じBが得られる。bit上限・最小shiftを8-byteの`MassAnalysis`で引き継ぎ、
候補03にあった幅選択時の再走査を省く。f64判定式を変えず、整数経路でだけscaleを構築する。最小shiftをL、
`B = D + 24 + bit_length(N)`として、B≤64ならu64、B≤128ならu128、それ以上なら5×u64を使う。
u64/u128の単位は`2^(L−149)`で、各項と同一combo追加を含む和は`2^B`未満に収まる。
カード除去とlose算出を整数で完了し、整数→f64のnearest/ties-to-even変換後に共通の2冪を掛ける。
結果はf64の正規数範囲にあるため、このスケーリングによる追加丸めはない。

dispatchはterminalごとに一度だけで、bucketにscaleや実行時型を埋め込まない。
`ExactSums`の53要素はu64/u128/5×u64でそれぞれ424/848/2,120 bytesとなる。
これは型の静的サイズでありprocess RSSの削減を表す測定値ではない。
normalizer・表示equity・test用global kernelは5×u64を維持し、compact各幅と広い整数の
differential testを行う。性能目標・cap・判定間隔・費用guardは初案と同じ値を保つ。
初案の観測を踏まえた改善であり、候補探索の履歴を隠した一回限りの試験とは扱わない。
走査共有と指数抽出の移動は分離実測していない。診断のcall数から時間寄与を推定せず、
診断用の別のtight十分条件も本番の判定には使わない。

[kernel.rs](../../../crates/holdem/src/kernel.rs)のfold/showdown、
[compatible_reach](../../../crates/holdem/src/compatibility.rs)、root normalizer、
[SOLの条件付きEV分母](../../../crates/cli/src/sol.rs)を同じmassの意味に揃える。
normalizerのown-weightとの積・最終和はf64演算のままである。
SOLの条件付きEVは、このf64互換分母をf32へ戻してからf32で除算するため、その丸めも残る。

[表示用equity](../../../crates/holdem/src/equity.rs)は`win + 0.5 * tie`と互換massをf64でrunout間集計し、
最後に比を取ってf32へ変換する。例えば唯一の合法相手が最小f32 subnormalのweightで常にtieなら、
中間分子をf32に落とさずequity 0.5を表せる。ただしf64の積・集計・除算の丸めまで整数化するものではない。

**solverのCFV経路は別の境界を持つ。** utilityとの積と符号の異なるpayoffの和にはf64の丸めが残り、
terminal出力はf32になる。最小subnormal weightにutility 0.5を掛けたCFVは、そのf32境界で0になり得る。
後から正しい互換分母で割っても消失前の値は復元できない。上流でreachを生成するf32乗算のunderflowと、
SOL保存時の量子化もこの修正の保証外である。分母のcard-removal修正、上流CFV丸め、保存量子化を混同しない。

検査対象は[massの独立算術tests](../../../crates/holdem/src/mass_tests.rs)、
[両席のkernel/pairwise tests](../../../crates/holdem/src/kernel_tests.rs)、
[subnormal equity回帰](../../../crates/holdem/tests/aggregate.rs)、
[Full SOL・lazy river・CFV境界の回帰](../../../crates/cli/src/sol.rs)に分ける。
凍結oracle `cfr-ref`は変更しない。

## 固定した比較条件

[旧source manifest](source-old01/source-candidate-manifest.json)は`db9b8742290ad06d472bf2836e017a11434341c6`を基準に、
両版共通の[計測example](../../../crates/cli/examples/hu_scaling_bench.rs)を適用する。
[最終方式の新source manifest](source-new04/source-candidate-manifest.json)を固定し、
旧source用targetの既存release binaryと、新source用の独立targetで検証したrelease binaryを用いる。
同じLinux x86_64 boot、4 logical CPU・全affinity、1 Rayon worker、compact/F32で測定する。
旧版はbuild-only、新版は通常workspaceと指定release testsを含む全validationを必要とする。
buildや転送を測定と重ねず、[terminal事前計算の実験](../showdown-kernel/README.md)の時間とは合算しない。

目標は過去のprepared-kernel比較のblock 1/new reportに記録された非負NashConvを、有効数字3桁で
上方向へ丸めた値である。0は0のまま。原reportとsource manifestは[reference](reference/)に置き、
protocolがbytes/SHA-256を固定する。過去source・旧sourceのcrate一致と過去・両版の入力一致も検査する。

| case | 内部NashConv目標 | 最大反復 | 判定間隔 |
|---|---:|---:|---:|
| River | 0.439 | 1,000 | 100 |
| Turn | 0.00228 | 1,000 | 100 |
| Flop | 0.0367 | 50 | 5 |
| narrow River | 0 | 10,000 | 1,000 |

NashConvは両席の`(BR0 - EV0) + (BR1 - EV1)`をclampせず用いる。NC/2を使う停止判定ではない。
NC/2の通常のExploitability解釈は2-player zero-sumに限り、rake等のgeneral-sumではNashConvとして扱う。
各版自身の数値評価に基づく内部目標であり、同じ閾値への到達は外部品質の同等性を証明しない。
特にtarget 0は微小負NCも通るため、数学的な完全均衡の証明には使わない。

planned iteration budgetは最初から最大反復数に固定し、各判定間隔だけ学習を進めて、最初の達成点または
上限で止める。`run_seconds`は全solve区間・停止判定用EV/BR・loop処理を含む。
構築、最終report用の再評価、全node CFV/stateの取得・保存はその外側で計測する。
得られるのは判定間隔上の初回到達時間であり、連続的な最短到達時間ではない。

4 caseそれぞれwarmup 1組と測定3組、計32 process・測定24 process。
case indexとblock番号の和が偶数ならold先行、奇数ならnew先行とする。
各caseの時間中央値からnew/old比を求め、幾何平均≤1.10かつ全case≤1.25を修正費用の事前guardとする。
これはこの実験の費用上限であり、公開の速度保証やR1受入閾値ではない。観測後の目標・上限・判定間隔変更、
pilot、retry、標本置換は行わない。

## 検証と保持

[run.py](run.py)と[verify.py](verify.py)は、同じ版・caseの各実行で停止反復数・品質軌跡・品質値・
canonical/stateの原bytes一致を要求する。旧版と新版では正規化config、algorithm、rake、utility、
global hand IDs、counts、normalizerのf64 bits、root weight・topology・dealを含む共通headerを照合する。
旧版と新版のCFV/state差は許容し、数値修正の正しさを旧版のbit一致で判定しない。

各processは300秒上限、起動余裕320秒、sampled RSS 10 GiB、空きRAM 1 GiB、空きdisk 4 GiBを条件とし、
外側cgroupは12 GiB以下・swap 0、絶対期限は起動側で固定する。準備失敗はterminal diagnosticとして残す。
process失敗、再現性・条件の不一致、目標未達では残りをskipし、費用guardを算出しない。
目標未達でもbenchmarkの診断reportを保持する。native wait4 peakとsampled process-tree RSSは別指標であり、
出力時の割当やpre-exec親プロセスのhigh-waterも影響するため、solve専用memoryやmemory非回帰の証明には使わない。

保持単位はsource archive/manifest、測定binary、build証拠、controlsと参照原文、全processの
stdout/stderr/resource samples、report、config、canonical/state原bytesである。compiler/Pythonはidentityのみ。
重複bytesは既存StoreのCASで保持し、SHA-256と原bytesの照合を行う。reportのBLAKE3欄は形式検査のみで、
独立再計算とは扱わない。source-afterは実行時の再hash記録であり、独立したfilesystem snapshotではない。
portable verifierはcheckoutの信頼するhelperだけを読み込み、保持したsourceやbinaryを実行しない。
資源・費用の根拠は[cloud台帳](../cloud/README.md)を参照する。
候補04回収後のVM10削除・資源不在確認は[cleanup記録](../cloud/cleanup-vm10/reconciliation.json)に保持する。
台帳の$32 / $40は実請求額ではなく保守的な予約保持額で、VM10の$3予約も解除していない。
