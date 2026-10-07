# P1-Q1: i16 storage精度床の試作・ローカル計測

開始pot 5.5 BBに対する0.1%目標には、今回の範囲では **V1** を推奨する。
River 3000反復、Turn 1500反復の比較による仮選択であり、製品既定は変更しない。
base `6234545`、branch `s3-p1-i16-proto`の未commit変更。commit・push・branch操作なし。
隣のworktree・`.sol`実装・f32 storage・CFR更新式・schedule・木は変更していない。

## 実装とmemory

`I16Storage::new`時に`SOLVERS_I16_VARIANT=V0|V1|V2|V3|V4`を読む。未設定はV0、未知値はpanic。
V1〜V4はcheckpoint非対応。`arrays`/`arrays_mut`、state、snapshot、各restoreを明示panicで拒否する。
L = 1 arenaのaction×hand要素数、N = action node数、H = nodeごとのhand数の総和。
「要素」はregretとstrategy_sumの対を数える。表はarenaとnode/hand付随のheap容量である。

| 変種 | 実装 | bytes式 | bytes/要素（River / Turn） |
|---|---|---|---|
| f32 | 両arena f32（比較基準） | 8L | 8.0000 / 8.0000 |
| V0 | 両arena i16、node scale、最近接丸め | 4L+8N | 4.0064 / 4.0063 |
| V1 | V0＋確率的丸め | 4L+72N | 4.0579 / 4.0567 |
| V2 | 両arena i16、hand列ごとのi8指数、最近接丸め | 4L+72N+2H | 4.7518 / 4.7655 |
| V3 | V2＋確率的丸め | 4L+72N+2H | 4.7518 / 4.7655 |
| V4 | regretはV0、strategy_sumはf32 | 6L+8N | 6.0064 / 6.0063 |

V1〜V3は試作共通の`QuantMeta`をarena×nodeごとに保持する（x64で32 bytes = Vec header 24＋u64更新回数8）。
したがってnode付随はscale 8N＋metadata 64N。V1の指数Vecは空、V2/V3はさらに指数2H bytesを使う。
V2にも共通metadataの更新回数がある。製品化時には不要なVec header・V2のcounterを除去できる。
V4の未使用strategy scaleも試作では保持し、8Nへ算入する。列指数は初回更新で確保する。
上の指数2Hは全列確保時の式（小さいhand数ではVec最小容量の影響あり）。実arena容量と式の照合はJSONに保存した。
別途、storage本体はI16 200 / f32 48 bytes（x64）、scratchは4×容量要素数、並列view本体と各scratch、allocator管理が必要。
木・終端rank表・評価scratch・hash採取時の一時stateコピーはstorage arenaに含めない。RSS上限の式ではない。
`MemoryEstimate::i16_bytes`/`bytes_for`はこの試作ではV0のまま。計測は実arena容量を別途取得する。

確率的丸めはfloor＋Bernoulli（負値も同じ）、global要素番号・nodeごとのarena別更新回数・arena種別を
固定SplitMix64 hashへ入れる。worker番号・task順・view local offsetは使わない。
V2/V3は各列の最大絶対値が概ね16000〜32000になる2の冪を選ぶ（i8範囲−128..127）。
冪と指数はf32のbitsで求める。全ゼロ列は指数0。列内比のnormalizeはraw i16を使用し、
`raw_regrets`は列指数とnode係数で復元する。`scale_all`はnode f32係数へ掛け、次の更新で指数に吸収する。

## 収束・速度

DCFR既定（alpha=1.5、beta=0、gamma=3、pow4_reset=true）、rakeあり、targetによる早期停止なし。
指定のTurn木とRiver木を使用。50反復ごとに同じ木の厳密BRを評価し、
`(NashConv / 2) / 5.5 × 100`を%として記録した。Turnは参考計測が重かったため許可された1500反復まで。
到達iterationは50間隔での最初の観測値。途中で到達しても後から悪化し得るため、最終値と最良値も示す。
全曲線は`progress/*.jsonl`、集計・全引数・環境・hashは`summary.json`。

### River: 3000 iteration

| 変種 | ≤0.3% | ≤0.2% | ≤0.1% | ≤0.05% | 最終 % | 最良 % @iter | 秒/iter |
|---|---:|---:|---:|---:|---:|---:|---:|
| f32 | 250 | 350 | 500 | 800 | 0.005989 | 0.005989 @3000 | 0.00128 |
| V0 | 300 | 300 | 550 | 1150 | 0.036635 | 0.021716 @1800 | 0.00148 |
| V1 | 300 | 400 | 550 | 850 | 0.006863 | 0.006863 @3000 | 0.00222 |
| V2 | 250 | 300 | 500 | 1050 | 0.108321 | 0.028191 @1500 | 0.00310 |
| V3 | 300 | 400 | 550 | 900 | 0.007827 | 0.007827 @3000 | 0.00353 |
| V4 | 250 | 300 | 500 | 850 | 0.006566 | 0.006566 @3000 | 0.00158 |

### Turn: 1500 iteration

