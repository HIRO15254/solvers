# VM12 の読み取り確認・費用案

2026-09-26 20:17:35–20:17:45 UTC に読み取り専用の 9 コマンドを実行し、
すべて exit 0・有効 JSON を得た。VM・disk のプロジェクト全体一覧は空だった。
この directory は起動前の案と観測であり、予約・起動・停止・型変更・削除や
`../budget.json` の編集は行っていない。

請求先は project `solvers-abstraction-20260723` から billing account
`015A1D-8A8F19-EC7035`（表示名「趣味」、通貨 JPY）へのリンク。
billingEnabled と account open は true。請求額・実効為替・税・反映遅延は今回の
応答では分からず、累計実費は不明のままとする。

## Quota と型

`us-central1` は UP。観測した使用量/limit は次の通り。

| 指標 | 使用量 / limit |
|---|---:|
| region CPUS | 0 / 200 |
| region E2_CPUS | 0 / 24 |
| region N2_CPUS | 0 / 200 |
| region PREEMPTIBLE_CPUS | 0 / 0 |
| global CPUS_ALL_REGIONS | 0 / 32 |
| region INSTANCES | 0 / 24 |
| region SSD_TOTAL_GB | 0 / 500 GiB |
| region IN_USE_ADDRESSES | 0 / 8 |

[公式 quota 表](https://docs.cloud.google.com/compute/resource-usage) は E2 の通常枠を
N1 と共有する `CPUS`、N2 を独立した `N2_CPUS` と記載している。一方 API は
`E2_CPUS = 24` も返した。このフィールドの今回の適用を解決したとは主張しない。
E2 32 vCPU の起動を quota 確認だけで保証せず、実際の操作応答を保持する。
N2 32 vCPU は観測した通常 family 枠と global 枠に収まる。

[Spot 公式説明](https://docs.cloud.google.com/compute/docs/instances/spot) によれば、
専用 preemptible 枠のない場合は通常枠を利用する。`PREEMPTIBLE_CPUS = 0` を
Spot 用の余裕とは数えず、割当経歴や今回の作成許可までは判断しない。
Quota と Spot 空き容量は別であり、いずれの型も今回の起動成功は未検証である。

`us-central1-b` の型定義は `e2-standard-4` が 4 vCPU / 16 GiB、
`e2-highcpu-32` と `n2-highcpu-32` が 32 vCPU / 32 GiB。全型の
`isSharedCpu` は false。global 枠を使い切るため、4 vCPU bootstrap と
32 vCPU 測定を別 VM で並行させず、同一 VM を停止して拡大する案とする。
再起動・family 変更で実 CPU が変わり得る。bootstrap host 固有の
`target-cpu=native` binary を無検証で転用せず、固定した互換 target または
測定 host での build と起動確認を実験 protocol に含める。

## 費用案と停止境界

通常料金を保守的な計算根拠とし、Spot の現在単価や割引額を推測しない。
[公式 compute 表](https://cloud.google.com/products/compute/pricing/general-purpose)
の Iowa / Default (USD) は e2-standard-4 が $0.13402284/h、
e2-highcpu-32 が $0.79152384/h、n2-highcpu-32 が $1.147136/h。
最大型を上回る **$1.15/h** を全計算時間に使う。

| 枠 | 計算 | USD |
|---|---|---:|
| compute | 1.15 × 1.02 h | 1.173 |
| 40 GiB balanced disk | 40 × 24 h × 0.000137 | 0.13152 |
| Spot ephemeral IPv4 | 1.02 h × 0.0025 | 0.00255 |
| download、最大 1 GiB | 1 × 0.30 | 0.30 |
| 税・遅延・その他予備 | 固定 | 1.00 |
| 合計案 | | **2.60707** |

[disk 公式表](https://cloud.google.com/compute/disks-image-pricing) の
$0.000136986/GiB/h を切り上げ、STOP 後の回収を含む disk 24 時間を確保した。
[network 公式表](https://cloud.google.com/vpc/network-pricing) の Spot IPv4 は
$0.0025/h。Iowa から Asia（Korea・Indonesia 除外）への最初の有料帯は
$0.12/GiB だが、無料枠を控除せず $0.30/GiB を転送予備として使う。
実際のアカウントは JPY であり、この USD 計算は請求額の保証ではない。

**新規 $3 の予約案**により、既存未精算 held $35 と合わせて $38/$40、
未予約残額 $2 となる。これは root が reservation を更新する前の案であり、
本 preflight は予算を消費済み・追加予約済みとは記録しない。

初回 create の前に **元の絶対 STOP を 1 時間以内**で固定し、bootstrap、
停止、拡大、再起動を含めて延長しない。測定期限はその STOP の **15 分以上前**。
見積りの 1.02 時間は丸め・停止遅延の予備であり、実行期限の延長許可ではない。
E2 を優先し、quota・容量不足時の N2 fallback も同じ期限と $3 枠内に限定する。
取得・hash 確認後に VM と自動削除 boot disk を明示削除し、absence を確認する。
40 GiB pd-balanced boot disk は autoDelete、保存対象 download は 1 GiB 以下。

## 証拠の読み方

`commands.json` と各 `*.command.json` / `*.result.json` に argv・開始終了時刻・
exit code・process-local 環境指定・stdout/stderr の SHA-256 を保持する。
raw stdout/stderr は byte のまま保存し、恒久的な SDK 設定を変えていない。
表示名の原 bytes は UTF-8 の「趣味」であり、端末の文字表示は証拠に使わない。

`pricing-sources.json` は公式 HTTP response の URL・取得時刻・全 response hash と
保存抜粋 hash を持つ。価格 row・列名・直前の Iowa label は原 HTML の抜粋。
全 response は保持していないため抜粋から元ページ全体は再構成できない。
`draft.json` は今回の観測・費用計算を構造化した案、`files.json` は自身を除く
本 directory の hash 一覧である。実験結果・性能認定を含まない。
