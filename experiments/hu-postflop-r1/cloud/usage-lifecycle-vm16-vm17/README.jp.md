# VM16・17と追加VM18の通常E2料金による再評価

[report.json](report.json) は、取得済みの原本から **合計$0.90の予約復活**を提案する。
台帳の現在値はVM16=$1.90、VM17=$1.80、VM18=$1.50。適用すれば全予約は$39.00から
$38.10、利用可能額は$1.00から$1.90となる。**この監査では台帳を変更していない。**
請求額は不明であり、見積は保証された請求上限ではない。

| VM | 既存保持額 | 全期間/重ねる32vCPU/disk（分） | 保守的見積USD | 新保持案 | 復活案 |
|---|---:|---:|---:|---:|---:|
| 16 | 1.90 | 26 / 13 / 26 | 1.353993114 | 1.40 | 0.50 |
| 17 | 1.80 | 39 / 21 / 40 | 1.475869100333 | 1.50 | 0.30 |
| 18 | 1.50 | 34 / 13 / 34 | 1.363991970 | 1.40 | 0.10 |

各VMで元の **$1予備費と512MiB転送枠を全額維持**する。全期間、32vCPU枠、diskの期間に
それぞれ120秒を加え、分単位で切り上げる。全期間に小型VM料金を計上した上で、stop/resize前
から小型へのresize完了までの32vCPU料金を重ねる。停止中の時間も、小型の重複分も差し引かない。

成功SDK引数、同一数値IDの機種readback、記録済み全start/resizeの集合から、両機種が
`e2-standard-2` と `e2-highcpu-32` だけであることを確認する。取得済み一次HTML excerptの
Iowa/default列を直接読み、通常料金 **$0.06701142/h、$0.79152384/h** を使う。
Spot割引・無料枠・creditは引かない。disk=$0.000137/GiB/h、IPv4=$0.0025/h、
送信=$0.30/GiBは既存の保守的単価を維持し、一次excerptの料金がこれを超えないことを検査する。

## 身元・削除・diskの根拠

- VM16：instance `1127898822003203169`、disk `4260965739450932321`。
  作成要求`04:58:50.8833182Z`、delete DONE `05:21:46.211Z`、空在庫確認`05:22:19.881926Z`。
  disk作成時刻`04:58:54.817Z`は明示原本にある。32vCPU枠は`05:02:07.510172Z`から
  `05:12:19.031415Z`まで。作成・回収bootの同disk URI/40GiB/autoDeleteと数値IDを照合する。
- VM17：instance `2775050120395558750`、disk `2052432309614088030`。
  作成要求`06:03:26.7694177Z`、delete DONE `06:39:13.383Z`、空在庫確認`06:39:59.735307Z`。
  **diskのcreationTimestampは欠落している。** 作成前のdisk一覧が空だった照会の開始
  `06:02:59.618400Z`を保守的な起点にする。これは作成時刻の推定値ではなく、存在期間の上限側の枠。
  作成時と最終削除前の同disk URI/40GiB/autoDelete、削除前数値IDと空一覧を照合する。
  32vCPU枠は`06:06:31.458832Z`から`06:24:56.413189Z`まで。
- VM18：instance `3769585733775752220`。既存の[固定監査](../usage-audit-vm18/report.json)を
  保存コピーから完全再生し、通常E2の2単価だけを置き換える。34/13/34分の期間、全転送・予備費・
  120秒余裕と原本検査は変更しない。元のreportとreaderは編集しない。

各VMの同ID errorなしdelete DONE、同project・適切なname filterで取得した
instances/disks/addressesの空一覧、SDK引数、stdout/stderr hashを検査する。
VM17の4回目の起動は元STOPを延ばさない回収限定例外の原文と成功receiptを検査する。
実削除は元STOPを47.383秒超えたが、その後の空在庫まで全期間に含める。
SDKの保存記録による身元連鎖であり、Cloud Audit Logs全件を取得したとはしない。

## 転送・欠測

VM16は3回の成功downloadと115,423,261 bytesの検証済みarchive receiptを照合する。
成功progressを上側へ丸めたpayloadは115,834,880 bytes。Monitoring観測値は
121,242,568 bytes、uptimeは1077.68152999999998872秒。

VM17は10回のdownload記録を保存する。中断した最初のarchiveは、受信済み断片合計だけでなく
**全127,201,851 bytes**を数える。失敗したpart00再試行にも全50,331,648 bytes、clientが
拒否したsidecar試行にも1MiBを計上する。再回収の全archive・sidecar・診断結果など成功分を
加えたpayload枠は306,841,147 bytes。検証済み最終archiveは127,202,699 bytes。
これはwire trafficの厳密上限ではなく、元512MiB枠を据え置くための保守的照合である。
Monitoringの99,729,092 bytesは既知転送より少なく、不完全な観測のまま扱う。

全metricの欠測、区間境界、最初/最後の未観測量は不明であり、0補完・観測uptimeによる料金減算は
行わない。将来invoiceや追加証拠が差を示した場合、保持予算を再評価する必要がある。

## 再生と保存境界

```text
C:/Python313/python.exe -B experiments/hu-postflop-r1/cloud/usage-lifecycle-vm16-vm17/test_analyze.py
C:/Python313/python.exe -B experiments/hu-postflop-r1/cloud/usage-lifecycle-vm16-vm17/analyze.py --check
```

[analyze.py](analyze.py) と6件の[純テスト](test_analyze.py) が算術・切上げ・欠測転送枠・
単価/対象改変拒否を検査する。[inputs-manifest.json](inputs-manifest.json) と
[additional-vm18-inputs.json](additional-vm18-inputs.json) は576個、1,727,087 bytesの
小さい原本とsource snapshotsを固定する。新しいAPI取得、native実行、archive読取り/展開はない。

最初のreport生成はVM17のdescribeがnameを省略した形式である点、次は回収JSONが
複数行stdoutの末尾にある点で明示的に失敗した。数値ID＋固定target引数を保った形式対応と
一意の回収JSON行の抽出を修正し、両失敗ログを残す。欠落値や欠測を作って埋めていない。
成功再生、source前後pins、ログは[checks01.json](checks01.json)に保存する。
