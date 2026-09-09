# Multiway convergence GCP Spot計画（2026-09-09）

これは当初の送付・起動計画と実行記録である。ユーザーの「転送OK」により、
固定済みの3ファイルの送付と予算内の実験は承認された。当初の対象候補は
`solvers-abstraction-20260723` / `us-central1-a` / `mw-convergence-c4-20260909-a`。
最終payloadは固定済みで、起動時のbilling linkと既存resourceは再確認した。
以下の「案」「未実行」は当初計画時点の記述であり、最新状態は末尾の実行記録を参照する。

## 機種とディスク

候補は `c4-highmem-32`（32 vCPU、248 GiB）である。C4の公式仕様表にこの機種の
vCPU数・メモリが記載されている。C4 VMはPersistent Diskを使えず、NVMe interfaceの
Hyperdisk Balanced、Hyperdisk Balanced HA、Hyperdisk Throughput、Hyperdisk Extreme
だけを使えるため、旧計画の`pd-standard`/30 GBを流用しない。build、EHS cache、run
directoryを同じboot diskへ置くため、50 GiB `hyperdisk-balanced` を候補とする。
Local SSD (`-lssd`) は使わないので、STOP時のLocal SSD破棄オプションも指定しない。

公式資料:

- [C4 machine types and supported disks](https://cloud.google.com/compute/docs/general-purpose-machines)
- [Hyperdisk Balanced](https://cloud.google.com/compute/docs/disks/hd-types/hyperdisk-balanced)
- [Spot VMの作成](https://cloud.google.com/compute/docs/instances/create-use-spot)
- [VM実行時間の上限](https://cloud.google.com/compute/docs/instances/limit-vm-runtime)
- [Spot VM価格](https://cloud.google.com/spot-vms/pricing)

## 費用ゲート

親が実行直前に確認した候補単価は `us-central1` の `c4-highmem-32` Spotで
**$1.18876 / VM-hour** である。これは固定価格として扱わず、起動直前に公式価格表を
再確認してmanifestへ時刻と単価を記録する。初回は単一VMを最大4時間とし、Spot
preemption・価格変動・Hyperdisk・ephemeral IP・egress・短時間課金のために保守的な
予約を置く。

| 枠 | 計算 | 上限/予約 |
|---|---:|---:|
| 初回Spot稼働 | 4 h × $1.18876 | $4.75504 |
| 旧VMの請求予約 | 既存履歴の保守枠 | $1.00 |
| disk/IP/egress等の雑費予約 | conservative reserve | $3.00 |
| 追加を含む稼働上限 | 12 h × $1.18876 | $14.26512 |
| 計画合計 | $14.26512 + $1 + $3 | **$18.26512** |
| 予算残 | $20 − $18.26512 | **$1.73488** |

12 VM-hoursはaggregateの運用上限で、4時間の初回枠を使い切ることを意味しない。
preemption後の再起動は自動化せず、実測稼働時間を記録し、12時間または価格再計算後
の予算上限に近づいたら停止する。Cloud Billingの表示は遅延し得るため、実行直後に
請求額をUSD 0とは断定しない。billing明細が確定するまで、費用欄は未確定とする。

## 作成時の設定案（未実行）

`gcloud compute instances create` と `set-scheduling` のhelp（portable SDK 583.0.0）で
次の公開flagsを確認した。以下は実行可能な形の案だが、ここには実行承認を与えない。

```sh
gcloud compute instances create mw-convergence-c4-20260909-a \
  --project=solvers-abstraction-20260723 --zone=us-central1-a \
  --machine-type=c4-highmem-32 \
  --image-family=ubuntu-2404-lts --image-project=ubuntu-os-cloud \
  --boot-disk-type=hyperdisk-balanced --boot-disk-size=50GB \
  --no-boot-disk-auto-delete \
  --provisioning-model=SPOT --instance-termination-action=STOP \
  --max-run-duration=4h --no-service-account
```

`STOP`はpreemptionまたは4時間の自動終了時にVMを停止し、boot diskを残す。これに
よりcheckpoint、raw log、EHS cacheを同じVMへ保持できる。再起動時にはmax-run-duration
が新しいRUNNING開始時点から再計算されるため、これは予算制限の代わりにならない。
親側のlauncherが通算時間を記録し、再起動前に上限を再確認する。最終回収後は
`gcloud compute instances delete ... --delete-disks=boot`でVMとdiskを削除し、
static IPや追加diskは作らない。作成前後にinstances/disks/addressesをread-onlyで
一覧し、名前・zone・状態を照合する。

起動直前に必ず次を確認する（この文書作成時には実行していない）。

```sh
gcloud compute machine-types describe c4-highmem-32 \
  --zone=us-central1-a --project=solvers-abstraction-20260723 \
  --format='yaml(name,guestCpus,memoryMb,zone)'
gcloud compute regions describe us-central1 --project=solvers-abstraction-20260723
gcloud compute instances list --project=solvers-abstraction-20260723
gcloud compute disks list --project=solvers-abstraction-20260723
gcloud compute addresses list --project=solvers-abstraction-20260723
```

この端末ではgcloud helpとSDK versionは確認できたが、Windowsの既定gcloud configが
`%APPDATA%`のlog/credentials DBへ書き込めず、machine-type describeのAPI照会までは
完了していない。従ってC4のzone availability、quota、起動成功をこの計画から推測しない。
親側の認証済みread-only照会では、`c4-highmem-32` が `us-central1-a` に存在し
32 vCPU / 253,952 MiB（248 GiB）であること、`us-central1` の一般CPU quotaが
未使用であること、既存VMが0件であることを確認した。C4/Hyperdisk固有quotaは
取得できておらず、Spotの確保成功も未確認なので、これらを利用可能枠とは扱わない。

## 送付manifestとpayload

最終変更を確定した作業ツリーからsource archiveを作る。archiveには
workspaceの`Cargo.toml`/`Cargo.lock`、`crates/`、必要な`examples/`、`tools/`、
`docs/`を含め、`.git/`、`target/`、`.cache/`、`runs/`、秘密情報、認証ファイル、
過去runの巨大artifactを含めない。Rust buildに必要な全workspace sourceを残し、既存
binaryやEHS cacheは送らずVM上で生成する。送付前にtarのrootが`Cargo.toml`であること、
archive file listに鍵/token/credential名がないこと、sha256がmanifestと一致することを
検査する。

送付する候補は次の3ファイルだけである。末尾の準備状況に最終payloadを示す。

1. `source.tgz` — 最終workspace source archive。
2. `source.manifest.json` — archive SHA-256、git revision、作成UTC、含めたroot、除外
   パターン、対象project/zone/VM名、想定binary pathを記録。tokenやsecretは記録しない。
3. `gcp_convergence_bootstrap.sh` — Ubuntu 24.04でRust 1.97.0を導入して
   `cargo build --locked --release -p cli --bin solvers --example mw_checkpoint_audit`を
   行い、solverとcheckpoint audit binaryのhashを保存する既存script。実験は開始しない。

archiveは`tools/gcp_source_package.py`がallowlistを検査して決定的に生成する。
git-trackedのworkspace sourceに加え、最終変更を含められる明示的な`crates/`、
`examples/bench_multiway/`、GCP/benchmark toolだけを対象にし、各relative pathとSHA-256
をmanifestへ書く。`--dry-run`はinventory/hashをstdoutへ出すだけで、archive・manifestを
作成しない。symlink、workspace外へ解決するpath、許可suffix外のfixtureは拒否する。
親が最終変更を確定してからdry-runを確認し、承認後に一度だけ実archiveを作る。

送付承認後の転送案は一時的なhome directoryを使い、root専用`/opt`へsudoで移す。
これはsource destinationを明示するための案であり、まだ実行していない。

```sh
gcloud compute scp source.tgz source.manifest.json gcp_convergence_bootstrap.sh \
  mw-convergence-c4-20260909-a:~/mw-convergence-payload/ \
  --project=solvers-abstraction-20260723 --zone=us-central1-a
gcloud compute ssh mw-convergence-c4-20260909-a \
  --project=solvers-abstraction-20260723 --zone=us-central1-a --command \
  'sudo install -d -m 0750 /opt/solvers-experiment/results && \
   sudo install -m 0640 ~/mw-convergence-payload/source.tgz /opt/solvers-experiment/source.tgz && \
   sudo install -m 0640 ~/mw-convergence-payload/source.manifest.json /opt/solvers-experiment/source.manifest.json && \
   sudo install -m 0750 ~/mw-convergence-payload/gcp_convergence_bootstrap.sh /root/gcp_convergence_bootstrap.sh'
```

SSH転送方式（ephemeral public IPまたはIAP）は起動前に親が選択し、選択理由と実際の
egressをmanifestへ記録する。static IP、GCS bucket、追加service accountは作らない。
`--no-service-account`でも、SSHのユーザー認証は必要なので、OS Loginまたは既存の
承認済みSSH経路を親が確認する。認証token、秘密鍵、ローカルcredential fileをpayloadへ
コピーしない。

## 実行・回収・後片付け

1. 起動前に価格、billing link、quota、zone、既存resource、archive hashを再確認する。
2. VM作成後、instance metadataとboot disk設定が案どおりかread-onlyで確認する。
3. `source.tgz`とmanifestを転送し、転送後のremote SHA-256をlocal値と比較する。
4. bootstrapをrootで一度だけ実行する。`results/bootstrap-*`、environment、binary
   SHA-256を確認してから、親がsolver実験を明示的に開始する。
5. runごとにeffective config、git revision、binary/evaluator hash、cache build/load、
   wall/build/training/evaluation時間、checkpoint、raw stdout/stderrを保存する。
6. preemption時はVMを自動再作成・自動再開せず、停止VMのdiskから結果とcheckpointを
   回収してから、残りaggregate時間と予算を再判定する。
7. 初回4時間、aggregate 12時間、または費用見積り$20のいずれか早い境界で停止する。
8. result archiveとmanifestを回収し、checksumを再検査した後、VM・boot disk・IPを
   deleteする。請求は遅延を踏まえて後日照合する。

この候補は高メモリpreflightと数時間以内のbounded run用であり、C4の確保可能性や
Spot価格を保証しない。`.mwsol`は全streetの観測済み平均戦略を保存するが、生のregret
と平均質量0のfallbackを再現する監査には`.mwckpt`を使う。held-out評価が有限の候補を
調べる範囲であることを実験結果に明記する。

## 送付直前の準備状況

[送付対象と送付先の明細](multiway-cloud-payload-2026-09-09.json)に最終payloadを固定した。
`runs/gcp-convergence-20260909-control/final/source.tgz`は163file、751,361 bytes、
SHA-256 `1c8e6174b4e84efbffb4a13ae613b82220ce0ee768dfdf10d84fb634ea12944b`。
全memberをmanifestへ照合し、別directoryに展開したsourceを新しいtarget directoryで
offline/locked `cargo check`に通した。転送・VM作成はまだ行っていない。
2026-09-09 00:58:17 UTC時点のread-only一覧はVM・disk・reserved addressすべて0。

以前の自動承認レビューは、非公開sourceについて具体的なpayloadとdestinationへの
明示承認が不足しているとして転送を拒否した。その後ユーザーが「転送OK」と
具体的なpayloadの送付を承認したため、通常の認証済みGCP経路で実行を進める。

## 実行記録（承認後）

- C4-highmem-32の作成は地域C4 quota 24 vCPUで失敗。VM/diskは残らなかった。
- C2D-highmem-32（32 vCPU、256 GiB）をus-central1-a/cで試したが、両zoneとも
  Spot在庫不足。各失敗後にVM/diskが0件であることを確認した。
- 次の候補はN2-highmem-32、最低CPU platform Intel Ice Lake、32 vCPU / 256 GiB、
  us-central1-a、`mw-convergence-n2-20260909-a`。50 GB pd-balanced、service accountなし、
  最大4時間後STOPとし、同じ承認済みsourceを送る。
- 公式Spot表（2026-09-09 UTC取得）でN2候補は$1.257472/h。初回4時間は$5.029888、
  通算12時間は$15.089664。旧利用$1・雑費$3の予約込み$19.089664で、総予算$20以内。
  当初C4の$18.26512見積りはN2実行時にはこの値へ置き換える。実請求は未確定。
- 32 vCPUという仕様だけで高速化を断定せず、同条件のwarm-cache実測を採用判断に使う。
- N2は起動に成功し、Ice Lake/32CPU/約251GiBのOS可視RAMをSSHで確認した。
  最初のN2宛て転送は自動承認レビューが宛先変更への明示承認不足として拒否したため、
  未転送のまま停止した（開始01:35:06 UTC、停止01:38:12 UTC、約187秒）。
  この分のcompute概算は約$0.066であり、通算予算に含める。
- ユーザーが続けて「VMどこでも転送していいよ」と明示承認したため、
  以後の実験用VMの機種・zone変更を含む転送境界は解決した。N2を再起動して
  同一payloadで実験を進める。起動ごとの上限4時間に加えて通算時間も記録する。

## state 4修正版の実行先

N2は01:46:51 UTCと02:03:03 UTCに`compute.instances.preempted`が記録された。
1回目のpilotは学習前に回収され、2回目はbucket-cache不具合が再現したため
01:56:24 UTCに親が停止した。以後、旧solverで戦略学習は行っていない。
その後のpostflop cap2の資源count中に2回目のSpot回収を受けた。完了結果は未回収。
N2の3回の稼働区間は合計約24.5分、compute概算約$0.51で、実請求は未確定。
N2 instance/diskは停止状態で保持し、回収または不要の確認後に削除する。

次のC4はAPIで存在と24vCPU/190,464MiBを確認し、地域quota24に収まるshapeを使った。

| 項目 | 実行値 |
|---|---|
| VM | `mw-convergence-c4-24-20260909-a` |
| project / zone | 同じproject / `us-central1-a` |
| machine / platform | `c4-highmem-24` / Intel Emerald Rapids |
| vCPU / memory | 24 / 186GiB（guest可視約182GiB） |
| boot disk | 50GB Hyperdisk Balanced / NVMe / auto-delete false |
| RUNNING開始 | 2026-09-09 02:06:01 UTC |
| 自動STOP予定 | 2026-09-09 06:05:54 UTC |
| Spot公式観測単価 | $0.89157/h |
| 4時間compute概算 | $3.56628 |

`state4/source.tgz`は167file、763,039bytes、SHA256
`a70f11a96e510da933e174fdf1dc9a98148807c5bfb140768741731c46a8d3b1`。
全tar memberのhash照合、workspace fmt/clippy/test（717 passed、30 ignored）、
Python24test、別agentによるcache contextとcheckpoint migration reviewを通した。
3ファイルとpilot runnerのremote SHA256を`sha256sum --check --strict`で照合済み。
通常featureだけをbuildし、未完のresearch sampling featureは使用しない。

このsourceはstreetだけでなくstreet開始時の相手人数でcombo-bucket cacheを区別する。
state3以前のcheckpointは明示拒否される。新しい学習はfreshで開始し、旧結果は
過去の診断として保持する。C4pilotは8/24threadsのK32比較とK256を順次実施し、
arena budget160GiB、checkpoint間隔1分、service上限110分、VM上限4時間とする。
通算費用はmachineごとの実測稼働時間で加算し、$20と既存保守枠を維持する。

## state 4 実行・回収・後片付け追記（2026-09-09）

- 現行state4は `mw-convergence-c4-24-20260909-a`（`us-central1-a`、
  `c4-highmem-24`、Intel Emerald Rapids、24 vCPU / 186 GiB）で実行した。
  RUNNING開始は02:06:01 UTC、自動STOPは06:05:54 UTC、Spot観測単価は
  `$0.89157/h`、4時間compute概算は`$3.56628`である。
- state4の`source.tgz`は763,039 bytes / 167 files、SHA-256
  `a70f11a96e510da933e174fdf1dc9a98148807c5bfb140768741731c46a8d3b1`、
  manifest SHA-256は
  `2a95ba3021117dc2e97f531965ca251de5dfdde271cd4bc618269c5e39be42d2`。
  bootstrapは既存ハッシュと一致し、remote payload全件も一致した。
  bootstrap完了は02:12:43 UTC、Rustは1.97.0、solver SHA-256は
  `841b66d95d5c8c56cdea9fd7b95a5a716f14d93896a903014dffecf958d3433a`、
  checkpoint audit SHA-256は
  `6eb48acf40f987e577301a629618c88e439e2592e85326b76c80627bd359391f`。
- 通常featureだけを使い、fmt/clippyは通過、workspace testは717 passed / 30
  ignored、Python testは24 passedである。
- 旧state3のcheckpointはstate4のactive-opponent-count cache key修正後には無効であり、
  state3 checkpoint invalid for state4として歴史的診断に限定する。
- N2の回収結果は
  `runs/gcp-convergence-20260909-control/n2-results-20260909.tgz`、SHA-256
  `28e30e26a432d44c5ba84769547d638368fda01543fd2bac061f3fab83250b2c`。
  N2 VMとboot diskは削除し、02:35--02:36 UTCのAPI一覧で残存するのがC4とそのboot
  diskだけであることを確認した。N2約24.5分のcompute概算は約`$0.51`、短い
  n2-standard-4 recoveryの実請求は未確定である。
- $20総枠は維持し、既存の保守予約を含む保守的な残額`$4`を確保する。追加の
  VM起動・転送・外部呼び出しはこの追記では行っていない。

## 稼働率を踏まえた停止・縮小（02:49 UTC）

C4の全baselineとcap2 census完了後、親がVMを停止し、APIの
TERMINATEDとlastStopTimestamp=02:49:22.281 UTCを確認した。
第1区間43分20.892秒のcompute概算は$0.64413。ディスク・通信と実請求は別で未確定。
K32では8/24threadsの戦略・評価が一致し、最終progress時間61.247/55.090秒に
対してCPU単価は3倍になるため、以後の小規模比較はlocalへ移す。
Localは16logicalCPU、31.92GiB RAM、確認時13.76GiB available。
大きなモデル用はc4-highmem-8（8vCPU/62GiB、公式観測$0.29719/h）を候補とし、
実行キューの準備後に再開する。停止中も50GBbootdiskは保持され課金対象。
公式価格: https://cloud.google.com/spot-vms/pricing?hl=en

## 8vCPU再開とlocal分担（02:56 UTC）

同じVM/diskをc4-highmem-8へ縮小し、02:56:45.608 UTCにRUNNING。
APIとguest nproc=8、62GiB（visible約60.8GiB）を確認し、同じ
production binary hashで準備済みqueueを開始した。公式観測単価$0.29719/h、
最大3時間compute概算$0.89157。guest側もqueue完了・通常error後5分でshutdownする。
15秒ごとのCPU busy fraction、available RAM、実験process RSSを記録する。
Cloudは混合K128/64/32のcap1対cap2とK256の262,144 sweepへの延長、
localはK32batch8/12とその後のaverage-sampling比較を担当する。
現在の縮小VM名には過去shape由来の`24`が残るが、実shapeは8vCPUである。

## Round5実測・出力エラー回収（04:05 UTC停止）

8vCPUでのround5区間は02:56:45.608--03:53:32.184 UTC、compute概算$0.28122。
K256の262,144 sweep checkpointは保存されたが、12,297,431 strategyを持つ解の
出力が旧1,000万件上限で失敗した。再学習は行わず、03:55:28.271--04:05:53.072 UTCの
短い再起動でcheckpointの独立auditと回収を完了した。この区間は概算$0.05158。
親がAPIでTERMINATEDを確認し、修正準備中はVMを停止した。50GB boot diskは
実データでの再出力検証に備えて保持している。

確認済みC4の3区間のcompute小計は$0.97693。先行N2約$0.51、短いN2回収、
それ以前の未明細利用、disk/network等と最終請求は別であり、これを総請求額とはしない。
元の$20枠と既存利用・付随費用の保守枠$4は維持する。
区間計算は `runs/multiway-convergence-round5-20260909/cloud-k256-extension/budget-observed.json`。

CPUの15秒間隔ログ199件について、03:00:15--03:49:47 UTCの観測区間平均は37.99%。
K256追加学習のsolve全工程は56.48%、cap2のsolveは27.67%、cap2 auditは16.16%。
初期化・評価・checkpoint・出力を含む数値であり、純学習の速度ではない。観測開始前の
約115秒は含まない。学習時の約75%という瞬間値を全工程の稼働率に置き換えない。
詳しい区間集計は同directoryの `utilization-summary.json`。

旧queueの `shutdown -h +5` は、停止前5分にログインを禁止するため、予定した回収猶予中の
SSHを妨げたと診断した（[upstream shutdown仕様](https://github.com/systemd/systemd/blob/main/man/shutdown.xml)）。
以後のcontrollerは5分後にshutdownを実行するsystemd timer方式へ変更し、現在の凍結済み
クラウド実験のscript/hashは変更していない。既存キューの失敗と回収記録も保持する。

## 最新停止記録とstate 4 export-fix（2026-09-09 04:51 UTC）

旧queueやstate4の計画値は上記のhistorical記録として保持する。最新の親側API観測は
`runs/gcp-convergence-20260909-control/export-fix-state4/cloud-stopped-after-review.json`
に保存した。`c4-highmem-8` は 04:48:53.194--04:50:48.225 UTC の115.031秒だけ稼働し、
04:51:27.4791442 UTCの観測で `TERMINATED`、boot disk保持を確認した。観測Spot単価
`$0.29719/h` によるcompute概算は `115.031 / 3600 × 0.29719 = $0.00949613`
（約$0.0095）である。これはcompute見積りであり、遅延する請求明細の確定額ではない。

export-fix用の凍結sourceは171 files、archive SHA-256
`cbd1df5267ab12c1518f30894b7abae0005bf6556f0dcea8ab5135ebbf9e9241`、manifest SHA-256
`0f8a1e59e6d9afe2c5558e16dce77c3372d6e0ddf31a99a6adef05a878a7f22e`、controller SHA-256
`19790e169a66940ad05247d223a468860905c16c46e33de52497f5de21f27766`である。workspace検証は
727 passed / 30 ignoredで、controllerはcheckpoint SHA、state 4、262,144 sweeps、
12,297,431 strategy blocks、camelCase export schemaを実行前条件として確認する。

自動承認レビューがprivate sourceの送付先・payloadの具体承認不足で転送を拒否したため、
この時点ではpayload転送、source build、checkpoint exportを実施していない。最新の
payload JSONでは過去のRUNNING・auto-stop値を`state4_historical_round5_plan`へ移し、
`state4_current`を上記TERMINATED観測へ同期した。承認待ちを維持し、追加VM起動やcloud
操作は行わない。


## 最新Goal後の保存・回収（完了）

ユーザーはオープンソース公開予定のSolverについて任意の転送を改めて明示許可した。
許可された同じ171ファイルのarchiveとmanifest、配置パスを修正したcontrollerを転送し、
既存state4 / 262,144 sweeps checkpointから12,297,431 blocksの `.mwsol` を作成した。
元checkpointのSHA-256は変わっていない。2,171,445,586 bytesの成果物をローカルへ回収し、
SHA-256照合に成功した。転送許可待ちは解消済み。

初回のcontroller配置エラーでは計算前に停止し、保存先のアクセス権による回収失敗後も停止した。
回収専用の再起動では圧縮コピーへ切り替え、重複する非圧縮転送を中止した。
圧縮archiveは997,005,699 bytes、途中中止した非圧縮転送は374,308,864 bytesだった。
最終停止は `2026-09-09T09:07:38.137000+00:00`、API状態は `TERMINATED`。boot diskは保持している。

今回3区間のcompute概算は **$0.1562**。
[現在観測したSpot価格](https://cloud.google.com/spot-vms/pricing?hl=en) `$0.29719/h` と実行記録を使用。
これまでに識別したcompute小計は約 **$1.65** であり、
過去の未集計区間、disk、public IP、通信料、請求の遅延を含む最終請求額ではない。
総予算$20と未確定費用用$4の余裕は維持する。
[区間・費用記録](../../runs/multiway-convergence-round5-20260909/cloud-export-fix-state4/budget-observed.json)。
