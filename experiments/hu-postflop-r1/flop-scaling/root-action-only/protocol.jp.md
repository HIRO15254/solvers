# Root action only: 未実行の単一要因候補

この研究コピーは、chanceを含む木の **CFR root action 1箇所だけ** を追加で並列化する。
production、公開default、chance traversal、quality traversalは変更しない。
ビルド・solve・性能計測は未実行であり、VM起動や予算の追加許可ではない。
VM18 chance-depth比較のprotocol・採否guard・原本には手を加えない。

元sourceは `crates/engine/src/solver.rs`、SHA-256
`69063b31c3433b2eb4798b2f41015311200301a7ba1887b05bbefdb936d4e81a`。
`prepare.py` はpinを確認し、private factory `ActionPlan::for_cfr` と `step/run` の
2 call site、および専用plan unit testsだけを研究コピー `solver.rs` へ追加する。
逆適用が元source全bytesに一致し、CFR/value traversal本体がbyte一致することを検査する。
原本、flat、worker-scratch、fused-updateは編集・混在させない。

## 変更する分割条件

chanceなしの木では現行 `ActionPlan::new` にそのまま委譲する。
chanceありの木では、worker>1、rootがAction、root storage要素数≧8192を前提に、
現行と同じ `grain=clamp(ceil(root_elements/(4*workers)),4096,65536)` を求める。
rootの子subtreeにgrain以上のものが2本以上ある場合だけ、rootの`here/below`をtrueにする。
他の全nodeはfalse。大きい子が1本だけなら、下位action forkを探索せずNoneとする。
root自身がChanceならNone。workers1は計画を作らない。

chance depthは引き続き「並列化資格を持つchance通過数」であり、actionでは消費しない。
新root forkの内側で既存chance fan-outが動くことはある。depth0でもこのroot actionは
並列化し得るため、混在木の完全逐次対照にはworkers1が必要になる。これはchance depthの
数え方の変更ではなく、従来なかった独立のaction並列化範囲の追加である。
qualityのfactoryは元の`ActionPlan::new`なので、混在木のquality action並列化は引き続き無効。

## mutable view / Rayon の静的監査

元 `solver.rs:546–592` の `ActionViews` は、自nodeと各子の互いに重ならない
`StorageSpan`を切り出す。`StorageView::split` は元viewを空にする契約なので、
親own viewを子へ貸さず分割を維持することが重要である。
rootのhero passは自nodeのstorage要素とI16 scale slotを保護した後、sigmaを読み、
各子のCFVが戻ってから元のaction順でregret/strategyを更新する（元786–875行）。
opponent passはsigma読取後に子viewを分割し、元順に子CFVを加える（元884–940行）。

既存 `cfr_action_children`（元956–988行）が使う`par_iter_mut`と`par_chunks_mut`は
異なるstorage viewと出力sliceをtaskへ渡す。reach/sigma/contextは共有読取、scratchは
task localで、lockやborrowをnested traversalへ持ち越す新機構は導入しない。
chanceの再帰は元どおりaction planをNoneへ落とす（元708/764行）。
従ってこの候補はchance後にaction forkを増やさない。

Action edgeでは次元を保持し、chanceでのみdealごとのmapped dimensionを使う
（`tree.rs:215,238–242`）。rootの可変手次元を固定したりHold'em専用条件にしたりしない。
zero handでは既存の `num_hands>0` / `!out.is_empty()` guardでparallel chunksを避ける。
ただし静的監査はRust型検査や実行検証の代用ではない。追加のplan VecはO(nodes)で、
`run`では全反復/両seatに再利用、`step`では呼出しごとに作成する。
同時に保持するtask scratchとopponent child CFVが増える可能性を残す。

## 実行前に必須の検証案

保存した4つのRust unit testsは、threshold4095/4096、rootだけのmark、単一heavy child、
root chance、workers1、chanceなしの既存計画一致を扱う。これらはまだcompileも実行もしていない。
Python source検査だけで品質合格とはしない。

次のcampaignでは、既存ユーザー許可内でrootが利用可能予算と実行条件を起動前に固定し、
性能計測より先に以下を完了させる。

1. 候補のcore unit/integration testsを有限のrelease枠で実行する。plan unit testsの固有名と
   root forkが実際に選ばれた証拠を保持する。
2. F32/I16双方で、root action→可変次元Transition→nested chance、zero dimension、
   zero-length storage span/I16 scale slot、zero reachと一般和payoffを含む小fixtureを追加する。
   rootの2子が固定grainを超えることを明示的に確認し、無効経路だけを比較しない。
