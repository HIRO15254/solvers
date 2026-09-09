# Multiway convergence実験 GCP Spot予算計画（2026-09-08）

## 最新状態（2026-09-08 22:15 JST）

ユーザーの「一旦VM片付けて」に従い、クラウド実験を停止した。
`mw-convergence-20260908-a`（us-central1-a、n2-highmem-8 Spot）と
付属30 GB boot diskは削除済み。2026-09-08T13:15:01Zの再確認で、
project `solvers-abstraction-20260723` のVM・disk・reserved IPはすべて0件。
追加VMは作成していない。ソース転送は自動承認レビューに拒否され、
転送・ビルド・クラウドでのsolver実験は未実施。短時間のVM起動分の課金は
発生し得るため、実際の請求額は未確定でありUSD 0とは記録しない。
認証、指定された請求先との連携、Compute APIの有効化は完了している。
以下は起動前に作成した旧計画で、現在の実行状態ではない。

この文書は、ユーザーが許可した総額 **USD 20** の範囲で、Multiway
Preflopの小規模収束ベンチを外部Spot VMへ移す場合の事前計画です。これは
実行記録ではありません。現在はGCP projectの確認と認証が保留中で、VM、disk、
static IP、bucket、その他の課金resourceは作成していません。
この作業による追加利用費は **USD 0**です。

## 価格と上限

2026-09-08に確認したGoogle Cloud公式Spot価格表では、`us-central1` Iowaの
`n2-highmem-8`（8 vCPU、64 GiB）は **$0.314368 / VM-hour** です。
[Google Cloud Spot VM pricing](https://cloud.google.com/spot-vms/pricing?hl=en)

この単価で初回は **単一Spot VMを最大4時間** とします。

| 項目 | conservative budget |
|---|---:|
| VM単価（n2-highmem-8、us-central1） | $0.314368 / h |
| 初回上限（1 VM × 4 h） | $1.257472 |
| boot disk・IP・egress等の予約 | **$3.00** |
| 初回終了後の予算残 | $15.742528 |
| 全体の理論的VM時間（予約を除く） | 約54.1 h |
| 運用上のaggregate VM上限（初回を含む） | 48 h（$15.089664） |
| aggregate上限＋予約の計画額 | $18.089664 |

48時間は価格変動、短時間課金、disk/image、network、転送、再試行を吸収する
ための運用上限です。残り約$1.91は使い切らずに残します。Spot価格は変動する
ため、実行開始時に公式価格表を再確認し、`$0.314368/h` を固定価格として
扱いません。公式説明にもSpot VMはpreemptされ得ることと価格変動が記載されて
います。

Google Cloudの利用料表示には遅延があるため、実行直後の
請求画面だけでゼロと判断しません。開始・停止時刻、VM名、zone、machine type、
価格確認時刻、実行ログを保存し、後日のbilling明細と照合します。

## 実験ゲート

`examples/bench_multiway/3max_2bb.toml` のpush/fold sanity fixtureは先にローカルで
検証します。ローカルで短時間に終わる実験のためにはVMを起動しません。
クラウド実行は、より大きいTreeや多数seedの比較に時間・メモリ上の利点があると
確認した場合に限定します。初回は1台・1回・最大4時間とし、同じ小規模fixtureで
動作確認してから、事前にresource preflightを通した条件へ進みます。
`.mwsol`はpreflop nodeだけを保持するため、一般postflop treeの完全なpolicy
比較やGTO Wizardとの一致をこの実験から主張しません。実験runnerは各variantの
config、logs、run directory、held-out evaluation、inspect結果を保持します。

初回4時間の結果を確認してから、次の条件を満たす場合だけ同じSpot VMまたは
新しい単一Spot VMで追加実験を検討します。

- VMがpreemptされた場合でもcheckpointとraw logsが残り、再開可能である。
- 実験のaggregate VM時間が48時間を超えない。
- runtime再確認価格と予約費を含めた見積もりがUSD 20以内である。
- 追加の複数VM、persistent disk、static IP、外向き転送を暗黙に作らない。

VM作成時にCompute Engineの最大実行時間と終了動作を設定し、CLI側timeoutだけに
依存しません。予算alertは強制上限ではないため、開始前に最大runtimeに基づく
費用を予約します。Spotのpreemptionと価格再確認により再試行が必要な場合も、4時間単位で停止し、
aggregate時間を記録してから次の起動を判断します。実験終了後はVMを停止・削除し、
不要なdiskとIPを残しません。

## 認証と実行状態

portable `gcloud` SDKは `.cache/cloud-tools/sdk583` に配置済みですが、active
account/projectの最終確認は保留中です。認証情報・token・secretはログやこの文書に
保存しません。projectが確定し、billing accountとquotaが確認できるまで、課金
resource作成や変更は行いません。

本計画はshell起動scriptを含みません。実行時には、現行CLIの`--cache-dir`を使い、
各runのeffective configとbinary SHA-256、git revision、wall/build/evaluation
metricsを成果物へ記録します。Spot価格、課金状態、作成resource一覧は実行直前と
終了後にread-onlyで再確認します。
