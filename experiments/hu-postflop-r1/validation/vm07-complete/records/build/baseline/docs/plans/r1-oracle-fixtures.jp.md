# R1 HU独立oracle fixture

T1-01の小さいHU独立照合を補う。外部参照値の代用や、全NLHEの精度認定ではない。
R0時点の範囲は[asset map §4.1](hu-postflop-r0/asset-map.md#41-独立oracle)、
既存のTurn→River照合は[oracle_diff.rs](../../crates/holdem/tests/oracle_diff.rs)に残す。

## 追加する有限ゲーム

[oracle_river.rs](../../crates/holdem/tests/oracle_river.rs)で次の3件を定義する。
全件River `Ks Qs 7h 2d 3c`、開始pot 5 chips、最小bet 1 chip、
両seatのbet/raise menuはpot-after-callの50%と100%、最大aggression数は3
（最初のbetを含む）、iso off、chip EV、F32/DCFRである。
50%の端数は切上げ、合法最小raiseへ引上げた後にeffective stackでcapし、
重複targetを除く。all-inの不足raiseを含み、これを超える別actionはゲームに含めない。

| Fixture ID | Effective stack | Terminal rake |
|---|---:|---|
| `river-raised-no-rake` | 40 | なし |
| `river-short-allin-capped-rake` | 8 | 12.5%、cap 1.5 chips |
| `river-raised-uncapped-rake` | 40 | 12.5%、cap 100 chips（この木では非拘束） |

Rakeはfold/showdownの両terminalで、その時点のpot全額から徴収する。
未callのbetを除く等の別rake規則を表すfixtureではない。

| Seat | Comboとweight |
|---|---|
| OOP | `AhAd:0.75,7c7d:0.25,JhJd:0.5,KsKd:1` |
| IP | `AcAs:0.5,KhKd:1,JcJs:0.25,AcAd:0.125` |

`KsKd`はboard衝突で除去し、`AhAd`対`AcAd`はprivate card衝突で除去する。
残る11組のworldをweightの積で正規化する。rankは固定boardで手計算した
`JJ < AA < 777 < KKK`をoracle側に保持する。AA同士・JJ同士のtieを含む。
新fixtureはproductionのrankerを呼ばない。

## 照合と独立性

Scalar adapterは賭け状態、合法action、遷移、精算をtest内に独立記述し、
凍結`cfr-ref`へ渡す。productionのbuilder、betting helper、payoff pipeline、
terminal evaluatorをoracle側の計算へ流用しない。共有するのはカードとcomboの識別子だけ。
`crates/cfr-ref`本体を変更しない。

Strategyをexportする前に、全11 worldについて両実装の木をrootから対応付ける。
各nodeのactor、history、順序を含む全action label、contribution、child数、
terminal位置を比較する。Oracle profileは未知infosetを一様戦略で補完せずエラーにする。

各fixtureのiteration 0と3で、production平均profileをscalar oracleへ渡し、
両seatのEV、BR、`BR − EV`を絶対誤差`1e-4 chips`未満で照合する。
Rake時は両seatのEVを個別計算し、zero-sum shortcutを使わない。
これは同じ有限ゲーム上の評価一致の検査であり、3反復で収束したという判定ではない。
奇数potの内部baselineはOOP 2/IP 3 chipsで、公開subgame-start EVへの変換を
このtestから認定しない。Capped fixtureのcheck-check tie、bet-fold、
short-all-in-callのscalar精算は手計算値でも固定する。

実行対象は `cargo test --locked -p holdem --test oracle_river`。
この文書の追加自体はtest実行成功の証拠ではない。Source識別付きの実行証拠は
R1の検証記録へ残す。通常変更のworkspace検査もAGENTSに従う。

## 保存後profileの限定照合

[CLIのsol.rs unit test](../../crates/cli/src/sol.rs)の
`full_artifact_profiles_are_reevaluated_after_u16_quantization` は、保存後評価の
限定fixtureである。前節の独立scalar oracleとは別で、production evaluatorを使う。

既存の小さいRiver／Turn fixtureを3反復解き、`Full`の`.sol`を保存して再読込する。
Riverは12.5%・cap 1.5 chipsのrakeあり、Turnはrakeなし。両方でsource storageの
F32／I16を検査する。全storage refのu16戦略を`dequantize_probs`で復元し、
再構築した同じcompiled gameのoffsetへF32 `strategy_sum`として配置する。
regretは0とし、追加iterationを行わず、両seatのEVとBRを個別に再評価する。
NashConvは両seatの`BR − EV`の和から求め、保存値blockを評価値へ流用しない。

`meta.ev`／`meta.expl`／`meta.nash_conv`は量子化前live profileの測定値として
元summaryとの一致を検査する。量子化後profileのEV／BR／NashConvは別の値であり、
metadataがその値を認定するとは扱わない。F32へ配置した後の列正規化による丸めも含む。

許容差は測定結果に合わせて選ばず、各fixtureの最大action数`A`、rootからterminalまでの
最大action node数`L`、ChipEV payoff幅`U = pot + 2 × effective_stack`から事前に定める。
`D = 65535`、`ε = f32::EPSILON`として、各seatのEV／BR差は
`U × [L × (A / (2D − A) + 4Aε) + 256ε]`以下、NashConv差はその4倍以下とする。
最初の項はu16丸め後の列正規化を含むtotal variation上界と、経路上のcouplingによる
期待値差の上界である。`4Aε`は確率変換・再正規化、`256ε`はこの小fixtureの
f32 terminal／chance集約に固定で与える数値余裕であり、任意サイズの木の誤差保証ではない。

Turnの`NoRivers` artifactは保存集合が全storage refを覆わないことを確認し、
test内のprofile復元helperが明示拒否する。欠落riverを一様戦略や再solveで埋めて
「保存後profileの再評価成功」とすることはない。公開CLIに再評価機能を追加するtestではない。
このtestも実行成功の証拠はsource識別付きの検証記録へ別途残す。

## 残る検証範囲

既存Turn fixtureと合わせても、Flop開始の独立oracle、suit-isomorphismのprofile対応、
保存／再読込後profileの独立評価、I16や全algorithm、ICM等の非chip utility、
一般のrake規則、GTO Wizardの日常／拡張セットを認定するものではない。
外部参照fixtureの条件・値取得と比較手順は別途必要である。
