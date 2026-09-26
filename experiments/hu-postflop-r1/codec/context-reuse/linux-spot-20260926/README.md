# SOL context再利用のLinux Spot比較

VM08で3方式の`stream-write`を比較し、事前採用screenは不成立となった。
CCtx再利用candidateは採用せず、本体のwriterをbulk版へ戻す判断の根拠である。
以下に結果と、実行前に固定した計画を保持する。

## 実行結果

2026-09-26 01:30:21–02:00:26 UTC、AMD EPYC 7B12、2論理CPU（1物理core/SMT2）、
8 GiB、Rust 1.97.0、同一boot `ec1e2ee2-ab43-4ae0-b342-e656914bb92a` で実行した。
fmt、workspace Clippy、workspace tests **906成功/0失敗/31ignored**、文書検査、
共通Python検査、release SIGINT **1成功/0失敗**を通した後、3方式を別targetでbuildした。
通常testの対象は固定context候補であり、後続のcompact/並列化変更の検証ではない。

| 入力 | candidate中央値 | bulk中央値 | legacy中央値 | candidate/bulk |
|---|---:|---:|---:|---:|
| River | 4.484429 ms | 4.086059 ms | 3.819699 ms | 1.09749 |
| Turn | 8.802418 ms | 9.350687 ms | 17.239196 ms | 0.94137 |
| Flop | 86.362559 ms | 87.861279 ms | 241.918503 ms | 0.98294 |

9 warmupと63測定の計72標本が正常終了。全再保存SOLは元入力と全byte一致し、
全方式・全反復のcanonical/root byte列も一致した。Riverの1.05以下、Flopの0.95以下という
事前screenを満たさない。小さいRiverは数msで変動があり、この1回のscreenから一般的な
速度退行を断定しないが、採用を支える結果ではない。Flopでbulkがlegacyより速い結果も、
異なる旧host上の回帰原因の解明とは扱わない。solve全体・品質・R1受入を認定しない。

[complete-proof](complete-proof/) はsource archives、固定binary、全supervisor記録・stdout/stderr・
RSS samples、各resultとcanonicalをSHA別gzipで保持する。[検証結果](complete-verification.json)は
83段階/72標本を検査し、容量制約で外した22重複aliasも同じ元byteの保持物へ結び付け、
欠けた一意payloadが0件であることを確認した。元bundleは13,224,624 bytes、SHA-256
`d11e9f59140839abb166327fdf9caa327efe98cf80344e22b67e39ebbae6939c`。
compiler/Python実体はhash識別のみ、compiled target binaryは同一byteのfrozen copyで保持する。
[build-proof](build-proof/) は測定前に回収した独立のbuild完了記録。
Riverの全24標本では短いprocessをRSS samplingが捉えず、sampled peakは0だった。
これは未観測であり、使用メモリ0やメモリ削減を意味しない。

```text
python experiments/hu-postflop-r1/codec/context-reuse/linux-spot-20260926/verify_retained.py experiments/hu-postflop-r1/codec/context-reuse/linux-spot-20260926/complete-proof --expect complete
```

## 実行前に固定した計画

candidate、bulk byte serde版、旧byte serde版を同じLinux host・入力・release条件で
比較し、保存時間とprocess全体の資源使用、生成byte列を記録する。
[既存の126標本計画](../../write-phases/README.md)とは問いと標本構成が異なり、
その区間計測・計測摂動の校正・旧Flop回帰原因の認定を代替しない。
以下は計画の記録であり、実行証拠は上記の保持物である。実行条件の機械可読な固定値は
[protocol.json](protocol.json)、実行・検証は[run.py](run.py)に置く。

## 実行枠

VM08の予定枠はGCP Spot `e2-standard-2`、2 vCPU、8 GiB RAM、40 GB disk、
最大6時間、予約額上限2米ドルである。R1全体の許可済み累計30米ドル枠に含める。
既存実験分の20米ドルを引き続き保留し、VM08へ2米ドルを新たに予約した後の未予約枠は8米ドル。
この予約額は実際の請求額ではない。起動前の料金確認、残予算との照合、絶対終了時刻、
外側の有限systemd cgroup、転送・保持・VMとdiskの終了処理はroot担当が管理する。
runnerだけを動かしてVM費用の上限を保証したことにはしない。