3. pools1/2/4および有限32worker smoke、depth0/1/2で全state bits、EV/BR/gains、
   CFV all/selected/none、read-only state不変性、同じ反復数の`run`/連続`step`一致を確認する。
   既存 `parallel.rs` / `value_scratch.rs` を土台にするがoracleと実装は共有しない。
4. 独立scalar oracleにexportしたprofileのEV/BR照合を、候補forkを確実に通す木でも行う。
   現行Hold'em oracle testsは逐次設定なので、既存成功だけで新経路を認定しない。
   frozen `cfr-ref` 本体は変更しない。node/action semanticsは独立fixture側で定義する。
5. 性能比較の前に同一fixture全F32 state・quality JSONの全byte一致を要求する。
   未完了・不一致なら性能解釈を行わない。

### 用意した実演算テスト候補（未compile・未実行）

`root_action_paths.rs` を研究用source copyの `crates/engine/tests/root_action_paths.rs`、
`root_action_oracle.rs` を `crates/holdem/tests/root_action_oracle.rs` へ配置する。
候補 `solver.rs` の置換も同じ研究用copyに限る。productionと既存oracle本体は変更しない。
`test-overlay.json` に配置先と全source pinを保持する。

fixtureは487node/195storage refs/8706elements。root P0は1手、P1は2048slotだが
非zero reachは2slotだけで、各root子のP1 action自身が4096elementsを持つ。
payoffはchance後の最大2×3手だけで計算するため、2048²の行列は作らない。
8つのchance枝は(0,2)/(2,0)/(0,0)/(1,2)/(2,3)の次元を含み、さらに2-wayのidentity chanceを通る。
zero reach、zero elementsのI16 scale slot、可変次元、nested chanceを少量のstorageで扱う。

専用の並列経路testは2workers、chance_depth0/min_children最大で実行する。
2つのroot枝の指定terminalが別workerで到達したことをCondvar rendezvousで確認する。
各待ちは2秒上限で、root forkが消えた場合に永久waitせずfailする。
この確認は人工的な同期を含むため性能試験に混ぜない。ノイズでtimeoutしてもretryせず原因を記録する。

F32/I16ごとの比較testは2iterations、workers1/2/4 × depth0/1/2 × run/stepの18条件と
1worker対照を全state bits（I16 scalesも含む）、EV/BR/gains、CFV all/selected/none、
root query、read-only不変性で比較する。強制forktest以外に同期観測器は入れない。

別のscalar adapterは2つのroot private assignmentsだけを列挙し、独自のhistory state machine、
chance遷移、payoff表を `RefGame` として実装する。engineのtree/transition/payoff関数を
scalar規則の計算に使わず、共有するのはexportしたinfoset keyとpolicyだけである。
除去されたchance massはroot normalizerを変えずzero payoffとして表す。
F32/I16 × workers1/2について、2iterationsのexport profileを既存 `cfr-ref` APIでEV/BR再評価し、
各seatの絶対差≦1e-4を要求する。これは2fixture実装の照合であり外部NLHの認定ではない。

次campaignの2CPU検証で使う固定テスト選択は以下を案とする。各commandのwall/RSSはrootが
起動前に有限枠へ固定する。実行後に標本や閾値を調整しない。

```text
cargo test --locked --offline --release -j2 -p engine --lib root_action_only_tests:: -- --test-threads=1
cargo test --locked --offline --release -j2 -p engine --test root_action_paths -- --test-threads=1
cargo test --locked --offline --release -j2 -p holdem --test root_action_oracle root_action_profile_ -- --test-threads=1
```

固有test名4+3+2の計9件をraw stdoutで確認する。oracle testはengine側fixtureをmoduleとして
取り込むが、この最後のfilterで重複のfixture testを実行しない。Rustによる型・borrow検証や
実際の成功はまだ得ていない。現時点で完了したのはsource生成・逆適用・構文整形とPython純検査だけである。

## 次の性能screenを設計する場合の境界

まだ実行protocol・予算・VM・反復数を確定しない。VM18結果からdepthを事後に有利な標本へ
選び直すことはしない。次campaignではCFR depthとquality depthを事前固定し、
同source/同compiler/profileのbaselineとこの候補でroot forkの有無だけを比較する。
root子ごとのstorage量・terminal数を計時外に記録し、分割資格と負荷偏りを確認する。
warmup/反復/順序、全工程deadline、RSS/保存上限、時間・quality・noise guardは
新しい未観測のcampaignとして事前固定する。VM18のguardを改変・流用して採用扱いにしない。

root枝間の待ち合わせを減らせる可能性はあるが、既にchanceでworkerが埋まっていれば、
allocation・同時保持・nested schedulingを増やすだけかもしれない。
16core/32logicalでの比較から32physicalの線形スケールやSMT原因は認定しない。
採用前の通常workspace fmt/clippy/testsとコードレビューは別途必須である。
