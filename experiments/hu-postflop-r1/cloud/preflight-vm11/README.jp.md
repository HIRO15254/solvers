# VM11 の読み取り確認・費用案

2026-09-26 UTC に次の最終 pipeline 比較向けの準備を確認した。予約・起動は
実行していない。`draft.json` は未実行案であり、`../budget.json` は変更していない。

請求先は project `solvers-abstraction-20260723` から billing account
`015A1D-8A8F19-EC7035`（表示名「趣味」）へのリンクで、billingEnabled と account open
は true。プロジェクトの VM と disk 一覧はいずれも空だった。請求額や税、反映遅延を
今回の API 応答からは確認できず、累計実費は不明のままとする。

`us-central1-b` の `e2-standard-4` は 4 vCPU・16 GiB、共有 CPU 型ではない。
region は UP、使用量は CPU 0/200、E2 CPU 0/24、instance 0/24、SSD 0/500 GiB、
利用中アドレス 0/8。`PREEMPTIBLE_CPUS` は 0/0 なので、独立した Spot 専用枠の
余裕は主張しない。[公式の quota 説明](https://docs.cloud.google.com/compute/docs/instances/spot)
では専用枠未付与の場合は通常枠を使う。今回と同じ project・zone・型の VM10 は
直前に起動実績があり、上記の通常枠には余裕があるが、Spot 空き容量・今回の作成許可・
実際の起動成功は未検証である。

費用案は Spot 4 vCPU、40 GiB balanced disk、最大 3 時間、回収 download 最大
1 GiB。計算は Spot の現在価格を推測せず、[公式 compute 表](https://cloud.google.com/products/compute/pricing/general-purpose)
の既定価格 $0.13402284/h を $0.14/h に切り上げる。
[disk 表](https://cloud.google.com/compute/disks-image-pricing) の $0.000136986/GiB/h
を $0.000137 に切り上げ、disk は STOP 後の回収を含む 24 時間分を確保する。
[network 表](https://cloud.google.com/vpc/network-pricing) の Spot IPv4 $0.0025/h、
転送分 $0.30/GiB の保守的な枠、その他 $1 の余裕を加えると **$1.86187**。
$3 を新規予約する案は、既存 held $32 と合計 $35/$40、未予約残額 $5 となる。
これは請求予測の精密値や価格保証ではなく、実行枠の概算である。

起動時に絶対 STOP deadline を 3 時間で固定し、自動再起動しない。証拠回収・hash
確認後に VM と自動削除 boot disk を明示削除する。作成失敗・Spot 回収時にも
元の予算と deadline を超えて自動延長しない。起動前に root が測定 protocol と
source を固定し、予約台帳を更新してから既存 `launch.ps1` を使用できる。

原出力は stdout/stderr を bytes のまま保存し、`commands.json` と
`configured-commands.json` が引数・取得時刻・SHA-256 を持つ。初回 SDK の Python
探索が一時パスを stdout に混入し、JSON として解析できない 3 応答があった。
その原文を保持し、Python と UTF-8 を process environment で明示して該当 3 件と
表示名の計 4 件を再取得した。`configured-*` を判断に使い、disk・machine type は
初回の有効 JSON を使う。恒久的な SDK 設定は変更していない。

価格の原 HTML は該当行・Iowa ラベル付近のみを保存した。`pricing-sources.json`
は取得先・時刻・全 HTTP response hash・抽出行 hash を記録する。compute の列名は
別取得の `pricing-compute-header-source.json` に結びつく。全ページは保存しておらず、
これらの小さな抜粋から全 response を再構成することはできない。
