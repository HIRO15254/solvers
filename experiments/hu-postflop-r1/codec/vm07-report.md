# VM07 codec byte-serde measurement

既存 source06 の Full River / Turn / Flop SOL を同じ入力として、**metadata・全 raw strategy/value byte と再保存ファイルの完全一致**を確認した。96 回の実行、8 段階のビルド検証、移設後の 1,319 必須ファイル参照の検証は PASS。速度の事前判定は **12 項目中 11 項目**で成立した。**Flop の保存時間は中央値 303.004 ms → 367.828 ms（1.214 倍）で判定不成立**であり、保存全般の高速化とは結論しない。

これは codec の小規模計測であり、solver 全体の速度、保存戦略の BR、外部参照の品質、R1 全体の合格を示さない。

## 固定条件と再現性

- baseline: Git `88ffa5dd4583e5c8bae84e20e9b8390cb719e4f0`。同じ研究 example だけを追加。
- candidate: source07 archive `a6d346a033cf90f68adf131c7a14f5a754417cf0d952e1008d5736e7e9d6dd2a`。source06 は入力ファイルの生成元であり、計測 binary の source と区別する。
- CPU: AMD EPYC 7B12、Linux x86_64、8 logical CPUs、同じ boot `0727ebb3-36dd-4fae-b978-c629114703ed`。Rust 1.97.0 release、`target-cpu=native`、別々の fresh target。
- ビルド終了: 2026-09-25 20:08:45.959563 UTC。plan 発行: 20:18:35.227662 UTC。実行: 20:18:35.760248–20:19:05.697731 UTC。
- plan digest: `afa17b234bcf2a29588d3f4cf221147b055ffb8ebe2c57b0fc3e2298b7b667e5`。runner SHA: `6c8ca8e49f6c20320d4140a8b1322e0c62b3d37de1b26968e4270970d6eba79f`。
- 各 case / operation / binary に除外 warmup 1 回。その後 3 組を B/C、C/B、B/C の順で実行。計 24 warmup + 72 measured samples。入力は直前 hash により warm cache、cache eviction なし。
- 記述的改善条件は事前に median(candidate)/median(baseline) ≤ 0.95、かつ 3 組中 2 組以上で candidate が速いことと固定した。結果を見た閾値変更・失敗標本の置換・最速値の選択はない。

入力サイズは River 7,609 B、Turn 24,920 B、Flop 176,833 B。[inputs.json](inputs.json) の各 SHA-256 と一致し、両 binary の全再保存出力も同じサイズ・SHA-256・直接 byte 比較で一致した。full decode の通常検証を保ち、partial read は公開 reader API を使用した。

## 結果

時間は 3 回の中央値。`read-root` は open + read、`read-repeat-chunk` は **64 回の合計**で open を除く。`stream-write` は resident payload からの write/sync/atomic persist で、別途計測した事前 load を除く。canonical 出力、hash、保存後 readback は対象時間に含めない。

| 入力 | 操作 | baseline ms | candidate ms | candidate / baseline | 速かった組数 | 事前判定 |
|---|---|---:|---:|---:|---:|---|
| River | decode-all | 0.702480 | 0.549580 | 0.782343 | 3/3 | 成立 |
| River | read-root | 0.679230 | 0.452559 | 0.666282 | 3/3 | 成立 |
| River | read-repeat-chunk ×64 | 23.650459 | 9.725240 | 0.411207 | 3/3 | 成立 |
| River | stream-write | 10.440639 | 8.355909 | 0.800325 | 2/3 | 成立 |
| Turn | decode-all | 20.115440 | 10.855160 | 0.539643 | 3/3 | 成立 |
| Turn | read-root | 3.264469 | 2.218151 | 0.679483 | 3/3 | 成立 |
| Turn | read-repeat-chunk ×64 | 118.180828 | 29.742920 | 0.251673 | 3/3 | 成立 |
| Turn | stream-write | 21.090390 | 11.219230 | 0.531959 | 3/3 | 成立 |
| Flop | decode-all | 354.365324 | 132.866548 | 0.374942 | 3/3 | 成立 |
| Flop | read-root | 2.164871 | 1.142610 | 0.527796 | 3/3 | 成立 |
| Flop | read-repeat-chunk ×64 | 94.088319 | 20.567250 | 0.218595 | 3/3 | 成立 |
| Flop | stream-write | 303.004095 | 367.827635 | 1.213936 | 1/3 | **不成立** |

Flop 保存の各組は `(333.512165, 367.827635)`, `(299.654815, 547.759332)`, `(303.004095, 114.359159)` ms（baseline, candidate）。ばらつきが大きく、この 3 組から悪化の原因や一般的な効果量を推定しない。他の全 raw pairs も [machine-readable report](vm07-report.json) に保持する。

計測開始後、同じVMから002診断証跡のSCP回収を行った。どのsampleと重なったかの同期記録はなく、
I/O競合を排除したhost条件とは認定しない。共有Spot・warm cache・3組という限定条件の記述統計である。
書込みはserdeに加えchunk圧縮・hash・sync・persistを含み、個別の寄与は計測していない。
raw bytes・chunk境界・圧縮手順は同じだが、bulk Vec拡張のallocation/cacheやdisk待ちを原因と断定できない。

監督記録の process memory / wall time は hash・canonical・検証を含む。codec phase のメモリ改善や solver のメモリ削減の証拠として流用しない。入力内の EV / NashConv は source06 の pre-save metadata のままで、本計測では再評価していない。

## 保持と独立検証

[verify-retained.py](verify-retained.py) は元 VM path を変更せず、collector manifest の `original_path` → archive member で byte を解決する。全 payload の hash、plan 自己 digest、元の固定 runner、build-stage / binary / tool / input の参照、metadata・全 canonical・再保存 byte を検査し、12 比率を別に再計算した。source archive と build source manifest は baseline 241 件（許可した example 追加のみ）、candidate 332 件で完全一致した。

4 bundle は計 1,360 included payload。主 codec bundle の容量上限で当初欠けた `flop-stream-write-3-candidate/output/canonical.bin`（115,060,885 B、SHA `88a2db6d80c6ae7747b3ae95f687154fca2102395624a8604dcda54764d13ae8`）は supplement から回収し、他と同じ byte 検査を行った。欠測として補完した値はない。

supplementの終了後systemd照会は3 unitsともinactive/dead、時刻空、RuntimeMax/MemoryMaxがinfinityで、
当初設定した有限制限を確認できていない。LoadStateも未取得であり、unloadedだったとは断定しない。
この照会のResult=success/exit 0を実行成功や外側制限の証明には使わず、保存された内側supervisor記録と区別する。

[evidence-vm07/retention.json](evidence-vm07/retention.json) は plan、attestation、96 回の原 report / supervisor / stdout / stderr / samples、8 build stages、関連 example / runner を 521 compact files（6,191,813 B）として選択し、元 byte のまま保持する。選択時に全検証を再実行した。compact files は Git commit 後に Git-backed となる。約 2 GB の canonical、SOL、binary、source archives とその他の source bytes は **ローカル raw bundle にのみ保持**し、Git に格納されたと主張しない。各必須参照の場所・サイズ・SHA は JSON report と retention に記録する。

再実行方法とローカル保持の要件は [retained-usage.md](retained-usage.md) を参照。検証器の負例テスト 13 件も PASS。現在の verifier は original Linux path の存在を要求せず、必須 byte が失われた場合は合格を返さない。
