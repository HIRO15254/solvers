# Windowsの実中断・再開（2026-09-26 JST）

**20反復で協調停止し、980反復を追加して1,000へ到達した結果が、直行1,000反復と一致した。**
中断childの終了コードは130。最終checkpointの全bytesと全decoded state、Full SOLの全field
（`wall_secs`のみ除外）、保存後profileのEV/BR/gain/NashConvを照合した。
これは[固定River対照](../README.md)の正しさの証拠であり、性能や外部品質の認定ではない。

## 実行と結果

sourceは`c87c5a5d7119f11cecd17d5e901747c600beb914`に、HU取消のexit code修正、
既存Unix ignored test更新、研究用`hu_checkpoint_audit`を加えたもの。
build前の199ファイルのhash一覧、dirty差分と変更sourceの原bytesを保持する。
Windows 11、Rust 1.97、Python 3.13.7、debug情報なしのdev build、build jobs=1。
`.cargo/config.toml`のnative指定は空の`RUSTFLAGS`で上書きした。正確な環境とbinary hashは保持記録を参照する。

対照は2026-09-25 22:30:44〜22:31:58 UTCに実行した。
最初に10反復checkpointを観測し、監視器が専用consoleへ実Ctrl-Breakを1回送信。
HUは次のcheckpoint境界の20反復で取消・保存し、childと監視器がともに130で終了した。
元の中断成果物は後続工程の前後で全ファイルが不変だった。
再開は別出力先で実行し、進捗の元prefixと新たな反復を検証した。全6工程で子processを回収した。

再開と直行の保存済み量子化profileを、それぞれ実ファイルから独立に読み込んで再評価した。
両者の値は完全一致（OOP / IP、chips）:

| 指標 | OOP | IP |
|---|---:|---:|
| EV | 1.2295895947350397 | 2.768844816419813 |
| BR | 1.2296492523617215 | 2.768901242150201 |
| gain | 0.000059657626681808296 | 0.00005642573038766585 |

NashConvは`0.00011608335706947415`。rakeを含む一般和ケースなので零和のExploitabilityへ換算しない。
保存前のlive metadataと保存後の量子化profileの値は別に保持している。
経過秒数、manifestの実行ID・時刻・path、event時刻を対照間の同値条件から除外するが、
各実行内部の整合と実行・成果物の識別には使用する。

## 検証と資源停止

| 検査 | 結果 |
|---|---|
| `cargo fmt --all --check` | 成功 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 成功（locked/offline） |
| `cargo test -p cli --example hu_checkpoint_audit` | 2 passed |
| controllerの合成test | 5 passed |
| `python -m unittest discover -s tools/tests -v` | 39 passed |
| `cargo test --workspace -- --test-threads=1` | コンパイル中の資源停止、test本体未実行 |

初回の対象buildは209.67秒で空きRAMの下限30億byteに達して停止した。
空きが回復した後、同じ600 MiB/30億byte/600秒の枠で1回だけincremental再実行し、36.28秒で成功した。
通常検証後に対象buildを再確認して、6.67秒で成功。直後の3binary識別子を記録し、対照の前後で固定した。
全workspace testは137.90秒、観測tree working set `631,672,832` byteで600 MiBを超えた。
両資源停止ともCtrl-Breakによる終了と空のJobを確認し、強制終了は不要だった。
コンパイラ診断による失敗ではなく、全workspaceの成功証拠には含めない。
Unix専用の実SIGINT ignored testはこのWindows hostでは未実行。

build/checkのRAM値はprocess treeのsampled working setであり、hard allocation capや真のRSS peakではない。
対照は512 MiB、空きRAM下限30億byte、disk下限4 GiBの枠を使用した。
途中停止を資源超過の代用にしないよう、全raw sampleも検査した。

## 保持と再検査

`manifest.json`は原pathから保持相対pathへの対応、SHA-256、size、gzip圧縮前後の識別を記録する。
成功・失敗build、通常検証、全6工程の生ログと資源sample、3 runの全成果物、比較器出力、
2つの保存後監査、source/toolchain/binary識別を保持する。実行binary自体はGitへ保存せず、
記録されたsource・toolchain・build手順から再buildする。

```text
python experiments/hu-postflop-r1/checkpoint/evidence-20260926/verify.py
```

この検査は保持bytes・記録・checkpoint同値・保存後数値の再照合で、solverやRust testの再実行ではない。
実中断の対照はWindowsで再実行済み。通常workspaceの検証範囲はpartial。
I16、Turn/Flop、複数worker、外部24case、性能改善、R1総合受入は本結果の対象外。

同じ`3d36aa8` sourceの後続[Windows workspace実行](../../validation/windows-workspace-20260926/README.md)
では898 passed・30 ignoredで完了した。本証拠内の先行資源停止記録はそのまま保持する。
