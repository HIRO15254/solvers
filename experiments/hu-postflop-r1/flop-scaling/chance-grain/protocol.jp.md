# CFR chance depth 1/2 の固定比較

これは未実行の研究用手順であり、production/default の変更や VM 起動の許可ではない。
元本体 `solver.rs` SHA-256 `69063b31c3433b2eb4798b2f41015311200301a7ba1887b05bbefdb936d4e81a`
をそのまま使い、公開 `ParConfig` の CFR `chance_depth` だけを 2 または 1 にする。
品質評価前は必ず深さ2へ戻す。`min_children=12`、演算、fixture、F32/DCFR、反復16、
CFV capture 無効は共通である。flat / worker Scratch / fused update は混ぜない。
[提案とコード根拠](../../../../docs/research/2026-09-27-r1-flop-parallel-grain.jp.md)を具体化するが、
ビルドは費用節約のため別bootの2論理CPUで行う portable 方式に固定する。

## ビルドと計測の境界

`run.py prepare --source SOURCE --workspace NEW_WORK --out NEW_PROOF --cargo CARGO --rustc RUSTC
--build-deadline-utc UTC` → `build --out PROOF` を2論理CPUで実行する。
原208fileの全workspace snapshotに、同一の `adapter/solve.rs` を
`crates/holdem/examples/flop_chance_grain_probe.rs` として追加する。元sourceは変更しない。
package schema は `r1-chance-grain-package/v1`。package manifest、installation、全source、
controls、compiler/Cargo、archive、生成binaryをhashで結び、原本をproofへ保持する。

