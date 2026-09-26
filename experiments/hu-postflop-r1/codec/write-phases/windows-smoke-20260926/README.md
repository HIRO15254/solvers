# Windows writer 計測のruntime smoke

2026-09-26 UTC、candidate `2fecc099b9911511a0938fb2700bbb124bc1046e` に
研究用計測を適用したWindows debug binaryで、FlopのOFF/ON各1回を実行した。
**writerの実行、byte保存、区間の整合を確認した限定的なsmoke**である。
Linuxの4 fresh release builds / 126 process campaign、計測摂動の校正、性能合格、
既存のFlop保存悪化 `+21.39%` の原因、R1全体の受入は認定しない。

## 元の失敗と後続結果

| stage | 実際の結果 |
|---|---|
| instrumented copy | 正常終了、197 source filesを固定 |
| 初回build | Cargo childは0だが、`descendants_after_root_exit` によりsupervisorは1・`failed`。Ctrl-Break helperの `AttachConsole` がWinError 6で失敗。Job cleanupは完了 |
| build confirmation | 同一commandを既存cacheに対して実行し、child / supervisorとも0。実行前後のbinary SHA-256不変 |
| OFF / ON | 両方ともchild / supervisor 0、source identity不変、cleanup完了 |
| phase validation | 既存`validate.py`で正常終了 |

初回buildの失敗を後続成功で上書きしない。confirmationはfresh release buildではない。
保持binaryは1,891,840 bytes、SHA-256
`8248720a768f2a8a74c92b5bf66c8f698c105d20e62e2fe18a03babec32e9628`。
Rust/Cargo 1.97.0、build jobs 1、debug情報なし、incrementalなしを元planに記録する。

元Flop SOLとOFF/ONのrewriteは全176,833 bytesが一致した。
OFF/ONのcanonical 115,060,885 bytes、root canonical 11,473 bytesも一致する。
両result JSONはtiming以外が完全一致。これは保存profileのBRを再計算した試験ではない。

ONの226 compression groupsについて、13 leafの合計＋未分類1,801,700 nsが
inner total 608,515,300 nsに一致し、parent totalとの差は33,500 ns。
圧縮包絡から内部File時間を除く式、write byte数、各call数、SOL directoryの圧縮byte数も
既存validatorで照合した。単一debug sampleのdurationから原因やoverheadを認定しない。

## 保存とoffline再検査

[manifest.json](manifest.json)は53個の元pathを45 gzip blobsへ対応付ける。
plans、実行scripts、supervisor/log/raw samples、phase/results、SOL/canonical、binaryと
適用script・runtime・protocol・pins・validator・監視器を元bytesで保持する。
同一bytesを確認した空log、SOL、canonical等はblobを共用する。
全blobの圧縮前後byte数・SHA-256を記録し、compiler実体はidentityのみ残す。

197個のinstrumented sourceと変更前3ファイルは単一`source/source-bundle.tar.gz`にまとめた。
tarのframingは保持時に作成したが、各memberの内容は元bytesのまま。
変更前の残194ファイルはinstrumented側と同一なので、元candidateの全197 pinsも照合できる。
元Git archiveのrevision・byte数・hashは`preparation.json`に保持し、
新しいsource bundleを元archiveそのものとは扱わない。
`E:\codex-work\solvers\r1-writer-smoke-20260926`の元出力は削除していない。

```text
python experiments/hu-postflop-r1/codec/write-phases/windows-smoke-20260926/verify.py
```

[verify.py](verify.py)は元EドライブやCargoを使わず、全blob・source pins・実行前後identity・
raw sample・cleanup・失敗と成功の終了値・planの時系列・成果物一致を再検査する。
保持した既存`validate.py`の`validate_phase()`を呼び、SOLのheader/directory、
sample timer、phaseと元validation stdoutの一致も確認する。
[verification.json](verification.json)に再検査結果を保持する。
binary実行、build、test、solverの追加実行は行わない。
