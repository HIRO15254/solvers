# 取得済み使用量による追加の未使用予約枠

この提案は **2.10 USD** の未使用枠を再利用し、保留総額を39.95から37.85 USD、
未予約枠を0.05から2.15 USDへ変更できるという資源使用量の再評価である。
台帳は変更していない。請求額は未確定で、Spot割引、credit、無料枠を計算に入れていない。

| VM | 現在保留 USD | モデル USD | 提案保留 USD | 復活 USD |
|---|---:|---:|---:|---:|
| 08（base＋scale32） | 4.50 | 3.663995032 | 3.70 | 0.80 |
| 09 | 1.60 | 1.443047333 | 1.50 | 0.10 |
| 10 | 1.80 | 1.591027333 | 1.60 | 0.20 |
| 11 | 1.80 | 1.509638333 | 1.60 | 0.20 |
| 12 | 1.80 | 1.608794667 | 1.70 | 0.10 |
| 13 | 1.60 | 1.376456333 | 1.40 | 0.20 |
| 14 | 1.90 | 1.637026480 | 1.70 | 0.20 |
| 15 | 2.00 | 1.695487658 | 1.70 | 0.30 |

VM08は同じinstanceの2予約を合算する。2つの当初1 USD予備費は両方残す。
baseを2.20 USD、scale32を既存の1.50 USDに割り当てる。実測されたE2 highcpu32の
通常単価0.79152384 USD/hを使用し、N2 fallback用の1.15 USD/hを置き換える。
4 vCPU用0.14 USD/hを全生存期間に重ねる方式と、24時間のdisk枠は維持する。

VM09–13は、launchの新規40 GiB boot disk要求、作成時の単独autoDelete disk URI、
削除前disk数値ID・instanceへの接続、削除後の空inventoryを照合する。
disk時間をlaunchからabsenceまでの全期間＋120秒、分単位切上げで扱う。
個別creationTimestampが記録されていないVMでも、作成日時を捏造しない。
VM12は初期4 vCPU期間も含め、当初N2 fallback上限1.15 USD/hの全期間課金を維持する。
VM09のdisk absenceはrawの空inventoryで確認できるが、cleanup概要にdelete operationは含まれない。

VM14/15は既存の同一instance IDの2→32→2 lifecycle検査を再実行し、
通常E2単価（2 vCPU:0.06701142、32 vCPU:0.79152384 USD/h）に更新する。
高CPU期間はstop要求前から縮小完了後まで、さらに120秒を加えて分単位切上げとし、
全期間の小VM課金と重ねたままにする。

全VMで、当初の不確実性予備費と1 GiBのnetwork allowanceを残す。
Monitoringの欠測・端点外通信は不明のままで、0としていない。
これは正式な請求上限の証明ではない。VM02/05は過去disk absenceのraw証拠が弱いため対象外。

`validate.py` は既存のVM08/13およびVM06/07/14/15の検証器を再実行し、
345件の取得済みファイルpinと新しいDecimal計算を確認する。価格は9月27日の
公式ページ取得物を用い、新しい取得や現時点の料金確認を主張しない。
小さいJSON・source・HTMLの読込みのみで、API・native code・archiveへのアクセスは行わない。

```text
python -B experiments/hu-postflop-r1/cloud/usage-followup-20260928/validate.py --check
```

台帳反映後の再検証では、rootが保存した反映直前の台帳を
`--budget <immutable-before.json>` で指定する。反映は独立レビュー後に別途行う。
