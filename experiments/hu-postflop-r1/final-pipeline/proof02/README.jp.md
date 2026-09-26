# Final pipeline proof02: 現行 CLI・保存形式の比較

同じ品質目標での全 CLI 比較と、保存後 profile・codec の検査が完了した。
build 9 stage、測定・照合 156 process がすべて passed。
時間と対象 I/O の事前条件を満たしたが、OS メモリの改善判定は **未成立**。
結果と適用範囲は [日本語レポート](report.jp.md)、数値の正本は
[trusted checker を経た report.json](report.json) を参照する。

比較元は `88ffa5dd4583e5c8bae84e20e9b8390cb719e4f0`（SOL3 / CKPT1）、
比較先は `11e4062ba1735e58b60d12999cb23ed10fd1a163`（SOL4 / CKPT2）。
両者の production source・Cargo 入力・契約文書は
[source pins](../source-pins.json) と archive で固定され、同じ boot で新規 target
から build した。2つの研究用 example は別に検査され、production CLI の
振る舞いを計装で置き換えていない。新sourceの958 workspace test 等の基礎検証は過去の
同一 production source の証拠で、この campaign の再実行件数には含めない。

対象は synthetic River / Turn / 限定 Flop の3例、no rake、F32、Full 保存、
solver 1 worker。Flop は各 player **3 combo** で、Turn/River は check down する。
全レンジ・通常規模の全 street betting tree、外部24例、I16 / NoRivers / ICM、
この source の32 worker性能を認定する測定ではない。

proof01 の失敗記録は [別に保持](../proof01/README.jp.md) している。
proof02 は同一 VM の別 unit、別 target、別 proof directory で開始し、
CPUWeight=100を事前設定した。[起動設定の根拠](../../cloud/vm11/deployment-correction02.jp.md)
を参照する。proof01 の build や標本を proof02 に継ぎ足していない。

原 archive は **32,221,395 bytes**、SHA-256
`27f772cdf4e9c8293f470d0e8b09df2a03f8b8e3fa873444bc1ee4d817005b84`。
[archive-check.json](archive-check.json) は 2,113 original alias、999 content member、
欠落0・retention issue 0を示す。archive 内にはさらに `recovery-manifest.json` が
あり、regular member の総数は1,000。原 manifest と SHA sidecar も同じ directory に保持する。

[report-command.json](report-command.json) はローカル trusted `report.py` の
exit 0 を示す。この helper は保存された判定文を信用せず `runner.check` を再実行する。
`report.json` は completed / payload_integrity verified を記録し、plan・build・result
のSHA-256を結び付ける。日本語レポートは、その入力hashを再照合したうえで、原resultから
中央値、比、反復数、品質、成果物bytes、I/O判定を別計算して作成した。
再検査時は archive を空 directory に展開し、trusted checkout の
`verify.py --out <展開先> --expect completed` を使う。
