# Showdown/Foldの事前計算による実測

2026-09-26 UTC、T1-08 / SOL-15の実装比較。役強さによるO(n)走査は以前から
存在した。今回、各handのカード番号と両seatのlocal indexも事前計算し、
terminal評価時の再計算と全1,326 comboへの一時展開を取り除いた。
固定4ケースすべてで出力を維持し、事前の速度基準を満たしたため採用する。
外部参照に対する品質認定、R1全体の受入、極小weightの数値問題の解決は含まない。

## 測定結果

同一bootのIntel Xeon 2.20 GHz、e2-standard-4（4論理CPU / 2物理core、16 GiB）。
F32、compact layout、1 worker。ケースごとにwarmup新旧1組、その後3組を交互実行。
全32回のうち24回を時間集計に使い、warmupも含む32回を数値照合に使った。
反復回数・順序・閾値は[protocol.json](protocol.json)で測定前に固定した。

| ケース | 反復数 | 旧run秒の中央値 | 新run秒の中央値 | 短縮率 |
|---|---:|---:|---:|---:|
| River | 1,000 | 8.763098 | 6.285461 | 28.27% |
| Turn | 1,000 | 1.731362 | 1.292089 | 25.37% |
| Flop | 50 | 0.454137 | 0.406426 | 10.51% |
| 狭いRiver | 10,000 | 0.092674 | 0.065149 | 29.70% |

各ケース3組とも新方式が速かった。新/旧の中央値比の幾何平均は0.761783。
事前基準「幾何平均≤0.95、各ケース≤1.05」を満たす。
比較区間はsolverの`run_seconds`のみで、構築・初期化・query・artifact保存は除く。
それらの時間もraw JSONに保持している。3組だけの記述的比較であり、信頼区間や
別CPU・別レンジでの保証にはしない。特に狭いRiverは100 ms未満と短い。
以前の32 vCPU実験とは別hostであり、数値を混ぜず、この変更の32-thread効果も推定しない。
[独立監査](independent-audit.md)で構築・初期化の中央値も分離した。
Turnの構築時間は約0.110 ms増えており、全工程が短縮したという意味ではない。

全ケースで旧新8回ずつの`canonical.bin`（strategy/CFV）、`state.bin`
（regret/strategy-sum）の元bytesが一致した。EV/BR/NashConv等の品質bits、
global combo IDs、正規化config、木構造、rake/utility条件も一致した。
同一の反復で同一の解を得る時間の比較であり、外部solverとのExploitability認定ではない。

## アルゴリズムとメモリの範囲

完成boardごとに役強さを一度評価し、`(rank, global combo)`でsortして共有する。
terminalでは同rankのgroupと、それより弱いgroupのprefixを順に走査する。
相手reachの合計とカード別合計から自分の2枚と衝突するhandを除去し、
tie/全体では二重に除去した同一comboを1回加える。
deckサイズ固定なら相手・自分の初期supportの和集合に対してO(n)。
sortと役判定を繰り返す処理ではない。

今回の8-byte `RankEntry`はu16 rank、u8カード番号2個、u16 local index2個を持つ。
旧`(HandRank, u32)`も8 bytesなので、tableの1要素あたり保持量は増えない。
元のsort順と浮動小数点加算順を維持する。local indexは後続の公共カードで変えず、
既存のchance maskでdead handのreachを0にする。builderがtableを変換する際は
一時allocationが必要。保存形式・初期supportの定義・公開APIは変更しない。

旧compact kernelの2×1,326要素f32 stack配列、計10,608 bytesをソース上で除去した。
これは実測RSSの10,608-byte削減という意味ではない。native `wait4`のpeak RSSは
全標本52,080,640 bytesで一定で、sampled process-tree peakはそれより小さかった。
この差をsolver固有のメモリと解釈せず、RSS削減の採用根拠には使わない。
sampled値は100 ms間隔で短時間peakを見逃し得る。どちらもsolve区間専用のRSSではない。

既存の極小weightの桁落ちは、この加算順保存の変更でも残る。
[既存境界の監査](../range-scaling/source06/normalizer-edge-audit.md)と
[未採用のexact-weight設計案](../../../docs/research/2026-09-27-postflop-exact-weight-proposal.jp.md)
は別の根拠・設計として扱う。

## コードと検証の識別

