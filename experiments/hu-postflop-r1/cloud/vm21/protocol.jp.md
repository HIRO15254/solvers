# VM21 の資源上限と再利用境界

これは sparse-rank-groups 研究案の資源制御である。旧VMの証拠やnative binaryを候補の実験証拠へ流用しない。旧VM19 deploymentをGCP上へ展開し、rootが別に固定するoverlayを検査・適用した後、新しいVM21上でbaseline/candidateをbuildする。原VM19/20のsource・protocol・証拠は変更しない。測定条件と合否基準は別の候補harnessのprotocolで、起動前に固定する。

## 予算

新しい予約は最大 **$1.35**、共有上限は$40。`reserve.py` は提案と静的計算だけを作成し、台帳を書かない。rootが取得使用量と利用可能額を確認し、`budget.json`・`reservation.json`・`reservation-check.json` を対応させて予約する。$1.30しか利用可能でなければ、この案は実行しない。

| 項目 | 上限計算 |
|---|---|
| 2 vCPU | 47分 × $0.06701142/時 |
| 32 vCPU | 10分 × $0.80/時 |
| 20 GiB disk | 24時間 × $0.000137/GiB時 |
| 外向き転送 | 320 MiB × $0.30/GiB |
| IPv4 | 47分 × $0.0025/時 |
| 未知の価格・税・遅延等の余裕 | $1.00 |

合計は約$1.3472939457。2 vCPU分を全体時間へ重ねて計上した保守的な見積もりで、請求額ではない。単価はrootによる実行前確認が必要。47分は全体45分+120秒、32 vCPUの10分は実際の8分区間+120秒の丸め等の余裕。丸め余裕を実行期限の延長に使わない。圧縮archive上限256 MiB、転送全体上限320 MiBを別々に守り、再ダウンロードや別VM・disk追加を自動実行しない。

## 固定期限と開始回数

資源は `solvers-abstraction-20260723/us-central1-b/solvers-r1-20260928-21`、E2 Spot、20 GiB pd-balanced、開始は最大3回（作成2 CPU、測定32 CPU、回収2 CPU）。`automaticRestart=false`、instance termination actionはSTOP。代替VM種や通常VMへのfallbackはない。

`launch.py` は作成要求時刻+2,700秒を秒へ切り捨てた**原絶対STOP**をGCPへ設定する。build deadlineはSTOP−1,500秒（作成要求から20分以内）。旧bootstrapは元の固定SHAで使用し、overlayによる新harnessの実行は別の明示dispatchとする。

1. 2 CPUで依存取得、両armのportable `x86-64-v3` build、candidateを含むcore testsを完了させる。候補の4追加test名と既存oracle/determinism検査を検査し、失敗なら32 CPUへ進まない。
2. rootがSDK receiptを保持してstop→TERMINATED確認→`e2-highcpu-32`へresizeする。
3. `phase.py arm32` は新しいdescribeで同一instance・TERMINATED・32 CPU・原STOPを照合する。arm時刻+480秒を秒へ切り捨てた**phase STOP**を記録し、GCP `set-scheduling` で設定する。原STOPまでphase終了後12分以上残ることを要求する。
4. `phase.py start32` は新しいdescribeでphase STOPを照合し、残り360秒超の時だけ一度startする。phase STOPをstart後に計算し直さない。boot・SDK・preflight時間も480秒に含まれる。
5. 測定dispatcherはGCPのphase STOPを確認し、測定deadlineをphase STOP−20秒以内かつdispatch+360秒以内に固定する。必要な全scheduleの窓が不足すれば開始しない。systemdのRuntimeMaxSecとrunnerのUTC/monotonic期限も同じ境界内とする。GCP STOPが制御元停止時の上限を受け持つ。
6. 完了・失敗・phase STOPのいずれでも、rootはstop済みを確認し2 CPUへresizeする。`phase.py restore2` は新しいdescribeで**TERMINATEDかつe2-standard-2**の場合だけ、期限を元の原STOPへ戻す。32 CPUのままの延長は拒否する。
7. `phase.py start2` は同一instance・2 CPU・原STOPを確認し、600秒以上残る時だけ1回回収startする。回収reader・圧縮・転送・hash検証後、原STOP内にboot disk付きdeleteを行い、project/prefixのinstances/disks/addresses不在を保持する。diskの24時間以内の削除も照合する。

各phaseは保存先を排他的に作成する。SDK timeout/失敗では先へ進まず、既存資源をdescribeして原因を確認する。自動retry、新たな開始、deadline延長、固定sampleの差替えはない。停止・削除自体は必要な片付けとしてrootが判断する。SDKの`set-scheduling` flagsは実行前にインストール済みSDKのhelpで確認し、describe応答の設定値を次のstart前に検査する。

## 最小測定の実現性

CPU samplingはこの候補の時間比較に不要であり、perf text/rawを追加しない。既存chance-grainの2-depthを2-armに置換し、chance depthは2へ固定する。提案する固定日程はnarrow N2の4 smoke、narrow/expanded N16のbaseline 1-worker canonical各1件、2 cases × 2 arms × 16/32 workers × 4 rounds（round0 warmup）の32件、計38 solves。全state/quality bytesをcase/反復別canonicalへ照合し、gzip canonicalとfsync receiptを保持してから成功重複stateを除く。

VM18の同じ全street/N16日程は測定起動呼出からwrapper完了まで約278.565秒だった。8分からboot/control 120秒を控除した6分枠に収まる可能性はあるが、VM・候補・proof処理の差があるため保証ではない。各stageの固定上限・開始時reserveも6分窓と整合させる。旧90秒+10秒reserveをそのまま残すと終盤の開始条件が不足し得る。canonical60秒、その他20秒等は**起動前のharness reviewで決める案**であり、この文書だけで実装済みとはしない。

4追加testと全state一致は局所正当性の証拠になる。1秒未満のCFR sample、3標本のばらつき、16 guest core/32 logicalの区別を報告し、固定N16だけで同exploitability目標までの時間や32物理coreの比例スケールを認定しない。未完了ならperformance未評価を保持する。採用には通常の全workspace検証と対象範囲レビューが別途必要である。

## 起動を開くための小さな記録

`launch-approval.json` は `schema=r1.vm21-launch-approval/v1`、`resource`、`package_review_passed=true`、`files`を持つ。filesは旧`../vm19/deployment01.tar.gz`・`source-manifest.json`・`bootstrap.sh`、新`overlay-manifest.json`・`install-overlay.py`、4 control scriptsのbytes/SHAを固定する。この記録とrootの予算適用が無い限りlaunchは失敗する。

`reservation-check.json` は `budget_after`（共有台帳pin）、`reservation`（reservation.json pin）、`estimate_usd`（reserve.pyの正確なdecimal文字列）、`at_utc`を持つ。起動前の15分以内に対象prefixの空在庫を取得し、30分以内の予約確認と照合する。新しいfreeze後にはこれらを再作成し、古いreviewのpinを流用しない。
