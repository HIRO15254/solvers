# rank groupの52-card scratchを再利用する研究候補

F64でmassを厳密に表現できるcompact showdownに限り、各rank groupで作っていた
52-card配列をsweep全体で1個だけ保持する。group内の全tie評価が終わってから、
`below_card` をcombo順に加算する既存loopで、非zero opponent reachが触れたcardだけを0に戻す。
同じcardを複数回clearしても、その時点ではgroup sumsを参照しないため残留値はない。

group total・card sums・prefix sums・win/tie/lose・payoffの算術順序を維持する。
開始時の全体compatibility sum、fold、F64 gate、整数fallback、汎用engine、`cfr-ref`は変更しない。
対象は全レンジ表現の変更や別のEQアルゴリズムではなく、既存O(n) kernelのscratch初期化だけである。

VM20の失敗campaignから独立に検証した単一narrow・16-worker profileでは、
compact compatibility sumのleaf period比が約15.13%、terminal evaluator約15.81%だった。
この事後診断は候補選定の根拠に限る。all/group/foldの比を分離しておらず、
group clearがその全コストを占めることや、候補の高速化・32-worker scalingを証明しない。

## 保存物と適用

- `candidate.patch`: production `crates/holdem/src/kernel.rs`への限定差分。
- `kernel.rs`: GCP用の研究コピー。productionファイルは編集していない。
- `provenance.json`: 元source、候補、差分、生成controlのhashと未検証範囲。
- `prepare.py --check`: 変更範囲と生成物を再現。rustfmtだけを呼び、Cargoは呼ばない。
- `prepare.py --apply-to <fresh-source-copy> --receipt <new.json>`:
  元source pinと差分を確認し、別source copyのkernelだけへ適用する。
  fresh receiptと適用後hashを残す。production root・path escape・変更済み元sourceは拒否する。

`cfg(test)`内に旧F64 compact algorithmのprivate referenceを保持し、candidateとは別に
52-card group arrayを毎回作る順序と全f32出力bitsを比較する。これはoperation-order regressionであり、
独立poker oracleの代替ではない。既存quadratic kernel tests・frozen `cfr-ref` differentialは引き続き必要。

追加したRust testは次の4件。いずれもGCPでのcompile/run前であり、成功したとは扱わない。

1. rank間でcardが繰り返され、両席supportが異なるケース。
2. opponent不在・正負zero・全zero・単一非zeroとtie group。
3. 120個の固有card pair、多数のtie、非dyadic f32・subnormal reach。
4. 空tableと未更新outのsentinel bits維持。

## 比較前に満たす条件

GCPの小型VMで候補4test、既存kernel regression、oracle/parallel/storage regressionを検証する。
本体採用前にはリポジトリ必須の `cargo fmt --all --check`、
`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace` を同じsourceに対して完了する。

時間比較は同じmachine・boot・compiler・build flags・case・range・iteration数でbaseline/candidateを
交互に複数回実行し、warmupとCFR・BR/EV・保存のphaseを分ける。narrowとexpanded、16/32 workersを含める。
両席の全state byteとEV/BR/NashConv・Exploitability bitsを保存前後でbaselineに照合する。
F64 gate外の代表ケースも既存oracleで確認し、同等品質を維持した計測だけを性能判断へ使う。

採用guard・反復数・品質目標・case集合は比較を走らせる前に親campaignで固定する。
未完了・timeout・quality不一致・部分profileは性能合格にならない。process peakは静的配列サイズと分け、
全工程の時間やメモリが悪化する候補はkernel microbenchmarkだけで採用しない。
ローカルではsource加工・rustfmt・小さいPython source guardのみを行う。