旧sourceは`d8135a12c22c9d5e70216380298bda480e63ad05`。新sourceは同commitを基点に
kernel、postflop builder、6個のkernel test、設計説明を変更したsnapshot。
manifestには当時のcloud予約台帳差分も含まれる。後付けの報告や研究提案は測定sourceに含まれない。

| 対象 | SHA-256 |
|---|---|
| 旧source archive | `f8aa610da21e9a6115d485f362b637c78befd56ce38f67da1b63cfa382bd3d6b` |
| 新source archive | `a159aebe99e10410ad9e72f523fae5fc8127ee94f30d9d13c3fbb651b0addc13` |
| 旧release binary | `eb3b59e16012090113ea214d2f9e959fef25103b6691e07396524b5b04a02f45` |
| 新release binary | `54b85c3886de32e45510b9bee2ff71e0f9440c07e3ee9e224c034da54e94145a` |

採用根拠に使う旧新buildと32標本はすべてboot
`ac2eeb5d-385c-4ea1-95b6-1b545ce65a85`。
新sourceの`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、
`cargo test --workspace`が成功（933 passed、31 ignored、0 failed）。
追加のrelease oracle 3件とriver resolve 1件も成功。旧sourceはこのbootでは
release buildのみで、旧sourceの全workspace testを再実行したとは扱わない。

6個の直接testは旧global kernelとのbit一致と、独立O(n²)のpair列挙を照合する。
両seat、非対称・重複support、全support、win/tie/lose別、非zero tie utility、
zero/sparse reach、後続board mask、互換pairなし、既存の極小weight丸めを含む。
凍結済み測定器のPython test 16件も成功した。`crates/cfr-ref`は変更していない。
採用時の[全201 crate file照合](current-source-verification.json)でも、測定snapshotとの
file集合・全bytesの一致を確認した。

## 失敗・再準備と資源

最初のbootは15:01:42 UTCにSpotで中断した。新sourceの検証process記録は完了を示すが、
再起動後の回収で3つのraw fileが記録hashと不一致だった。
[firstbootの検査](firstboot/README.md)は不合格のまま保持し、完全検証の根拠にしない。
旧buildは中断され、このbootの性能標本は0件。停止前後の時間も混ぜない。

同じVM・diskを同じ予約と絶対STOP期限内で再起動し、旧新とも別targetへfresh buildした。
最初の測定prepareはCPU quota記録が空のため検証器が拒否した。測定開始前で全32件pending。
`kernel-control-evidence.tar.gz`にそのplan/result/index、制御scriptを残した。
CPUAccountingとCPUQuota=400%を明示した新しいservice/outputで再準備し、32件を完走した。
source、runner、反復数、閾値は変えていない。serviceの詳細は
[journal](vm09-final-journal.txt)とproof内host記録にある。

成功proofはVM側でfsyncしたarchiveを回収し、手元でも全payload hashと数値一致を確認した。
raw検証logのclose/hashだけではSpot停止後の永続性を保証しないため、完了記録のみで採用しない。
VM09と40 GiB diskは回収後に明示削除し、[cleanup記録](cleanup.json)と
instance/disk/addressの取得結果を保持する。料金は累計$40上限に対し予約保持$29、未予約$11。
実請求は未確認で、予約額を請求額として扱わない。

## 保持と再検証

`kernel-proof02.tar.gz`は8,089,211 bytes、SHA-256
`28707ceba11324ddc8aef84a740e3c334ec59b19dac0e1c5ec9ed7f29c3bab69`。
Git内のself-contained archiveにsource2組、source manifest、実行binary2個、
build/検証記録・raw log、32標本の記録・raw出力・canonical/stateを重複排除して保持する。
元の絶対pathとpayload hashの対応はarchive内`proof/retention.json`。
コンパイラ/Python自体はidentityのみ。保持したsourceやbinaryを検証時に実行しない。

archive hashとサイズを隣接JSONに照合し、新規scratchへPython 3.12以降の
`tarfile.extractall(..., filter="data")`で展開して、次を実行する。

```text
python -B experiments/hu-postflop-r1/showdown-kernel/verify.py --out <scratch>/proof --expect completed
```

[ローカル検証結果](verification.json)を保持した。これは測定binaryの再実行ではなく、
raw evidenceの再照合。実行後source照合はVMで記録したlive rehashであり、
独立取得した実行後filesystem snapshotではない。
