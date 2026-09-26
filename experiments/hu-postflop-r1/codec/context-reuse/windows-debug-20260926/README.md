# Compression context reuse: Windows debug 診断

候補patchの18標本で、元SOLとrewriteの全bytes、canonicalとroot canonicalの全bytesが一致した。
3ケース×3組のwriter時間中央値は下表の通り。これは**他実験も動く共有hostでの探索的診断**であり、
専用hostの性能認定、Linux release比較、旧Flop悪化`+21.39%`の原因認定、R1受入には使わない。

| ケース | baseline中央値 ms | candidate中央値 ms | 観測差 |
|---|---:|---:|---:|
| River | 4.5621 | 4.1719 | −8.55% |
| Turn | 40.2415 | 36.3692 | −9.62% |
| Flop | 557.5716 | 490.3901 | −12.05% |

各ケースでbaseline→candidate、candidate→baseline、baseline→candidateの順に3組を実行し、
全9組でcandidateのwriter時間が短かった。warmup除外や統計的な効果保証はない。
元の3標本ずつとprocess peakは[verification.json](verification.json)に残す。
sampled process-tree peak RSSの中央値は、River 14,721,024→17,412,096 bytes、
Turn 24,584,192→27,136,000 bytes、Flop 257,024,000→259,747,840 bytes。
これは**canonical/result出力・読戻しを含むprocess全体のsampled peak**であり、writer区間のメモリではない。
この観測でメモリ削減は認定しない。

## sourceと失敗の区別

baselineはGit `2fecc099b9911511a0938fb2700bbb124bc1046e` の197ファイル。
candidateはそのうち`crates/formats/src/sol_indexed.rs`だけを変更した研究用copy。
両者を別々の新規Cargo targetへWindows debug設定でbuildし、凍結binaryを実行した。
Cargo jobs / Rayon / test threadsは各1、incrementalとdebug情報は無効。
計測区間は2026-09-26 00:31:33–00:36:07 UTC。

- 最初の共有target試行は**無効**。baselineへ旧instrumented executableをコピーしており、
  sourceと対応しない。続くcandidateはCargo 0でも監視が失敗し、test/sample前に中止。
  元runner・plan・result・2件の監視記録・誤ったbaseline binaryを`invalid-shared/`に保持する。
- fresh実行も初回build 2件とformats test 1件は、Cargo 0の後にJob内の子processが残り、
  `descendants_after_root_exit` / supervisor 1となった。AttachConsole WinError 6と強制cleanupを
  失敗のまま保持する。事前planが限定して認める同一commandの再確認を各1回行い、
  sourceとexample binary不変・通常supervisor 0を確認した。
- formats testは初回と再確認のそれぞれで**77 passed / 0 failed**（58+6+13、doc test 0）。
  異なる154テストの成功とは数えない。追加したcontext境界・連続frameの2テストも成功。
- freshの全24 stage（初回3件、再確認3件、sample18件）のraw監視記録を保持する。
  全18 sampleでは、その役割の凍結binaryと固定入力が実行前後で一致し、正常終了した。

baseline binary SHA-256は`3a96b3e7bdf2bddc87949059616081e848234bfd506c523a91b18deedd3082a1`、
candidateは`a72382f635a68591781626a93b1dac252bad0561b50ff685aa630fe1e805d78f`。
両方の元bytesを保存し、古いinstrumented binaryとは区別する。

## 保持先・再検査

[manifest.json](manifest.json)は194元pathを123 gzip blobsへ対応付ける。
plans/scripts、全stageのsupervisor・stdout/stderr・raw samples、18 results、3入力、
rewrite/canonical/root、binary、source manifest/pins/patch/候補変更ファイルを保持する。
同一bytesを実際に比較したpayloadのみblobを共用し、圧縮前後のbyte数とSHA-256を固定する。
Cargo target全体や197ファイルの重複archiveは保存していない。

```text
python experiments/hu-postflop-r1/codec/context-reuse/windows-debug-20260926/verify.py
```

[verify.py](verify.py)は保持bytesを再hashし、ローカルGitの上記base revisionから197ファイルを読み、
候補の変更ファイルを重ねてbefore/after pinsを照合する。元Eドライブやネットワークは不要だが、
**そのGit履歴がローカルに必要**。全18記録の`identity_before`と`identity_after`で、
argv[0]を`result.binaries[role].frozen`へ、入力を`plan.inputs[case]`へ厳密に結び付ける。
失敗・再確認、時系列、test数、raw sample・cleanup、全成果物一致、中央値も再計算する。
compiler実体は元記録のidentityのみ保持し、再buildやtest、solverは実行しない。

元scratchは`E:\codex-work\solvers\r1-context-reuse-20260926`。
保持時には全payloadを読み出せた。Git commit `96c0e55`のgzipを直接読み、
`fresh/samples/`の全72ファイル / 730,561,025 bytesと原byte単位で再照合してから、
同scratchを削除した。再利用時の根拠は本ディレクトリのgzipとGit履歴とする。
元records/source/frozen binariesと、近接するfresh Cargo targetは今回の保持・検査では削除していない。
旧共有Cargo target `E:\codex-work\solvers\target\r1-writer-smoke-20260926` は、保全後に
root担当が801ファイル / 220,998,130 bytesを削除済みと報告した。元記録中のそのcompiled pathは
過去の測定位置であり、現在の可用性を表さない。誤った凍結baseline executableのbytesは本証拠に残る。
[retained-paths.txt](retained-paths.txt)はGit追跡確認用の全保持path一覧。

元SOLのcached品質値はこの試験で再計算していない。性能の一般化、production採用、
同等exploitabilityの総合判定は、この診断だけでは認定しない。
