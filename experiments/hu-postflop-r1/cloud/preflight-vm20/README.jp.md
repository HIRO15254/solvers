# VM20の公開料金確認

2026-09-28 04:23 UTCに公式公開ページ4件を認証なしで取得した。
21個の小さいHTML断片（料金行、表見出し、Iowaのregion表示）と元response hashを保持する。
過去の有限25秒timeout付き取得器を再利用し、N2行を対象から除いた。
API credentials、GCP資源の変更、予算予約、native実行は行っていない。

| 対象 | 通常単価 USD/h | Spot単価 USD/h |
|---|---:|---:|
| e2-standard-2 | 0.06701142 | 0.040212 |
| e2-highcpu-32 | 0.79152384 | 0.475008 |

balanced diskは0.000136986 USD/GiB/h、Spot IPv4は0.0025 USD/h、
Asia向け最初の有料egress帯は0.12 USD/GiB。

実験枠の提案額は **1.85 USD**。全47分を0.80 USD/hで計算し、
20 GiB diskを24時間、512 MiB通信を0.30 USD/GiB、当初の1 USD不確実性予備費と
IPv4を含めると **1.844385 USD** になる。Spot割引や無料枠は見込まない。
45分の絶対STOPに対する追加120秒は価格計算用で、実行延長を認めない。
機種は2 vCPUと32 vCPUのE2に限り、32 vCPUは有限測定期間のみ使用する。

`cost-proposal.json`の予算観測は取得時点の39.95 USD保留・0.05 USD未予約である。
未使用枠の照合が適用され、別途資源・実行制御の確認が済むまで起動可能とは主張しない。
正式な請求上限の保証ではなく、将来の料金・請求額は未確定。

```text
python -B experiments/hu-postflop-r1/cloud/preflight-vm20/cost-proposal.py --check
python -B experiments/hu-postflop-r1/cloud/preflight-vm20/check-pricing.py --check
```

通常価格・Spot価格の取得元とUTC時刻は`pricing-sources.json`と
`spot-pricing-source.json`に記録した。`checks.json`は全断片のpin、価格列とregion、
異なるFraction式による再計算を検証する。`derivation.json`は取得器と計算器のコピー元を示す。
