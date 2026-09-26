# CFR action taskの最小要素数を2にする研究候補

現行solverの **CFR action child iterator一箇所**に `.with_min_len(2)` を加える。
短命scratchを1 childだけのjobで初期化する機会を減らす候補であり、
ビルド・同品質検証・性能実測は未実施、本体へ未採用である。
[VM12の再解析](../scaling-next-audit/README.md)でRiverの減速の大半がCFR側に残るため、
EV/BR側のiterator、chance分割、fork選択、kernelはこの候補に含めない。

Rayon 1.12の[with_min_len](https://docs.rs/rayon/1.12.0/rayon/iter/trait.IndexedParallelIterator.html#method.with_min_len)
はjobへ分割する最小要素数を指定し、[for_each_init](https://docs.rs/rayon/1.12.0/rayon/iter/trait.ParallelIterator.html#method.for_each_init)
はjobで必要な初期値を作る。これは常に隣接2本ずつの固定chunkにするAPIではない。
2〜3本のforkでは子同士の並列分割を失い得る。scratchの寿命は依然job内だけで、永続cacheや
workerへの固定割当は導入しない。重いchildが同じjobに集まり、負荷分散が悪化する可能性がある。
16→32の原因がallocatorであるとも、この候補で32の方が速くなるとも断定しない。

各childのindex、disjointな出力とstorage view、末尾のchild順加算を維持する。
1 worker・非fork経路には到達しない。現行のaction planはchanceを含むrootでは
作られないため、Turn/Flopのchance経路への改善は主張しない。
不均等child、奇数child、2-child fork、深い単一child祖先、非対称次元、F32/I16、
1/2/4/8/16/32 workerの全state/EV/BR/CFV照合が採用前に必要である。
独立sourceレビューでは、出力行・action index・action順加算とscratch再利用時の
ゼロ初期化を維持していることを確認した。これは実行時の一致検証を代替しない。

## 再生成

```text
python -B experiments/hu-postflop-r1/action-minlen2/prepare.py --source-root . --out .cache/action-minlen2-review
rustfmt --edition 2024 --check .cache/action-minlen2-review/solver.rs
```

`prepare.py` は現行 `crates/engine/src/solver.rs` の全bytesをSHA-256で固定し、
新規directoryに研究用の1ファイル、差分、manifestだけを生成する。
sourceが変わった場合や出力先が既存の場合は拒否する。本体へpatchを適用しない。
生成した[差分](candidate.patch)と[hash](manifest.json)、[rustfmt照合記録](review.json)を保持する。
型検査・solver実行は[ローカルの競合計算とcommit余力](../cloud/host-probe-20260926-2332.json)
および未精算のクラウド予約のため行っていない。
配布用の全sourceコピーを作る際には、このpinに加えて全workspace source、Cargo.lock、
toolchain、configを固定し、新しいtargetで検証する。rustfmt成功はRustの型検査ではない。

## 最小の比較

先に通常fmt/clippy/workspace testsと既存parallel/oracle/storageの選定release検証を行う。
次に32 logical CPUの同一bootで、変更前・後を1/2/16/32 workers、VM12 River入力、
同じquality停止条件で比較する。各条件1 warmup＋3測定、変更前後の順を交互にする。
全state・保存strategy/CFV・EV/BR/NashConvのbitsと停止軌跡が一致しない条件は棄却する。
CFR時間とquality時間を別集計し、元の総solver時間も残す。別armのallocation計数を
性能時系列へ混ぜず、割合は参考値にする。32が16を上回るかとは別に、変更前後の
同一worker比較を行い、1/2 workerの回帰も公開する。

ここでは未実行の実験案までを固定した。起動前には有限runner、資源・予算、停止期限と
性能採否の閾値を別protocolで確定する。既存VM12の結果へ新標本を継ぎ足さない。
