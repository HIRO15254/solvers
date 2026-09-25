# VM05: candidate 1（`.sol` v2）の固定比較

この比較では、狭い Flop fixture の全工程時間が中央値で **12.7%短縮**し、記録された
process RSS 高水位が **38.4%低下**した。保存前の EV・BR gain・NashConv と、公開 export の
戦略・値は全9ペアで一致した。一方、`.sol` は River 1.27倍、Turn 6.63倍、Flop **13.55倍**に
増大した。metadata照会とメモリの改善には容量の明確な代償がある。この結果は後続形式の測定で上書きしない。

正本は [pilot](pilot.json)、[固定plan](plan.json)、[18 runの記録](comparison.json)、
[集計とcheck](analysis.json)。2026-09-25 16:21:07–16:21:20 UTC に baseline のみで pilot を行い、
16:22:24 に条件を固定、16:24:08–16:26:01 に比較した。GCP `solvers-r1-20260925-05`、
e2-highmem-8、AMD EPYC 7B12、Ubuntu 24.04、8論理CPU・solver 8 threads、同一boot IDである。
両版とも Rust 1.97.0 の release build。
[build・回帰検証の根拠](../../validation/vm05/README.md)と
[binaryの回収hash](../../validation/vm05/build-download-verification.json)を別に保持する。

baseline は `9632d8b`、candidate 1 は `f6103e7` に対応する snapshot 02。
実測sourceの識別には Git HEAD だけでなく、
[source archive](../../validation/sources/current-02.tar.gz) の SHA-256
`5edec6bea4ce887847c3430b3c45e5c5251402f2780f3289e95409e40b4b8fc6` を使う。
binary SHA-256 は baseline `77418ab6791041efd01378fbfa4d73e3fcd8153a6b7924d748f31727e54cdeb8`、
candidate `6849f0642e47bbd115b1a03dec80cc3b1f3c3a07a99726010d564f97869bfc49`。

全caseは pot 20 chips、残stack 60 chips、rakeなし、DCFR/F32、Full保存、iso mergingなし。
有限木とrangeは[fixture説明](../README.md)の通りで、Flopは各seat 3 combo、Flopのみbet、
Turn/Riverはcheckdown、全runoutを残した縮小問題である。baseline pilot後に変更したのは反復上限だけ。
内部停止目標 `NashConv < 0.04 chips`（開始pot比のExploitability `< 0.1%`）、check cadence、
入力ゲーム、threadsはそのまま、各caseを baseline/candidate の順に交互3回ずつ実行した。

| Case | 反復 | nodes / 保存node | 両版で一致したNashConv（chips） | Exploitability（開始pot比） |
|---|---:|---:|---:|---:|
| River | 100 | 27 / 10 | 0.029750058896130138 | 0.0743751% |
| Turn | 100 | 1,305 / 580 | 0.02882798512776752 | 0.0720700% |
| Flop | 50 | 21,618 / 14,410 | 0.036698924170599945 | 0.0917473% |

18 solve は全て正常終了し、timeout/resource停止はない。raw supervisor の exit 0、
cleanup完了、入出力identity一致も再照合した。各ペアのroot summaryは時間field以外で完全一致し、
`tree`・`strategy`・`ev --node all` のstdout SHA-256も一致した。各版の3反復間でもprofileは一致する。
checkpointは各case/各版の初回、計6件を完了反復上限からforkし、追加0反復でsummaryとexportが一致した。
これは復元・再保存の検査であり、中断checkpointからの追加学習の検査ではない。

以下は各版3回の中央値。括弧内は最小–最大。時間は秒、RSSは MiB（2^20 bytes）。
solve全工程はprocess作成前からroot回収・containment空確認までで、初期化・最終評価・保存・終了を含む。

| Case | 全工程時間 baseline → candidate | 中央値差 | `wait4.ru_maxrss` baseline → candidate | sampled tree RSS baseline → candidate |
|---|---|---:|---:|---:|
| River | 0.1811（0.1791–0.1815）→ 0.1796（0.1790–0.1830） | −0.8% | 24.48 → 24.50 | 10.75 → 10.28 |
| Turn | 1.5344（1.5330–1.7194）→ 1.4854（1.4252–1.4857） | −3.2% | 49.34 → 33.56（−32.0%） | 49.80 → 34.11（−31.5%） |
| Flop | 9.8505（9.8262–10.3834）→ 8.5971（8.4005–8.7013） | −12.7% | 696.93 → 428.97（−38.4%） | 691.88 → 426.58（−38.3%） |

Flopの時間差は3回とも同方向で、測定範囲も重ならない。Turnの中央値差49 msはsolve監視の
約50–60 ms間隔と同程度であり、小幅な速度差の確証は弱い。Riverはばらつきが重なり、速度改善とは認定しない。
3回は小標本で、信頼区間や別hostへの性能保証は与えない。初期化/CFR/BR/保存の独立spanは未計測であり、
この差をkernelだけ、あるいは保存処理だけの改善へ配分しない。

| Case | summary query中央値（ms）baseline → candidate | `.sol`中央値（bytes）baseline → candidate | 容量倍率 |
|---|---:|---:|---:|
| River | 11.412 → 11.228 | 7,473 → 9,468 | 1.27 |
| Turn | 38.558 → 10.951 | 21,335 → 141,512 | 6.63 |
| Flop | 483.530 → 11.069 | 131,744 → 1,784,573 | 13.55 |

