# F32 CFR更新のbuffer往復を省く研究候補

このディレクトリはsource準備であり、本体への採用・Rust compile・正しさ・性能の検証結果ではない。
production crate、凍結`cfr-ref`、既存の実験sourceは編集しない。現在の測定から帯域飽和などの
単一原因を認定せず、実在する不要な一時buffer操作を削る一因子候補として扱う。

## 根拠と変更範囲

[VM16 CPU診断](../../cloud/vm16/flop-cpu-occupancy-analysis01.json)では32 workersのCFRが
16 workersよりnarrowで8.40%、expandedで4.82%遅い一方、process CPU秒は約2倍だった。
expandedの品質7walkは32 workersで10.73%短縮した。CPU/wallにはspinも含まれ、SMT、帯域、
cache、scheduler、並列待合せの寄与は分離できない。本候補はCFR更新だけを変え、quality walkや
chance scheduling、worker数、scratch取得、terminal sweepを変えない。

baselineは`solver.rs` SHA-256
`69063b31c3433b2eb4798b2f41015311200301a7ba1887b05bbefdb936d4e81a`、`storage.rs`
`1929ce7947c6b60d9aea8d75540065c8c9a0596a3f59d82bbf8f4b6f687fd1a5`。
`baseline/`に3ファイルの原本、`candidate/`に研究copy、`candidate.patch`に可逆差分を置く。
`provenance.json`は全pinと、変更しないschedule/MCCFR/oracle等のcontext pinを持つ。
これはworkspace全体の実行snapshotではない。後続のbuild前には全workspace/lock/toolchainと
差分3ファイルをsnapshotへ固定する。Gitに収録されるまではGit-backed証拠とは扱わない。

`StorageOps::fused_update`と借用sliceだけの`CfrUpdate`を追加し、後者は`lib.rs`から公開する。
独自backendが新methodを実装しなくてもdefaultで従来の2段処理を受ける。F32Storage/F32Viewだけが
producerとstorage更新を融合する。MCCFRは従来methodを使い続ける。NLH固有の条件は導入しない。

## 算術・所有権の条件

acting-player nodeの子を全て評価し、旧順序で`node_cfv`を合成した後だけ呼ぶ。
F32の各要素について次の演算を同じ型・順序で行う。`pos/neg/avg`のf64→f32 castも従来どおり。

1. `delta: f32 = action_value - node_value`。
2. 更新前regretが`> 0.0`なら`pos`、それ以外なら`neg`を選ぶ。
3. `updated = old_regret * factor + delta`。`floor_neg && updated < 0.0`の場合だけ`+0.0`へ置換。
4. 全regretの更新後、`weighted: f32 = reach * strategy`を求める。
5. `reset_avg`ならその値を直接保存。その他は`old_sum * avg + weighted`。

減算・乗算のf32中間値を保つ。`mul_add`、逆数、積和の再結合、ゼロreachの省略、係数1の省略を
加えない。Vanilla、CFR+、DCFR、HS-DCFR等は同じDiscountsを消費するだけでscheduleを変更しない。
`-0.0 > 0.0`、`-0.0 < 0.0`の扱いも元式と同じ。reset時には旧sumを掛けたり読み出したりしない。
NaNやinfinityを新たに正当化しない。既存の比較・演算は保持するが、NaN payloadのbit同一性や
不正入力でのpanic途中のstorage状態は本候補の保証ではない。有限な合法入力のbit一致もnative検証前である。

defaultは旧solverの`cfvs -= node_cfv`→`update_regrets`→`cfvs = reach * sigma`→
`accumulate_strategy`を同じaction/hand順で実行する。I16実装と既存storage testsは原本bytesのまま、
regret block全体の量子化が完了してからstrategy block全体を量子化する。量子化scaleを共有・変更しない。