| 変種 | ≤0.3% | ≤0.2% | ≤0.1% | ≤0.05% | 最終 % | 最良 % @iter | 秒/iter |
|---|---:|---:|---:|---:|---:|---:|---:|
| f32 | 450 | 550 | 800 | 1400 | 0.041300 | 0.041300 @1500 | 0.31270 |
| V0 | 500 | 1050 | 未達 | 未達 | 0.336980 | 0.122534 @1050 | 0.36004 |
| V1 | 400 | 500 | 850 | 1350 | 0.053167 | 0.049535 @1350 | 0.39886 |
| V2 | 400 | 1050 | 1150 | 未達 | 0.073394 | 0.066169 @1400 | 0.43927 |
| V3 | 400 | 500 | 850 | 1500 | 0.049206 | 0.049206 @1500 | 0.52060 |
| V4 | 450 | 500 | 750 | 1450 | 0.042645 | 0.042645 @1500 | 0.33121 |

時間は全CFR更新区間の平均（評価・木構築・state hashを除外）、Riverは4 threads、Turnは8 threadsの参考値。
Intel Core i7-10700KF、Windows、rustc 1.97.0、release・target-cpu=native・CARGO_BUILD_JOBS=2。
混雑PCで別作業やcompile/testも走るため、厳密な速度順位・speedupの認定には使わない。

## 決定性・検証

全6 backendで、River 3000反復の60回すべてのplayer別Exploitability f64 bitsと最終storage BLAKE3がthreads 1/4で一致。
Turnも全6 backendの50反復で評価bits・全storage hashがthreads 1/4で一致。Turn 1500反復の全thread比較は未実施。
i16 hashは両arena、node scale、列指数、更新回数、V4 f32累積を含む（scratch・容量は計算stateではないため除外）。
指数計算のpowi/log2からbitsへの置換前後でも、River全12ケースの全state hashが一致した。
追加試験は再分割viewの全状態一致、小さいhand列・最小指数・discount・平均reset、
正負の確率的丸めの偏り、V4累積のf32一致、全checkpoint入口の拒否を確認する。
V0既定でfmt、workspace全target clippy（-D warnings）、hu-engine、hu-postflop、workspace testを実行。
各commandの最終結果とtest数は`summary.json`のvalidationを参照。ignored acceptanceは実行していない。

## 推奨と製品化の境界

第一候補は **V1**。両ケースの最終0.1%を満たす試作の中でarena memoryが最小の方式を選んだ。
V1のTurn arena容量はV0比＋1.26%、f32比50.71%。
V3はより多い付随memoryでTurn最終0.05%も満たす比較候補、V4は更新速度とf32累積の比較候補として残す。
Riverでは列scaleだけのV2は最終0.108321%へ悪化する一方、V1/V3/V4は0.01%未満を保った。
これは毎更新の最近接丸めによる小さい増分の消失という仮説を支持する。V4の改善は平均累積の寄与も示唆するが、
regretとstrategyの誤差寄与を完全に分離した証明ではない。追加のseed・木・Turn 3000超での床の確認が必要。

- checkpoint v4のbool backend＋4配列だけでは不足する。新versionでvariant、丸めalgorithm/seed identity、
  arena別更新回数、列指数、mixed i16/f32型を表現し、旧versionの受理方針・hash・長さ・streaming resume試験を決める。
- `MemoryEstimate::i16_bytes`を採用表現へ対応させ、allocation前にnode×hand付随を数える。
  parser/normalizer・runtime・validate出力・help・examples・tests・metadataも同期する。
- `docs/hu-postflop.jp.md` §4へstorage表現・丸め・決定性・精度境界、§7へcheckpoint state/versionと
  solution metadataの方式identityを明記する。`.sol` u16出力量子化の誤差は別問題として扱う。
- `docs/nlh-input-v1.jp.md` §10へ公開storageのliteral/既定・variant/seedの扱いを決め、
  CLI reference・user guide・compatibility hashへ同期する。環境変数を公開契約に黙って混ぜない。

## 再実行

```powershell
$env:CARGO_BUILD_JOBS = '2'
cargo build --release -p hu-postflop --example p1_i16_precision
$env:SOLVERS_I16_VARIANT = 'V1'
.\target\release\examples\p1_i16_precision.exe experiments/p1-perf-2026-10/i16-precision-20261007/configs/turn_i16.toml 8 1500 50 runs/turn_V1.jsonl
```

sourceは未commitのworkspaceにのみ保持する（ユーザー指定によりpatch/snapshot/旧sourceコピーは保存しない）。
base、変更source・Cargo.lock・build config・binary・config・progressのSHA-256をJSONに記録した。
計測は再実行・検査済みだが、将来の履歴からの再構築にはこの未commit sourceの保持が必要である。

2026-10-07追記: 試作worktree（`cisco-i16`、branch `s3-p1-i16-proto`）は利用者の承認を得て削除し、未commitの試作sourceは失われた。再現状態は`historical-only`（保持hashの検査だけが可能）。この証拠を基に利用者決定PF2・PF3でV4方式を選び、`storage = "i16-f32avg"`としてT6（`b347262`、[証拠](../mixed-storage-20261007/README.md)）で製品へ実装した。本directoryのSHA-256（LF正規化）は[manifest.json](manifest.json)。