summaryは保存直後、profile exportの前に別processで照会した。Turnは71.6%、Flopは97.7%短縮した
**監視下process時間**であり、pure decode時間やcold-cache latencyではない。candidateの約11 msには
process起動・監視記録・終了確認の床が含まれ、sub-ms性能は測れていない。
checkpoint中央値は River 81,323→81,330、Turn 356,281→356,288、Flop 34,538→34,545 bytes。
`.sol` v2 のnode別圧縮・index/checksumは、特にcheckdown node間の冗長性が大きいこのFlopで
v1の一括圧縮より容量効率が悪い。容量を含めた入出力最適化が全て達成されたとは扱わない。

RSSには次の制限がある。`wait4.ru_maxrss` はexec後のsolver imageだけに限定された指標ではなく、
exec前のaddress space高水位を残し得る。
[Linuxのexec実装](https://github.com/torvalds/linux/blob/v6.12/fs/exec.c#L951-L956)にある
旧mm高水位の保存は、Python launcher由来と考えられる約24–25 MiBの床と整合するが、
このVMでlauncher分を分離した校正はしていない。Riverおよび短いsummaryのnative RSSから、
solver本体の小さいメモリ差を認定しない。

candidate summaryの全9件は `sample_count=1 / max_observed_processes=0 / sampled_peak=0` で、
最初の有効なRSS観測前に終了した。**0 bytes消費ではなく観測欠落**である。
Flop summaryのnative値は225.66→25.12 MiBだが、これを純粋なreader RSSの88.9%削減と呼ばない。
Turn/Flop solveはnative高水位とsampled tree RSSの双方が大幅に下がり、launcher床より十分大きい。
なお `/proc/*/statm` のRSS自体も近似値であり、sampled値がnative値を少し上回るrunがある。
[Linux man-pages](https://man7.org/linux/man-pages/man5/proc_pid_statm.5.html)の精度制限に従い、
両metricを同義の厳密peakとして混ぜない。

本照合の戦略は `stored_quantized`、保存EVは `presave_snapshot`。公開exportはown reachが正の行のみであり、
zero-reach行全体の同値をこの測定だけで主張しない。保存された量子化profileへのBR再評価、NoRivers、I16、
一般和rake、通常規模Flop、外部24参照caseは対象外。`quality_status` と `saved_profile_br` は
[集計](analysis.json)でも `not_evaluated` を保持している。

raw bundleはローカル `runs/r1-cloud/vm05-pipeline.tar.gz`（11,950,377 bytes）、SHA-256
`3730bdb23b8cd53d7de70d567bfa897a8f6e3337582622757868c82b86b8cb7e`。
同名の `.manifest.json` が元pathとarchive memberを対応付ける。747 file、非圧縮171,378,186 bytes、
skip 0で、全memberのsize/hashを独立再照合した。compact JSONはこのdirectoryへ保持し、raw bundleは
ignored `runs/` にあるローカル回収物なので、Git cloneだけでraw全量が戻るとは仮定しない。

回収と再検証の入口は [download verification](download-verification.json)、
[全747件の保持対応表](retention.json)、[VMで作成した元manifest](bundle-manifest.json)。
747件全てのsize/hashを再検証し、629件・2,244,291 bytesのcompact証拠をこのdirectoryへ複製した。
残る118件・169,133,895 bytesは `.sol`/checkpointと64 KiB超の展開済profile出力で、
保持対応表の `archive_member` とbundle SHA-256からローカルraw bundleへ辿る。
`git_evidence_path` があるfileはGit側だけで元のraw記録とhashを再照合できる。
hashだけから大きいprofileやcheckpointを再構成できるという意味ではない。

| 証拠 | 入口 |
|---|---|
| 固定入力 | [River](frozen/river.toml)、[Turn](frozen/turn.toml)、[Flop](frozen/flop.toml)、[plan](plan.json) |
| 実行順と全stage起動ログ | [pilot.log](logs/pilot.log)、[paired.log](logs/paired.log) |
| 各runの完走・品質・resume・export check | [comparison.json](comparison.json)、例: [Flop baseline 1](records/paired/flop-1-baseline/result.json)、[candidate 1](records/paired/flop-1-candidate/result.json) |
| OS peak・sample時刻・停止・identity | 例: [solve supervisor](records/paired/flop-1-baseline/solve/supervisor.json)、[samples](records/paired/flop-1-baseline/solve/supervisor.samples.jsonl) |
| 保存前BR・反復推移・イベント | 例: [run.json](records/paired/flop-1-baseline/run/run.json)、[progress](records/paired/flop-1-baseline/run/progress.jsonl)、[events](records/paired/flop-1-baseline/run/events.jsonl) |
| 短いsummaryでRSS未観測の根拠 | [candidate summary supervisor](records/paired/flop-1-candidate/summary/supervisor.json)、[summary出力](records/paired/flop-1-candidate/summary/stdout.log) |
| build/source/binaryの保持先 | [build download verification](../../validation/vm05/build-download-verification.json)、[source snapshot 02](../../validation/sources/current-02.tar.gz) |
