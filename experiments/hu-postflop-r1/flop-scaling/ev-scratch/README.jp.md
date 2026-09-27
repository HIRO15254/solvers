# EV scratch再利用の限定診断

EV combineで毎回確保していたstrategy用のzeroed `Vec<f32>`を既存Scratchから
取得・返却する変更を、[全street Flop fixture](../fixtures/README.md)で検査した。
保存sourceのsolver.rsはSHA-256
`69063b31c3433b2eb4798b2f41015311200301a7ba1887b05bbefdb936d4e81a`。
baselineからのruntime source差分はこの1ファイルだけで、flat-chance候補は含まない。
[差分](proof01/solver.patch)と[50ファイルのsource archive](proof01/source.tar.gz)を保持する。

engine→game→holdemを新規ビルドし、その同じcrate graphへ通常版と割当計測版の
adapterをlinkした。cards・hand_index・外部依存は[既存release proof](../optimized/README.jp.md)
に対応するrelease rlibを再利用した。edition2024、opt-level3、thin LTO、codegen-units1、
target-cpu=native、同じrustcを使用し、debug依存との混在を避けた。

5ビルドと、narrow/expanded ×通常版/計測版 ×1/2workerの8solveはすべて正常終了。
各solveはF32/DCFRの固定2反復で、全stateと公開EV/BR/exploitabilityのquality JSONが
保存済みbaselineとbyte一致した。narrow stateは81,414,344 bytes、expandedは
283,677,926 bytes。CFVの保存、I16、CLIのTOML正規化、workspace全体の検証はこのproofに含めない。
未収束の限定検査であり、NC目標への到達・外部参照品質の認定ではない。

各stageは60秒、512MiB Job commit、Below Normal、sampled RSS 469,762,048 bytes、
host reserve 1.5GiB、disk reserve 1GiBで順番に実行した。上限拡張・自動再試行はなく、
全stageで正常cleanupと入力identity不変を確認した。失敗stageは発生していない。

## 観測した割当

7 quality walks（2EV・2BR・公開exploitability内3walk）のprocess allocator count。
要求bytes合計はalloc + alloc_zeroed + reallocの新size全体であり、live量・RSSではない。

| 入力 / workers | alloc / zeroed / realloc | 要求bytes合計 |
|---|---:|---:|
| narrow / 1 | 175 / 0 / 156 | 92,448 |
| narrow / 2 | 558,214 / 0 / 334,384 | 226,981,116 |
| expanded / 1 | 175 / 0 / 168 | 331,804 |
| expanded / 2 | 553,843 / 0 / 446,000 | 928,363,776 |

計測したquality phaseのzeroed allocationはすべて0回となった。
[変更前のnarrow baseline](../alloc-probe/README.md)は両worker設定とも
220,656 zeroed calls /62,332,704 requested bytesで、要求bytes合計は
1worker 62,421,200、2worker 287,355,012だった。expandedの変更前allocation countは
この系列では取得していない。全phaseでallocatorのnull返却は0回。

2workerのtask分割・背景pool処理によるcountの変動を排除した比較ではない。
atomic計装自体も競合を増やすため、時間短縮率、割当の時間寄与、OS peak、
32worker scalingを主張しない。CFRと品質のbit一致を保ったまま、元のper-node
zeroed allocationを避けられた範囲の証拠として扱う。

## 保持と再検査

[proof01 manifest](proof01/manifest.json) は107個の元rawファイルと2個の実行binary gzip、
計109payload /1,776,946 bytesを列挙する。source archive・plan・build/execution receipts・
全stdout/stderr/resource samplesを含む。8個の元stateは実行時に全bytesを比較し、
保持時にも各原streamのSHAを再計算して、既存の2canonicalへdeduplicateした。
元scratchの削除はこの保持処理では行わない。

後続の[照合付き整理](../../validation/ev-scratch/cleanup.json)で元runとsource copyを削除した。
原内容はこのproof、source archive、既存canonicalから確認でき、[削除後の検査](../../validation/ev-scratch/checks03/receipt.json)も成功した。

[verify.py](verify.py) は保存codeを実行せず、全payload hash、source exact set・唯一の
runtime差分、crate依存の結合、binary展開bytes、13recordのcommand/identity/cleanup、
canonical stream、quality bytes、phase順とcountを検査する。
[checks02](checks02/receipt.json) に原出力とsource前後pinsを保存し、2.919秒・exit0だった。
compiler・再利用依存・新規rlibそのものはpinのみで、hermetic rebuildではない。

```text
python -B experiments/hu-postflop-r1/flop-scaling/ev-scratch/verify.py
```

[run.py](run.py) はprepare/build/runを分離し、自動で次phaseを開始しない。
freshな`runs/`・`target/`子directoryと明示solver SHAをprepareへ渡す。
[checks01](checks01/receipt.json) は依存graph・release flags・資源上限・path境界の
6件の軽量testを保持する。重い再実行は他の実験と資源を共有しない時間に別途行う。
