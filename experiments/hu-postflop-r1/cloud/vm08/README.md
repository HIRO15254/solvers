# Spot VM08の実行枠と資源履歴

初期20 USDに加え、2026-09-26に利用者が2回各10 USDを追加し、R1の累計上限は40 USDとなった。
計算時間より節約を優先し、32 vCPU試験も求めている。2回目の追加許可時点では
未精算の既存20 USDとVM08の3 USD、合計23 USDを保持する。未予約枠は17 USDで、
32 vCPU枠はまだ予約・起動していない。
請求先は既存の`015A1D-8A8F19-EC7035`、projectは`solvers-abstraction-20260723`。
予約額は実請求額ではない。[台帳](../budget.json)を参照する。

## 初回2 vCPUでのcodec比較

- VM: `solvers-r1-20260926-08` / `us-central1-b` / instance ID `1635613034982056517`
- 初回は`e2-standard-2`、2 vCPU、8 GiB、40 GiB pd-balanced、Spot。
- 起動試行: 2026-09-26 01:15:19 UTC。絶対停止期限: **07:15:19 UTC**。
- Spot回収と期限到達の動作はSTOP。自動再起動なし、boot diskは明示VM削除時auto-delete。
- disk保持は起動から最大24時間で予約。証拠回収後は期限を待たずVM/diskを明示削除する。
- 最大ダウンロード1 GiB。再起動や追加起動で期限・予算を自動延長しない。
- 初回boot: `ec1e2ee2-ab43-4ae0-b342-e656914bb92a`、AMD EPYC 7B12、Ubuntu 24.04、Rust 1.97.0。

[通常料金](https://cloud.google.com/products/compute/pricing/general-purpose)の0.06701142 USD/hを
0.07へ切り上げ、Spot実単価の保証としては使わない。
[disk](https://cloud.google.com/compute/disks-image-pricing)は0.000137 USD/GiB/h、
[Spot IPv4](https://cloud.google.com/vpc/network-pricing)は0.0025 USD/h、転送は0.30 USD/GiBで予約する。
compute/IP 6.02時間、disk 40 GiB×24時間、転送1 GiB、税・価格差等1 USDを足すと
1.86797 USDとなるため2 USDを予約した。料金の確認日は2026-09-26。

この初回枠では当時の候補の通常workspace検証とLinux SIGINT検査を先行し、別々のsource/targetで
旧codec・直前bulk codec・context再利用候補をrelease buildする。
計算はCargo 1 job、Rayon 1 thread、test 2 threadsの順次実行とする。
比較中にbuildや転送を重ねない。source・input・binary・bootを固定し、保存結果の全byte一致を要求する。
性能比較は`codec/context-reuse/linux-spot-20260926/`の事前計画に従う。

[展開検証記録](setup-result.json)は、転送後の全source/inputの元byte一致を確認したもの。
初回setupはmanifestの2つの空directory entryを許可せず、file書込み前に拒否した。
宣言済みdirectoryとregular fileを別に照合する修正後、新しい空の展開先で検証が完了した。
build serviceは2026-09-26 01:30:21 UTCに開始し、invocation IDは
`dfa384316ba143298152997b1d420cd8`。外側cgroupは6 GiB、swapなし、最大5時間で、
runnerの締切07:00 UTCとは別にVMの絶対期限07:15:19 UTCも維持する。

[回収済み検証](../../codec/context-reuse/linux-spot-20260926/complete-verification.json)は
83 stage・72 codec標本の完了、workspace 906 passed / 31 ignored、SIGINT追加1件、
元SOLとcanonicalのbyte一致を示す。CCtx候補の事前screenはfalseであり、採用しない。
これは初期range compact化を含む後続sourceの検証や、R1全体の品質受入を代替しない。

## 同一期限内の4 vCPU再起動とsource01検証

codec証拠の回収後、同じinstance/diskを停止して`e2-standard-4`、4 vCPU / 16 GiBへ変更した。
[変更後のGCP記録](resized-description.json)は再起動時刻2026-09-26 02:11:30.371 UTCと
元の絶対停止期限07:15:19 UTCを記録する。再起動後のbootは
`dd839b25-cea6-489f-b936-815bb8e7f124`。停止前後のbootとCPUを別に識別し、
初回2 vCPU codec時間と4 vCPUでの時間を同一host比較として合成しない。

予約は追加1 USDで合計3 USDとした。4 vCPU通常料金0.13402284 USD/hを0.14 USD/hへ切り上げ、
元の6.02時間すべてをこの単価で数え、既存のdisk/IP/転送/予備を含めた保守的見積りは
2.28937 USDである。期限、disk保持、転送の上限は延長していない。

後続検証の入力は[固定source01](../../range-scaling/source01/pack-report.json)。
最初のvalidation起動はOS監視でcgroupの`cpu.max`が存在しない場合を扱えず、Rust検証の開始前に失敗した。
source01を変更せず、外部validatorの修正版を別実行として使う。起動や監視の修正だけを
fmt/clippy/testの成功とは扱わず、完了したstageの元ログとsource/binary識別で結果を検証する。
このsource固定後の予算・計画書更新は後続の文書差分であり、source01のbuild対象へ遡って含めない。

## 32 vCPU比較の追加許可と実行手順

4 vCPU段階の検証後、約90分に限定した32 vCPU Spot計測枠を見積もって予約する。
同一host・同一bootでcompact 1/2/4/8/16/32 threadsとdense 1 threadの7条件を用い、
4 case各1 pilot、各条件のwarmup 1回と測定3回、合計112標本と4 pilotを保持する。
32 vCPUという資源指定を32物理coreや線形速度向上の保証とは扱わない。
起動前に累計40 USD枠、有限停止期限、回収・削除枠を照合し、予約と実際の起動は別の記録に残す。

これは実験資源と支出上限の記録であり、作業状態やR1受入の正本ではない。
小型VMでの新しい比較は旧VMの絶対時間と直接合成しない。