Cargoはoffline/locked/release/jobs2、空の専用targetを使う。
`RUSTFLAGS=-C target-cpu=x86-64-v3` を明示し、`CARGO_ENCODED_RUSTFLAGS`、
`RUSTC_WRAPPER`、`RUSTC_WORKSPACE_WRAPPER`、`RUSTUP_TOOLCHAIN`、`CARGO_PROFILE_*`
を除く。profileは保持したworkspaceのthinLTO/codegen-units1/opt3である。
事前の `cargo fetch --locked` はrootの2CPU bootstrapで行い、このrunnerは取得もインストールもしない。
nativeへのfallback、別CPU用の再build、測定条件ごとのbinary変更はしない。
環境優先順位は[Cargo公式](https://doc.rust-lang.org/cargo/reference/config.html#buildrustflags)による。

2CPUで `rustc -Vv`、example build 1回、engine/holdem/cfr-refの `cargo test --tests`
（test threads1、ignoredは実行しない）を完了する。mapped chance F32/I16、zero/variable
value spaces、selected CFV、parallel determinism の指定6test名の成功出力も必須にする。
build/coreの個別上限は各480秒、build全体はprepareから最大1200秒で、全体期限は延長しない。
immutable `build.json` は全build原本のmembership/hash、binary、成功stageを保持する。

同じinstanceをstop/resize/rebootした後、32論理CPUで
`measure-prepare --out PROOF --measurement-deadline-utc UTC` → `measure --out PROOF`。
build全原本とbinaryを再hashし、別boot・同instance・同binaryを明示する。
`measurement.json` はbuild receipt/planのpinを持つ。測定中のboot/host/source/tool/controlsは
各stage前後に一致を要求する。回収readerは現在hostへ依存せず、保持した2CPU buildbootと
32CPU measurementbootを別々に検証する。再起動後の測定再開や標本の混合は不可。

## Portable v3 の実行前検査

build前と計測前、およびstage境界で、全allowed CPUのflagsとglibc loader
`/lib64/ld-linux-x86-64.so.2 --help` の `x86-64-v3 (supported, searched)` を保存・要求する。
loaderは環境による偽装を避けるため `LD_*` と `GLIBC_TUNABLES` を除き、`LC_ALL=C` で実行する。
glibcの[usable ISA検査](https://raw.githubusercontent.com/bminor/glibc/glibc-2.36/sysdeps/x86/get-isa-level.h)
はハードウェアCPUIDだけの判定ではない。追加のkernel flags検査は保守的で、
v2相当flagsに AVX/AVX2/BMI1/BMI2/F16C/FMA/LZCNT/MOVBE/XSAVE を要求する。
Linux表記の `pni` と `abm` を用い、公開されない `osxsave` 文字列は要求しない。
[Linux公式説明](https://docs.kernel.org/arch/x86/cpuinfo.html)に従い、欠落をCPU非対応の証明とはせず、
本実験の実行条件不足として停止する。測定guestは16core/32logical、32 affinity、CPU quota≧32を必須とする。

## 固定38solve

単一binaryのCLIは `CASE WORKERS ITERATIONS CFR_DEPTH NEW_OUTPUT`。
先に narrow/N2 の workers1/32 × depth2/1 の4smokeを行い、最初のdepth2をcanonicalとする。
次に narrow/expanded 各N16・workers1・depth2の2canonicalを得る。これらのCFRは各4秒以上を要求し、
未達なら反復数を増やさず停止する。N2とN16のstateを混同しない。

matrixは case2 × depth2/1 × workers16/32 × round0/1/2/3 = 32。
round0の8本をwarmupとして除き、残る24本（各group3標本）だけを集計する。
caseを外側で固定し、偶数roundはworkers昇順・depth2→1、奇数roundはworkers降順・depth1→2。
計38solve、build側3stageを含め41stage。最初の失敗で残りをskippedにし、再試行しない。

全38solveで全F32 stateのbyte比較とquality JSONの全byte比較を要求する。
既存state container `R1F32S01` とquality定義を変更しない。`grain.json` は別の観測出力で、
各深さのeligible chance node/child edge数を計時外に構造走査で数える。
chance budgetは並列化したかに関係なくchance通過ごとに減る。これらのcountはRayon task数や
反復・seat pass数・実行時間を意味しない。品質7walkの設定は両条件とも深さ2。

## 保存・時間・資源

成功stateは3本（narrowN2/narrowN16/expandedN16）のlossless gzipを保持し、
全byte検査→fsync receipt→重複raw削除の順を守る。元canonical rawはworkにのみ置く。
失敗/部分rawは成功と扱わず、時間が残ればlossless gzipと専用receiptを保持してからrawを除く。
圧縮やreceiptに失敗すればrawを残す。retention gateの二次失敗で元の失敗理由を隠さない。
proof上限240MiB、外側archive上限256MiB、総転送枠512MiB。任意の破損rawが圧縮上限内に
必ず収まるとは主張せず、削除して合わせない。元source archive上限1MiBは起動前packでも確認する。

buildのprocess RSS上限4GiB/outer6GiB、計測はprocess8GiB/outer12GiB、swap0、CPUWeight100。
空きmemory/diskは各2GiBを要求する。solve上限90秒、stage開始時に上限+10秒の余白を要求する。
測定全体は最大20分、root controlsは作成要求時刻から60分後のSTOPと回収15分を固定する。
32CPU測定開始時にSTOPまで35分未満なら開始しない。予算は別途取得使用量に基づく
利用可能資金の確認と予約が必要で、この文書は追加支出を許可しない。

VM17は136test pass/13ignored、44solveのstate/quality一致まで通ったが、固定全matrixは時間不足だった。
小さい原本statusログではexpanded1workerのCFR約25.2–25.8秒、quality約5.16–5.20秒、
state書出し約1.92秒だった。本手順は繰り返す1worker matrixを除き、2本のcanonicalだけにする。
ただしportable2CPU compile/coreの所要時間は未測定であり、旧native時間から成功を保証しない。
2CPU最大20分、resize等最大5分、32CPU最大20分、回収15分という60分の予算割当てで、
前半の遅延があれば測定を開始しない。全pipelineにはfetch、source/hash、native core compilation、
全state比較、gzip、fsync、reader、転送、削除を含める。solverタイマだけで期限を見積もらない。

## 完了後だけ行うscreen

trusted `analyze.py --out PROOF --report NEW_JSON` は全retained membership/hash、source、
immutable build、2boot、全38stage、binary、canonical gzipの展開bytes、quality、順序、期限を先に検証する。
未完了/欠損/不一致なら `not_evaluable`、groups空、performance claimsなし。完了済prefixを選び直さない。

深さ1/深さ2のCFR中央値比が **両inputの32workersで≦0.95**、16workersで≦1.03。
全case/workerのquality中央値比≦1.05、3本のRSS最大値同士の比≦1.10。
両depth・両case・両workerでCFRとqualityをそれぞれ独立に3本max/min≦1.15とする。
全条件を同時に満たしたときだけscreen passed。閾値やsampleを結果後に変更しない。
全3標本/中央値/min/max、CPU秒、CPU/wall、CFR+quality、構築、state書出し、全process、RSSを保存する。
比較基準は同depthの16workersで、1worker強スケールの数値は算出しない。

process CPUにはspin、allocator、schedulerも含まれ、差からSMTだけを原因認定しない。
16core/32logical guestは32物理coreの線形スケール証明ではない。Linux wait4 ru_maxrssは
全process high-water RSSで、工程peakや子process合計memoryではない。
このscreenはproduction採用・外部品質認定・R1全体受入ではなく、採用には別途全workspace
fmt/clippy/testと適用範囲のレビューが必要である。