`CfrUpdate.action_values`は呼出し後の内容を指定しないscratch。defaultではweighted値になるが、
F32では元値を残す。solverは以後これを参照せず`Scratch::put`へ戻し、次の`take`が全体をzero化する。
node_values、reach、strategyは共有slice、storageは排他的viewで、safe Rustの借用を維持する。
F32Viewは従来の`local_offset`でrebaseする。storage split、CFV記録位置、出力`node_cfv`、親への
ordered foldは変更しない。H=0またはA=0ではループが空になり、除算やchunks_exact(0)は導入しない。
有効なStorageRef/slice次元が前提で、offset演算や次元上限は既存契約を使う。

二度のaction_values全書換えと、その後の読戻しを省く。これは論理的なbufferアクセスの削減であり、
DRAM転送量やallocation数の削減を測定した主張ではない。state/scratchの確保容量も変わらない。

## 準備と今後の検証

以下は小さなPython source検査だけを行う。生成は異なる既存出力を上書きしない。

```text
python -B experiments/hu-postflop-r1/flop-scaling/fused-update/prepare.py
python -B experiments/hu-postflop-r1/flop-scaling/fused-update/prepare.py --check
python -B -m unittest discover -s experiments/hu-postflop-r1/flop-scaling/fused-update -p test_prepare.py -v
```

Python testsは差分の可逆性、baseline/candidateへの無関係な変更の拒否、I16後半のbyte保存、
旧default変換・呼出し順、snapshot/provenance保存を確認する。Rust型検査や算術oracleではない。
ここではRustのcompile/test/solveを行わず、cloud操作も行わない。

後続で資源枠と予算を確保した後、まず隔離workspaceに3ファイルを適用する。
追加の[研究用native fixture](tests/fused_update.rs)はcandidate側の
`crates/engine/tests/fused_update.rs`へコピーして使用する。deployment先とpinはprovenanceに保持する。
同じcrate内の変更していない旧storage methodsを対照にする回帰試験であり、独立CFR oracleではない。
fixtureの5 testsは未compile・未実行で、研究source準備のPython成功件数には含めない。
現在のfixtureはfull/rebased split両方（global base=2、local offset=1）、F32/I16全stateとscales、
view内外の前後sentinel、9種類のschedule条件、
A=0/1/3とH=0/1/3/5、±0/subnormalを含む。A=0/H=0はpublic StorageRefを直接使う空更新試験で、
その次元のaction treeをbuilderが受け付けるとの主張ではない。resetと非finite旧sumの試験は、
有限合法solverの品質試験と区別したrobustness検査である。

```text
cargo test --locked --offline --release -p engine --test fused_update -- --test-threads=1
```

以下は未実行の検証要件であり、現時点の成功を示さない。

- F32 full storageと非zero offsetのsplit viewを旧method列と比較するnative直接test。
  split viewの前後に置いたsentinelが変わらないことも確認する。H=0/1/複数、
  A=0/1/複数、正/負/±0 regret、zero/非uniform reach、floor on/off、reset on/offを含める。
  subnormal・丸め境界・異なるdiscount係数も含め、比較はf32 bits。I16はstate/両scaleの全bitsを比較する。
- `cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、
  `cargo test --workspace`を通常の採用前必須検証とする。実行資源枠は別途固定する。
- 既存`engine`の`parallel`、`dimension_changing_transitions`、`value_scratch`、toy/oracle試験を使い、
  0/可変次元、両storage、CFV recording、checkpoint、1/2/4/8/16/32 workersを確認する。
  Flop/Turnのignored oracle・isomorphism・storage照合は影響する範囲を明示して別実行する。
- 性能は同一VM/boot/native buildでbaselineとcandidateを固定条件比較する。まずVM16と同じ
  narrow/expanded、F32/DCFR/N16、1/16/32 workers。state/quality全bytes一致を先に要求し、
  CFRとqualityを分けてwall/CPUを記録する。候補有利な条件の選び直し、失敗標本の差替えは行わない。
  受入閾値・順序・反復・有限資源・予算はその実行前protocolで固定し、今回の準備から推定しない。

外部実装を閲覧・転用せず、repository内の現行処理から作成した。ライセンスはrepositoryの
MIT OR Apache-2.0。[設計境界](../../../../docs/architecture.md)と
[license方針](../../../../LICENSE-POLICY.md)に従い、独立oracleと実装共有しない。
