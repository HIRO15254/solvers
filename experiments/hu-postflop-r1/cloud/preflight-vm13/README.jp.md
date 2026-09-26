# VM13: 現行版の工程別計測に向けた読取り確認

2026-09-26 UTC、`solvers-abstraction-20260723`を8個のread-only SDK commandで確認した。
全commandはexit0・JSONとして読め、対象projectのVM・disk・予約addressは空だった。
請求先は`015A1D-8A8F19-EC7035`、JPY、billing enabled/open。
実請求額は取得しておらず、過去の未精算予約38 USDを維持する。
displayNameにはSDKの文字化けがあり、今回の照合はaccount IDと通貨を使う。

`e2-standard-4`は4 vCPU/16GiB。通常CPU quotaのusageは0で、4CPU要求は
regional CPUS200、E2_CPUS24、project CPUS_ALL_REGIONS32の各上限内だった。
PREEMPTIBLE_CPUS0も原文に保持し、Spot用の追加割当があるとは解釈しない。
これは実際のSpot空きや起動成功を保証する情報ではない。

公式料金のHTTP応答hashと、必要なtable行・header・regionを保存した。
通常料金を切り上げた0.14 USD/hをcompute上限として使い、Spotの割引を予算計算に織り込まない。
実際の請求は契約通貨のSKU、利用時間、税等に依存する。

| 仮定 | 予約見積り (USD) |
|---|---:|
| compute 1.02h × 0.14 | 0.1428 |
| pd-balanced 40GiB × 24h × 0.000137 | 0.13152 |
| Spot IPv4 1.02h × 0.0025 | 0.00255 |
| 回収転送最大1GiB × 0.30 | 0.30 |
| 税・料金差・遅延などの予備 | 1.00 |
| 合計 | 1.57687 |

見積りは残る未予約2 USD内。最大1時間で絶対STOP、15分前に計測を終え、
diskは回収後に明示削除する。停止後のdisk費用は最大24時間まで上表に含めた。
並行VM・期限延長・自動再実行を予定していない。
[draft.json](draft.json)は未実行の費用案であり、予約台帳の変更や起動完了の証拠ではない。

料金根拠は[Compute](https://cloud.google.com/products/compute/pricing/general-purpose)、
[disk](https://cloud.google.com/compute/disks-image-pricing)、
[network](https://cloud.google.com/vpc/network-pricing)。
[commands.json](commands.json)と[pricing-sources.json](pricing-sources.json)に取得時刻と原bytesのhashを保持し、
[files.json](files.json)はdraft生成時の50ファイルを指す。この説明はその後追加した。
