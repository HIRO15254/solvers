# Invocation-owned ScratchBank 候補

本体の `solver.rs`（SHA256 `69063b31c3433b2eb4798b2f41015311200301a7ba1887b05bbefdb936d4e81a`）から作った研究用コピー。flat-chance は混ぜていない。本体への採用、コンパイル、Rust テスト、品質・速度・メモリの実測は、この準備証拠では未実施。

変更は並列 walk が使う Scratch の寿命。`Solver::run` は全 iteration と両 seat pass に一つの bank を共有し、単独 `step` は呼出しごとに作る。EV/BR は既存 `ev_pass` / `br_pass` の入口、選択 CFV は `expected_values_where` に bank を作る。各呼出し終了時に bank を破棄する。既存 `ValueCtx` と関数署名を維持するため、MCCFR の exact EV/BR も同じ入口を通る。MCCFR の標本走査や RNG は変更しない。

五つの Rayon factory（CFR chance / action、value chance / hero action / opponent action）が Scratch を所有する lease を借りる。worker index はキャッシュの選択にだけ使う。checkout/checkin は `try_lock` で短時間だけ触り、walk 中には MutexGuard、RefCell の借用、共有可変 Scratch を保持しない。空きがなければ新しい Scratch を所有して進む。再入した lease 同士は独立し、先に戻った一つだけを idle bin に保持する。競合・pool 外・範囲外 index・poison は待機せずキャッシュを使わない。panic の unwind 中は lease を捨て、元の panic を隠す新しい panic を起こさない。

idle は worker-index bin ごとに最大一つ。ただし **active lease 数や保持バイト数の上限ではない**。nested work の in-flight buffer と、各 Vec の過去の capacity は残り得る。bank は invocation 終了時に解放されるが、長い `run` の途中で idle capacity が増える可能性はあり、ピークメモリは別途比較が必要。ゲームをまたぐ global TLS や persistent pool は使わない。

1 worker では空の bank として bin 自体を確保しない。`run(0)` は従来通り入口の早期 return で bank 作成前に終了する。

`Scratch::take` が clear/zero-resize する既存処理はそのまま使う。ReachMap による可変・ゼロ次元、I16 の storage view、演算順序、chance の ordered collect と元 child 順 fold、EV の平均戦略合成、CFV の記録条件を変更しない。chance の child output は従来通り Scratch から持ち出して最後に drop するため、その確保や storage view 自体の確保は残る。allocation がなくなるという提案ではない。

Rayon 1.12 の [`map_init` 契約](https://docs.rs/rayon/1.12.0/rayon/iter/trait.ParallelIterator.html#method.map_init)は worker ごと一回の初期化を約束せず、job 内の項目群に必要に応じて初期化する。[worker index](https://docs.rs/rayon/1.12.0/rayon/fn.current_thread_index.html)は別 pool と重複し得る。[`install`](https://docs.rs/rayon/1.12.0/rayon/struct.ThreadPool.html#method.install) と [`join`](https://docs.rs/rayon/1.12.0/rayon/fn.join.html) は待機中の他 work 実行を説明しており、index だけを独占所有の根拠にはしていない。

`prepare.py` は元 solver の byte pin を確認し、限定した置換だけを行い、整形前の逆変換が原文に完全一致することを検査する。rustfmt 後の候補・差分・preparer の pin は `provenance.json` に保持する。準備は次で再現可能。

```text
python -B experiments/hu-postflop-r1/flop-scaling/worker-scratch/prepare.py --check
```

初回生成は `--check` を省く。既存候補の上書きを拒否する。実行する外部プログラムは rustfmt の version/read-from-stdin 整形のみで、Cargo/rustc/solver を起動しない。初回生成の静的逆変換と整形は成功した。`--check` は保存内容を比較するだけで、Rust の型検査の代わりにはならない。

候補内の追加 Rust テストは七つ。再入 lease の非aliasと一つだけの返却、再利用 Vec のゼロ・縮小・拡大次元、競合/不正 index の非待機 fallback、panic 伝播と poison fallback、bank の所有権終了、F32/I16 それぞれの nested chance 上で `run(3)` と `step` 三回および 1/2/4 worker の state・値一致を検査する。uniform EV=2 / BR=3 の解析値も確認する。既存 parallel/value_scratch の F32/I16・mapped/zero・CFV/read-only 検査を併用する必要がある。アロケータの解放を測るテストではなく、bank に global owner がなく Rust の通常の field drop が行われることを確認するテストを含む。

クラウド比較は[事前固定した条件](protocol.jp.md)と[VM15の有限資源枠](../../cloud/vm15/README.md)に従う。
[保存処理](durable.jp.md)は測定終了後に作用し、各stageと完了caseを永続化する。
全64条件の判定は`cloud32/analyze.py`、一つの完了caseの参考値は`cloud32/case_analyze.py`で照合する。
後者は更新中のexecution/retained/wrapperを読まず、必要なsource・control・build・pilot・32条件とcanonicalを確認する。
部分caseから全体の採用guardを通過させない。実stateの伸長・照合はGCP側で行う。

[VM15比較結果](../../cloud/vm15/report.jp.md)で候補を含む138 tests成功、全64条件のstate/quality一致を確認した。ただし16 workersで必要な10%以上の短縮を満たさず、この候補は採用しない。固定反復の比較であり、収束目標や外部参照品質の合格を意味しない。