source、input、実行記録は`target/`へ置かない。Cargo出力だけを
`/opt/r1/vm08/target-context`以下に置き、固有の証拠は回収・hash照合してから資源を終了する。
同じVMでbuild、他実験、証拠転送と計測を重ねない。

## 固定する3方式と入力

| arm | 対象 |
|---|---|
| `candidate` | 現行のCCtx再利用writer。`crates/formats/src/sol_indexed.rs`は26,229 bytes、SHA-256 `f7c4488d81b619cd7a22f627b592148908ca6d359dfceaa2722876b5835b2f1f` |
| `bulk` | Git `d3bbb2766e2e63d0f065f055eef1bac77016f377`のbulk byte serde版 |
| `legacy` | Git `88ffa5dd4583e5c8bae84e20e9b8390cb719e4f0`の旧byte serde版 |

candidateのwriter hashだけでworkspace全体を識別したとはみなさない。
各armの完全なsourceは、root、revision、archive、manifestを含む
`/opt/r1/vm08/packs/sources.json`で指定する。manifestは
[pack-source.py](../../../cloud/pack-source.py)の形式を使い、archiveと展開fileを照合する。
必要なexampleを補う場合も、その実際のbyte列をmanifestに含める。
比較結果をCCtxだけの効果と解釈する前に、arm間のsource差分を確認する。

入力は[固定codec inputs](../../inputs.json)のRiver、Turn、Flop各1件である。
`/opt/r1/vm08/inputs/river.sol`、`turn.sol`、`flop.sol`として配置し、byte数とSHA-256を照合する。
新しいsolveや入力生成はこの実験に含めない。

## 実行順と資源制御

3 armのCargo targetを、実験開始時にそれぞれ別の空directoryとして用意する。
candidateのfmt、clippy、通常workspace tests、文書検査、共通Python tests、
Unix SIGINT試験を先に行う。その後、3 armのcodec exampleを各専用targetで
release buildし、生成binaryのhashを固定する。candidate内ではSIGINT試験のrelease依存を
codec buildで再利用する。別armのtargetから得た古い実行fileを使わない。
source、入力、binary、boot identityとCPU情報を記録・照合し、条件が変わった標本を成功扱いしない。

標準toolchain実体directoryは
`/opt/r1/rustup/toolchains/1.97.0-x86_64-unknown-linux-gnu/bin`。
`CARGO_BUILD_JOBS=1`、`RAYON_NUM_THREADS=1`、`RUST_TEST_THREADS=2`、
dev/testのdebug情報0、incremental無効を固定する。
effective compiler flags、Cargo.lock、toolchainとbinaryの識別を結果に残す。

| stage | 上限 |
|---|---:|
| clippy | 1,800秒 |
| 通常workspace tests | 2,400秒 |
| candidateのSIGINT試験release compile（`--no-run`） | 1,800秒 |
| 各armのrelease example build | 各1,800秒 |
| Unix SIGINT試験の実行 | 180秒 |
| 共通Python tests | 300秒 |
| 文書検査 | 60秒 |
| 各codec標本 | 60秒 |

process treeのRSS上限は6 GiB、host空きmemoryの下限は768 MiB、disk空きの下限は4 GiB。
これはsampled RSSの監視値であり、cgroupの使用量や厳密な瞬間peakとは異なる。
外側のcgroupとVM期限も必要となる。各stageの残時間が終了・cleanupまで収まる場合だけ開始し、
timeout、資源停止、監視失敗、残存process、identity不一致を成功や速い標本に置き換えない。

Unix SIGINT試験は、candidate専用targetでCLI integration testを`--release --no-run`で
compileしてから、`a_canceled_heads_up_solve_closes_as_canceled_and_resumes`を
`--release`と`--ignored --exact --test-threads=1`で1件だけ明示実行する。
この試験はKuhnを使い、solveとresumeへの実SIGINT、exit 130、取消状態、再開後の進捗を
確認する。既存testの指定するrelease profileを使うが、NLH Postflopの中断・再開同値性を
認定するものではない。

## 標本構成と比較範囲

各case・各armを1回ずつwarmupし、9実行をwarmupとして保持する。
測定は各caseについて7 block、各blockで3 armを1回ずつ実行し、63実行とする。
合計は72実行であり、warmupは性能集計から除外する。
7 blockは3 armの全6順列と追加順列1つから構成する。全6順列では各armが各実行位置に
同数現れ、追加順列はcase間で回転させる。この7 block全体を固定seed `20260926`でshuffleする。
実際の固定順序を実行前に保存し、結果を見て順序、回数、除外条件を選び直さない。

