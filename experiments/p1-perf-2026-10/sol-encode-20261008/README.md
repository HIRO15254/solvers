# P1 `.sol`の値block符号化（T19、2026-10-08）

状態: 計測完了、採用。問いは2つ。
- `.sol`生成中のprofileで25%を占めた`compatible_reach`（handごとの`nlh::combo_cards`の線形探索）を除くと、保存は短くなるか。
- 短くならないなら、保存の壁時計を決めているのはどこか。そこを直すと保存はどれだけ短くなるか。

関連は[P1性能計画](../../../docs/plans/p1-performance.jp.md)第3節、Linear SOL-15。前段のprofileは[T14〜T16の受入](../accept-t14-t16-20261008/README.md)（`raw/gtowb_save_profile_t16.txt`に同じものを置いた）。
再現状態は`verified`（記載scriptで計測したlog・JSONと集計を保持）。集計は[result.json](result.json)。

## 条件

- GCP c2d-highcpu-32 Spot（VM `p1perf-7`、europe-west4-b、AMD EPYC 7B13、16 core/32 thread、62 GiB、boot disk pd-balanced）、rustc 1.97.0。
- 比較の基準はT16（`a56e307`）。変更は`a56e307`への未commitの差分として2段階で測った。
  - cards: `compatible_reach`がcombo→card indexの定数表（1,326組）を引く。
  - T19: cardsに加え、値blockをEV passの各workerでpostcardと同じbytesへ一括符号化し、書き手はsref順に`write_all`するだけにする。
- 木は`configs/c_gtowb.toml`（GTO Wizard風、f32 storage 20.1 GiB）。targetを外し、25 iteration・`final_checkpoint = false`・32 threadsで解いた。
  停止（`done:`行）からprocess終了までを保存時間とした。出力先は2通り。
  - pd-balanced disk: `scripts/setup10.sh`（cards）、`scripts/setup13.sh`の後半（T19）。
  - tmpfs（`/dev/shm`）: `scripts/setup11.sh`（cards）、`scripts/setup13.sh`の前半（T19）。
- thread別のprofile: `scripts/setup12.sh`がtmpfs出力の保存中だけ`perf record`し、threadごとに集計した。
- 出力の一致はVMの`verify_save solution`で比べた（`.sol` payload、wall_secs以外）。

## 結果

保存時間（停止からprocess終了まで、秒）:

| 出力先 | T16 | cards | T19 |
|---|---|---|---|
| pd-balanced | 45.66 / 45.68 / 46.09 | 45.76 / 45.60 | 45.56 |
| tmpfs | 20.95 / 21.20 / 20.80、20.70 / 20.75 / 20.55 | 20.72 / 20.46 / 22.33 | 13.86 / 13.83 / 13.71 |

- cardsだけでは、diskでもtmpfsでも保存時間は変わらなかった。`compatible_reach`は並列のEV pass内にあり、壁時計を決める区間ではない。
- thread別のprofile（tmpfs）では、1つのthread（書き手）が保存時間の約8割で動いていた。その標本の43%は`ValueBlock`のserializeだった。
  値は`Vec<u8>`だが、serdeの既定実装では1 byteずつpostcardの出力へ渡る。戦略blockは既に一括符号化している（T3b）。
- T19ではtmpfsの保存が20.7秒から13.8秒（33%短縮）、process全体が61.9秒から54.2秒になった。
  pd-balancedでは書込み速度が律速のまま（46.09→45.56秒）。速いdiskほど効果が大きい。
- `.sol` payloadは、cards・T19ともT16とbit一致した（payload 9,507,078,156 bytes、正規化blake3 `e1d8e58d…`）。
- peak RSSは変わらない（T16 22.65〜22.68 GB、T19 22.65 GB）。
- memory見積りの保存作業領域は、action nodeごとに値block headerの上限14 bytesを加えた（gtow_bの229万nodeで約32 MB）。
  見積りと実確保の一致試験に合わせた。

## 判断

- T19を受け入れる。出力を変えずに、速いdiskでの停止後の保存を約3分の1短くする。
- 保存はdiskの書込み速度で決まる区間が残る。pd-balancedでは`.sol`（6,363.51 MB）の書込みに約45秒かかる。

保持物の識別は[manifest.json](manifest.json)。各runの`.sol`は保持しない。
