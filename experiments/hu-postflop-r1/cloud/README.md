# R1 Spot実験の費用と回収

利用者の累計20 USD許可から、初回は5 USDを予約する。[台帳](budget.json)は請求額と予約額を区別する。
単一e2-highmem-8（8 vCPU / 64 GiB）、100 GiB pd-balanced、最大3時間。
実験環境の構築と通常検証、その後の同一CPUでの基準／変更版測定に使う。

2026-09-25に確認した一次料金は、[e2-highmem-8通常料金](https://cloud.google.com/products/compute/pricing/general-purpose)
0.36159864 USD/h、[balanced disk](https://cloud.google.com/compute/disks-image-pricing)約0.000136986 USD/GiB/h、
[Spot IPv4](https://cloud.google.com/vpc/pricing-announce-external-ips)0.0025 USD/h。
Spotの変動額そのものは未取得なので、CPU/RAMは通常料金を切り上げた0.37 USD/hで予約する。
ダウンロードは最大2 GiB、転送予備0.30 USD/GiB、削除遅延・税・料金差等の予備を含め5 USD。
請求未反映の予約を解放して次のVMを増設しない。

[launch.ps1](launch.ps1)は起動前に予約総額を検査し、一意の試行記録を書いてから作成する。
[絶対終了時刻](https://docs.cloud.google.com/compute/docs/instances/limit-vm-runtime)にDELETE、
boot diskはauto-delete、Spot回収時もDELETE。停止からの自動再起動はしない。
固定IP、snapshot、外部bucket、サービスアカウントを作らない。
接続は既存gcloud認証を使い、[bootstrap](bootstrap.sh)はRust/toolchainと標準build依存を導入する。

削除前に小さいログ・manifest・結果を回収しSHA-256を照合する。
大型binaryや成果物は回収前にsizeを確認し、転送枠に含める。
終了時に記録したinstance ID/nameとboot diskの消失を確認する。自分のlabel/name以外は削除しない。
Spot回収で消失した未回収runは成功とせず、中断として残す。
Python supervisorは各コマンドのtimeout/resource制御に使い、Linuxではsystemd scopeのcgroupで子孫も囲む。

この台帳はR1実験費用の証拠であり、作業状態の管理先はLinear。

## 転送許可

最初のsource転送は自動承認レビューが非公開repositoryの転送許可不足として拒否した。
利用者は続けて「ファイルの転送を全面的に許可します。理由はこれらのアプリケーションは
最終的にオープンソースとなるためです」と明示した。この許可に基づき同projectの実験VMへ転送する。
基準build sourceは9632d8bのCargo設定とcrates、871630 byte、
SHA-256 `fdd8c1c014a94c70b56efbd79a36c18f6f1634c20aaf9062660ee3a6133a0197`。
VM側でも同hashを照合した。基準sourceの完全内容は当該Git commitから復元可能。

## 段階ごとの証拠回収

[collect-evidence.py](collect-evidence.py) は明示した file/directory だけを読み、
`tar.gz`、同内容の retention manifest、SHA-256 sidecar を出力する。Python 3.11+ の標準ライブラリだけを使う。
存在しないroot・重複root・既存output・symlinkのrootはerrorにする。remote操作やVM削除は行わない。
archiveとsidecarの**合計**を `--max-bytes` 以下（既定2 GiB、これを超える指定は拒否）に制限する。
繰り返し回収する場合、各回の上限を累計ダウンロード枠の残額へ収める。既定2 GiBを各回の追加枠と扱わない。

Spot回収で未転送データが失われるため、validation/build/pilot/pairedの完了ごとに小さいbundleを回収する。
選んだrootのwriterは停止済みであること。進行中の次stageのrootは選ばない。
入力のhash中・packing中に変更を検出した場合はerrorとし、不完全なbundleを公開しない。
以下のpathは実際に存在する完了stageだけを選ぶ例であり、欠測stageのrootをそのまま渡さない。

```sh
python3 /opt/r1/current/experiments/hu-postflop-r1/cloud/collect-evidence.py \
  --out /opt/r1/evidence-validation-03.tar.gz \
  --max-bytes 536870912 \
  --root validation=/opt/r1/validation-03 \
  --root baseline-build=/opt/r1/baseline-build-pinned.log \
  --root baseline-source=/opt/r1/baseline-source.tar.gz \
  --git-source baseline-source=9632d8b \
  --root baseline-binary=/opt/r1/target/baseline/release/solvers

python3 /opt/r1/current/experiments/hu-postflop-r1/cloud/collect-evidence.py \
  --out /opt/r1/evidence-pilot-03.tar.gz \
  --max-bytes 536870912 \
  --root pilot=/opt/r1/current/runs/r1-pilot
```

比較完了後は `--root paired=/opt/r1/current/runs/r1-paired`、必要なら固定planのrootを追加する。
変更版source archive、binary、build logも各々存在するpathを明示する。
directory内の `target/.cache/.git/cargo/rustup` は走査・収容しない。
binaryは例のように **fileそのもの** をrootにすればtarget配下でも収容できる。

8 MiB以下のrecord/config/logを優先し、その後は残るfileを小さい順に収容する。
実runの `.sol`、checkpoint、export stdout/stderr、resource JSONLも同じ枠内で選ぶ。
全対象regular fileについて元の絶対path、相対path、bytes、SHA-256、収容有無をmanifestに残す。
容量で省いたfileは `removed-with-VM`、`--git-source LABEL=REF` を明示したsource rootだけは
`source-contents-reproducible-from-git` とする。これは**内容**の復元可能性であり、archiveの元bytesが
戻せるという主張ではない。binary・run成果物へGit復元指定を流用しない。
symlinkは辿らず、link先文字列のhashと欠測理由を記録する。除外cacheは内容をinventoryしない。

archive内は短い `files/00000000` の名前を使い、manifestの `archive_member` で元pathへ対応付ける。
収容fileの可用性は `in-evidence-bundle-pending-download-verification` とする。
collector終了時のJSONにarchive/manifest hashと合計bytesを出す。3出力をローカルへ回収してhashを
照合するまで、VM上のbundleだけを長期保持済み証拠と扱わない。VM削除後もmanifestと照合記録を残す。

`pack-source.py`は選定ファイルと内容hashのmanifest、時刻を固定したsource archiveを作る。
既存Cargo targetを再利用する場合、抽出は必ず `tar --touch -xzf ...` とする。
固定した過去mtimeで上書きするとCargoが既存binaryを再利用する場合があるため、
sourceの内容hash一致だけで新しいコードの検証成功とはしない。

## VM05 の Spot 回収と証拠の可用性

`solvers-r1-20260925-05`（instance ID `1627891813360280286`）は
2026-09-25 17:05:26 UTC に Spot 回収が始まり、auto-delete の disk とともに消失した。
GCP system event と、その後の同 project の instance/disk 一覧が空だったことは親タスクの
tool 出力による報告であり、この記録の担当者は GCP へ再照会していない。
event ID・時刻・ローカル棚卸しは [preempted-05.json](preempted-05.json) に残す。

| 証拠 | 回収後の状態 |
|---|---|
| baseline/source02 の build・binary・通常検証 | `runs/r1-cloud/vm05-build.tar.gz` がローカルに残り、13 payload の size/hash を再照合済み。[回収記録](../validation/vm05/build-download-verification.json) |
| candidate 1（`.sol` v2）の pilot・固定比較 | `runs/r1-cloud/vm05-pipeline.tar.gz` の全747 payloadを再照合済み。[選定証拠](../pipeline/evidence-vm05/README.md)629ファイルも再照合済み |
| source03/v3 の通常検証、source02 の ignored 5件 | `runs/r1-cloud/vm05-v3-checks.tar.gz` の全15 payloadを再照合済み。[v3 通常検証](../validation/vm05-v3/README.md)7ファイルと[source02 ignored](../validation/vm05-ignored-source2/README.md)8ファイルも保持 |
| v3 の性能比較と HU-R0-019 の診断 run | **未回収のまま消失**。tool 上で見た数値は暫定観測で、Git-backed な実測証拠でも受入根拠でもない |

失われた `v3-results` bundle は VM 上での作成完了まで確認され、13,341,355 bytes、
SHA-256 `4ad690c136da5571e2b2e3f22743b71d428a990865c5a5fe1c942b0ba69ecf45` と報告された。
しかしローカル download/hash 検証は完了しなかった。collector の完了や hash の文字列だけで、
元の性能ログ・成果物・診断結果を保持したとは扱わない。
**回収済みの `v3-checks` と、消失した `v3-results` は別 bundle** である。
candidate 1 の回収済み比較も、後続 v3 の性能結果を代用しない。

source snapshot 01–04 と HU-R0-019/007 の参照入力はローカルに残るが、
元 run の時間・メモリ測定や diagnostic の成功を復元する証拠ではない。
source04 の保存だけで、その build・test・保存 profile audit が完了したとも扱わない。
再実行する場合は新しい host/source/input を識別した別の測定として記録する。

上表の raw bundle は ignored `runs/` にあり、Git clone だけでは戻らない。
小さい結果・manifest・log は repository の証拠用 directory に選定されている。
今回の棚卸しは bundle 内775 payloadと選定済み644ファイルの byte/hash を再検査したもので、
保留中の repository 変更が commit 済みであることを表すものではない。

## VM06 の停止・再起動と CPU の変更

VM06 は初期 VM の DELETE 方針を変更し、Spot 回収時の action を **STOP** にした。
[起動引数](launch-r1-20260925-06.json)と[GCP作成結果](create-result-r1-20260925-06.json)に
`instanceTerminationAction=STOP`、`automaticRestart=false`、絶対終了時刻
`2026-09-25T20:09:11Z` を記録している。STOP では disk を保持するため、VM と disk の明示削除が必要になる。
この変更は計算時間の延長や追加予算を認めるものではない。

`solvers-r1-20260925-06`（instance ID `5209515640504390740`、`us-central1-b`）は
2026-09-25 17:32:06.317259 UTC に Spot 回収が始まり、17:32:27.860 UTC に停止した。
同じ instance / disk を再起動し、GCP の `lastStartTimestamp` は 17:33:59.074 UTC、
報告時の状態は RUNNING だった。[preempted-06.json](preempted-06.json)に operation ID と provenance を残す。
これらの remote event / describe 値は親タスクの GCP tool 出力による報告であり、記録担当者は再照会していない。
元の絶対終了時刻と VM06 の 5 USD 予約は維持し、この時点の未解放予約合計は 15 USD。
これは実請求額の報告ではない。費用の正本は引き続き [budget.json](budget.json)。

停止前の CPU は Intel Xeon 2.20 GHz（family 6 / model 79）、再起動後は
AMD EPYC 7B12（family 23 / model 49）へ変わった。
[停止前の toolchain / CPU 出力](../validation/vm06-source06/checks/00-toolchain/stdout.log)と
[再起動後の出力](../saved-profile/evidence-vm06-build/records/recovery/recovery/00-toolchain/stdout.log)を保持済み。
[比較 build の preflight](build-comparison.sh) が旧 native cache の利用を
compile 前に拒否したことは親タスクの実行報告に基づく。
同じ instance ID と disk でも CPU は同一とは限らず、停止前後をまたぐ native binary / cache の再利用や
性能比較を、同一 CPU での比較として扱わない。

停止前に回収した [source06 通常検証](../validation/vm06-source06/README.md)は、全55 payloadの
size / hash をローカルで検証済み。fmt / clippy / workspace / Python 検査の成功記録は残っている。
旧 `audit-pair-build/result.json` については、回収した元データで14 stageの完了を主張する記録と、
停止前の終了時刻 `2026-09-25T17:31:29.267854Z` が見つかった。一方、`complete.json` はなく、
stage 12 の stderr は737 bytes中63 NUL、samples JSONLは114,688 bytes中1,178 NULを含んでいた。
以前の stage 12 実行中の snapshot だけから「Spot回収で計算が中断した」とした記述は撤回する。
確実なVM停止、result の完了主張、取得証拠の不全を区別し、ログ破損の発生時刻や原因は断定しない。
旧 stage 12 / 13 は認定可能な再利用証拠から除外し、元ログの整形や完了主張だけで成功へ置き換えない。
変更後CPUで17:56:43–17:59:57 UTCに行った復旧buildは、
[独立検証](../saved-profile/vm06-build-report.json)で新規8 stageの成功を確認した。
source06 / baseline source、compiler、binary、全出力hashとcleanupが一致し、
新boot `159efb96-10fb-4ce4-bb0f-bc2b27ee618f` とAMD CPUはbuild前後で変わっていない。
旧native cacheは使わず、空のtargetからcurrentをbuildし、その新しい依存だけをbaselineへ渡した。
source06のaudit exampleは保存profileの品質検査用であり、
[source03のpaired性能比較](../pipeline/vm06-comparison-report.json)や
[source03のphase計測](../phases/vm06-phase-report.json)とはsource・binaryを区別する。

最終6 bundleのdownloadとpayload検証後、VM06を明示削除した。
[公式operation](cleanup-vm06/operations.json)は target ID `5209515640504390740`、
状態 `DONE`、終了時刻2026-09-25 18:39:47.045 UTCを示す。
最初の削除前検査はdisk projectionの欠落で拒否され、削除しなかった。
続く[完全な対象確認](cleanup-vm06/verified-target.json)で同一ID・単一boot disk・autoDeleteを検証してから削除した。
[18:40:38 UTCの照合](cleanup-vm06/reconciliation.json)ではinstance・disk・予約アドレスの一覧がすべて空だった。
元の20:09:11 UTC終了期限より前の削除で、追加のVMや予約はない。
実請求額は未確認のため、[budget.json](budget.json)のVM06分5 USDを含む合計15 USDの予約を保持する。

## VM07: byte codecと002診断

source07は`88ffa5d`をbaseとするdirty archiveで、production変更はSOL byte serdeだけ。
[source manifest](source-07-manifest.json)、[通常検証](../validation/vm07-report.md)、
[codec比較](../codec/vm07-report.md)、[002実測](../reference/vm07-002-report.md)を別々に照合する。
codecは96 sampleの全保存bytesが一致し、11条件で読込み等の事前改善基準を満たしたが、
Flop書込みはmedian約21%悪化した。外部24caseの品質認定や全solveの改善には読み替えない。

5 bundleの一意の回収量はsidecar込み48,695,414 bytes。元codec bundleの容量制限で
除外されたcanonical 1件はsupplementで追加回収し、元記録は書き換えずhash・bytes・元pathを照合した。
全必須canonicalの独立byte比較が成功した後、VM07とauto-delete diskを削除した。
[operation](cleanup-vm07/operations.json)のtarget IDは`841167209049583155`、
削除完了は2026-09-25 20:32:24.560 UTC。
[20:34:35 UTCの照合](cleanup-vm07/reconciliation.json)でinstance・disk・予約addressは空だった。
実請求は未確認で、VM02/05/06/07の予約を合計20 USD保持する。予算残を仮定して追加起動しない。

## 費用明細とローカル資源の追加観測

2026-09-25 20:42–20:47 UTC頃に、認証済みBilling Reportsで対象projectと9月25日を
選択した[費用観測](billing-observation-20260925.json)を追加した。表は「表示する結果がありません」、
表示合計はJPY 0で、実験日のSKU明細は取得できていない。
[Googleの説明](https://docs.cloud.google.com/billing/docs/how-to/view-history)では費用反映に通常1日、
場合によって24時間超を要する。ゼロ請求や余剰予算の証拠には使わず、20 USDの予約を保持する。
この照会でVM作成・課金設定変更は行っていない。

同時期の[ローカルhost観測](host-probe-after-vm07.json)は空き物理RAM約3.58 GB、
利用可能commit約1.44 GB、memory load 89%だった。この条件では追加の重いbuild/solveを行わず、
参照入力の取得と軽い検証だけを進めた。恒久的なマシン容量不足の判断ではない。

## ローカル回収済み bundle の転送台帳

[transfers.json](transfers.json)は実際にローカルへ到着した15 bundle、そのmanifestとSHA sidecarを記録する。
既存10件4,459 payloadに、VM07の5件1,463 payloadを追加照合した。未回収payloadは0。
collectorのskip原記録1件は保持し、supplementでの回収先を結び付けた。

| bundle | archive bytes | sidecarを含む保持済み bytes | 内容 |
|---|---:|---:|---|
| VM05 build | 8,635,436 | 8,644,167 | baseline / source02 build、binary、通常検証 |
| VM05 pipeline | 11,950,377 | 12,421,995 | candidate1 / `.sol` v2 の pilot と比較 |
| VM05 v3 checks | 29,508 | 39,494 | source03 通常検証、source02 ignored 検査 |
| VM06 early checks | 1,373,699 | 1,408,625 | source06 通常検証、先行失敗と source |
| VM06 recovery | 6,928,478 | 7,003,003 | AMDでのsource06 / baseline品質audit build、旧破損証拠の隔離 |
| VM06 v3 results | 17,934,814 | 18,423,704 | source03 `.sol` v3 のpaired性能比較と成果物 |
| VM06 saved audits | 544,160 | 700,498 | source03の保存policy 18件をsource06 / baseline exampleで再評価 |
| VM06 phases | 24,168,476 | 25,068,553 | source03のphase on/offと校正、元の非計装比較とは別run |
| VM06 current | 13,268,830 | 13,877,354 | 最終source06の検証・比較・保存profile campaign |
| VM06 river diagnostics | 582,822 | 643,074 | HU-R0-017 / 019の診断、外部品質の受入根拠にはしない |
| VM07 checks | 2,562,647 | 2,578,685 | source07通常5検証、source archives |
| VM07 build | 9,327,291 | 9,709,153 | 8 stagesとsource files、4 binaries |
| VM07 diagnostic002 | 7,023,707 | 7,086,961 | 132 menuの診断と保存後profile監査 |
| VM07 codec | 28,468,679 | 28,926,174 | 96 sample、事前計画、canonical、実行環境 |
| VM07 codec supplement | 392,224 | 394,441 | canonical 1件の補完、終了後systemd照会（実制限の確認不能） |
| 合計 | **133,191,148** | **136,925,881** | 約0.127522 GiB |

これは確認できた一意の回収物の byte 合計であり、SCP 等の protocol overhead、再送・重複 download、
未完了転送を含む通信総量でも、GCP の請求対象 byte 数でもない。展開後の payload は二重加算しない。
失われた VM05 `v3-results` は回収済みに含めず、ローカルで生成して送信した source / tool archive も
download としては計上しない。追加回収時の累計枠確認には既存の予約条件を使い、この棚卸しから
予約解放や追加支出の許可を導かない。