各実行は次の1回の保存を行う。

```text
sol_codec_bench INPUT.sol stream-write 1 NEW-OUTPUT-DIRECTORY
```

exampleの保存時間は、全payloadを読込済みの状態から`write_sol`を呼ぶ区間であり、
chunk生成・圧縮・書込み・同期・置換を含む。入力読込み、保存後読戻し、canonical生成は
その保存時間の外側にある。supervisorの経過時間とRSSはこれらを含むprocess全体の値であり、
writer単独のpeakとして扱わない。

すべてのarm・warmup・測定で、再保存SOLと元入力の全byte一致、全payloadとrootの
canonical byte列の一致を検査する。source、入力、binaryの実行前後識別とともに保持する。
cached metadataのEVやExploitabilityを読み戻してもBRを再評価したことにはならない。
この比較は固定3入力のcodec試験であり、solve全体の改善、全NLH条件の品質、R1受入へ外挿しない。
CCtxの再利用による確保回数の削減とprocess peakの削減も区別する。

## 実行前に固定する比較基準

全byte照合の成功を前提として、今回の小規模比較に対する記述的なscreenを新しく定義する。
各比率は、warmupを除く7実行のcandidate保存時間中央値を、比較先の7実行の保存時間中央値で
割った値である。同じblock内のcandidateと比較先の時間・比率も、7組すべて提示する。

- 全3 caseで、candidate / bulk、candidate / legacyの両中央値比が`<= 1.05`。
- 主対象のFlopで、candidate / bulkの中央値比が`<= 0.95`。
- Flopの7組中4組以上で、candidateの保存時間がbulkより厳密に短い。

5%幅は従来の記述的な改善幅に対応する対称な幅として固定したもので、統計的有意性や
既存の正式な受入閾値ではない。標本数・順序・この基準を今回の結果を見て変更しない。
有効な全標本が揃っても基準を満たさなければscreenはfalseとする。これは実行失敗とは区別する。
欠測、byte不一致、監視失敗を除外して残った標本だけでscreen成功とはしない。

## 実行と保持後の検証

root担当がsource/inputの準備と外側の有限実行枠を確認した後、VM内で実行する。
`--deadline-utc`には、予約済みの絶対終了時刻から回収・cleanup時間を確保したISO時刻を渡す。
再実行でこの期限を延長しない。outputと3 arm用targetは新規directoryを使う。
build phaseは検証とbuildを終えると`ready_for_measurement`で終了する。
root担当がこの待機中に完了済みbuildログを回収して転送を終え、その後にmeasure phaseを開始する。
measure phaseは保存済み計画と期限を使用する。

```sh
python3 experiments/hu-postflop-r1/codec/context-reuse/linux-spot-20260926/run.py run \
  --phase build \
  --sources /opt/r1/vm08/packs/sources.json \
  --inputs /opt/r1/vm08/inputs \
  --out /opt/r1/vm08/context-run \
  --targets-root /opt/r1/vm08/target-context \
  --deadline-utc ISO

python3 experiments/hu-postflop-r1/codec/context-reuse/linux-spot-20260926/run.py run \
  --phase measure --run /opt/r1/vm08/context-run

python3 experiments/hu-postflop-r1/codec/context-reuse/linux-spot-20260926/run.py check \
  --run /opt/r1/vm08/context-run
```

この`check`は、source、archive、toolchain、build生成物、標本を記録済みの絶対pathで
読み直すVM内検証である。回収先へ移したdirectoryだけでは再実行できず、pathを書き換えた
記録を元の実行記録として扱わない。root担当が回収時に別の保持manifestを作り、回収fileの
size/SHA-256を元記録へ照合する。保持後のbyte照合と、VM上の実行・検証成功を区別する。

実行計画、source/archive manifest、toolchain・binary・host識別、標本順序、各stageの
stdout/stderr・supervisor記録・resource samples、結果、byte照合とcheck出力を保持する。
回収後にも保持fileのsize/hashと検証結果を確認する。未完了stageや失敗の元記録を残し、
成功した再確認で上書きしない。採否と性能の主張は、この証拠を照合してから別途記録する。
