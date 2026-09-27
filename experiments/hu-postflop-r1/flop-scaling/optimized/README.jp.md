# 全street Flopの最適化buildと状態照合

元版とflat chance出力候補を、runtime依存も含めて新しいCargo release targetへ
コンパイルした。narrow／expandedそれぞれの1／2 workers、計8条件で2反復を実行し、
各入力の全F32状態と公開EV／BR／seat別gainが一致した。narrowは前回debugの保持物とも
全bytesが一致した。[保持manifest](proof01/manifest.json)で実行と成果物を固定する。

| 入力 | 開始support | 全状態bytes | 4条件の状態・品質 |
|---|---:|---:|---|
| narrow | 34／30 | 81,414,344 | 完全一致、前回debugとも一致 |
| expanded | 63／160 | 283,677,926 | 完全一致 |

同じ入力・反復数での正しさ検査である。収束、同等品質までの時間短縮、32 workersの
並列効率、候補の採用、R1受入は認定していない。ローカルの別計算が継続中で、
各条件1実行・正式性能protocol外のため、raw時間からspeedupを算出しない。

## buildの範囲

[build.py](build.py)は現行cards／engine／game／hand-index／holdemの全sourceと
[共通adapter](../native-solve/solve.rs)を新規`.cache/`子ディレクトリへ複製する。
5crateのruntime dependency／featuresは保ち、研究用manifestからdev-dependenciesと
bench宣言だけを除去する。frozen oracleは複製・変更・リンクしない。
workspace membersは5crateとadapterに限定する。

元Cargo.lockを渡してoffline metadataで未使用packageを整理し、残るregistry 31packageの
name／version／source／checksumがすべて元lockに存在することを確認した。
releaseは元profileのthin LTO／codegen-units 1と既定opt-level 3、
元`.cargo/config.toml`のtarget-cpu=nativeを使用した。host用build script／proc macroには
Cargoの別profileが適用される。これは通常workspace全targetの検査ではない。

実体cargo／rustcで`build --release --offline --locked -j1`を実行した。
両armはそれぞれ空のtargetからbuildし、前回debug rlibをリンクしていない。
コンパイラはrustc 1.97.0、sourceは[manifestの保持時HEAD](proof01/manifest.json)と
各source hashで識別する。
productionは既存sourceから変更しておらず、flatのsolver.rsだけが
SHA-256 `4a58bfa9bfa384e98e5a92f477f6322d39baff975a3810ceef8533f5b6fabafa`の研究候補。

両buildはJob全体commit上限512 MiB、wall上限180秒、低優先度、1 Cargo jobで成功。
OSのJob peak commitはbaseline 439,816,192 bytes、flat 440,287,232 bytesだった。
これはCargo子孫を含むbuildの観測値で、solverメモリではない。
build sourceの前後hash、全command、stdout／stderr／resource samplesを保持する。

## solveの範囲

[run.py](run.py)が実行前に8条件と停止条件を保存した。
各入力内でbaseline 1、baseline 2、flat 1、flat 2の順に実行する。
すべてF32／DCFR、plannedも2反復、depth 2／min_children 12。
fixtureの型付きAPI再現であり、CLI TOML normalizationの検査ではない。
各processのwall上限60秒、Job commit上限512 MiB、開始時host available commit下限
1.5 GiB、低優先度を適用した。全8実行の終了・identity不変・子孫清掃が成功した。

各入力の元stateを直接stream比較したうえで全SHAも照合した。

- narrow: `35981691a7736e2df5a89734b1102ee8ffc1bacaab7719476b0152e459eb2700`
- expanded: `87d31b1da7e9b766cc09b02a51da74169167d705b104e9a60a1e4eeeb20f9cdd`

品質APIのf64 bitsとJSON bytesも各入力の4条件で一致する。
seat別gainの和であるNashConvはnarrow 474.61833541256254 chips、
expanded 460.08679897264506 chips。通常の零和Exploitabilityはその半分で、
いずれも未収束。EVとgainの基準は[adapterの説明](../native-solve/README.md)に従う。

1 workerではchance budgetが0となり、候補の並列chance出力経路は通らない。
2 workersがこの変更を実際に通る条件で、1 workerは対照として扱う。
CFV、SOL／checkpoint、外部参照との照合、全workspace testsはこの実験には含まない。

## 保存と再検証

105 payloadを保持する。両buildの全record、source snapshot archive、binary gzip、
8実行のrawと結果を含む。expanded stateは1本に重複排除し、gzipで保持する。
narrowは[前回の共通state](../native-solve/proof01/shared-state.bin.gz)を参照する。
4本ずつの元state hashと直接比較結果はmanifest／executionに残しており、
8本の独立した全stateを保持したという意味ではない。

source snapshot、生成lock、registry checksum、binaryは保持するが、compiler本体や
registry package archiveは同梱しない。target-cpu=nativeのためbinaryの別機種への
互換性は保証しない。保持byteの照合と、独立した環境での再buildは別の確認になる。

再buildにはRust toolchainとlockのpackageがofflineで利用可能なregistry cacheが必要。
flat入力の`.cache/r1-flop-native-flat01/solver.rs`が存在しない場合は、
`flat-chance/prepare.py --source-root . --out .cache/r1-flop-native-flat01`で再生成できる。
既存の出力を上書きせず、build.pyには新規の`--out runs/...`、`--source .cache/...`、
`--target target/...`と`--arm baseline|flat`を渡す。生成hashが固定候補と異なれば停止する。

```text
python -B experiments/hu-postflop-r1/flop-scaling/optimized/verify.py
```

検証結果は[checks01](checks01/)に保持する。
