# Chance出力の可変長bufferを親で保持する研究候補

並列chance childごとの `Vec<f32>` を所有してcollect/dropする経路を、親Scratchから借りた
一つの連続bufferへ直接書く形にする。子ごとの出力長を保ち、安全なslice分割で互いの書込みを
分離し、最後は元のchild順・同じ演算で加算する。CFRとEV/BRのchance分岐だけを変更する。
本体へは未採用で、実用Flopの速度・メモリ改善は未測定。

`mapped_dim` に従って可変長・0長を扱い、合計長のoverflowは明示的に拒否する。
chance depth/min_children、ActionPlan、storage view分割、reach写像、terminal kernel、
交互seat更新とrecordingは維持する。[差分](candidate.patch)と[生成pin](manifest.json)を参照。
source全体をSHA-256で固定しており、変更がある版へ推測で適用しない。

## 限定検証

- 独立sourceレビューで借用の分離、可変長・0長、元順fold、I16 scale viewの維持を確認した。
- [型検査](typecheck01/receipt.json)は全engine sourceのmetadata生成がexit0、約2.05秒。
- [既存integration test](smoke01/receipt.json)は候補のrlibと既存 `parallel.rs` を直接rustcで
  コンパイルし、mapped chanceのF32/I16 2 testsが成功した。1/2/4 workers、outer2/16、depth0/1/2で
  全state・EV/BR・exploitability・記録node値のbitsを直列と照合した。実行原ログも同directoryに保持。
- [0次元を含む追加検査](smoke02/receipt.json)では[研究test断片](zero_chance_tests.rs.in)を
  既存parallel testへ追記し、元版・候補版それぞれでF32/I16 2 testsが成功した。
  各246 nodesの混在・片席全空・両席全空の4形、1/2/4/8/16/32 workers、depth0/1/2を
  各版の直列経路と全state・EV/BR・記録CFVのbitsで照合した。空storageのI16 scaleも含む。
  版をまたぐ別binaryの出力を直接比較したものではない。候補の直列経路は変更していない。

上記は既存cacheの依存rlibを利用したdefault debugの限定検査で、依存4件とsourceのhash、
toolchain、実行command、原logをreceiptに保持する。fresh Cargo build・serde feature・workspace
検証・release性能・NLH native実行の成功を意味しない。cache全体とbinaryは配布証拠として保持して
いないため、再実行にはcompatibleな依存buildが必要。production sourceは変更していない。
manifest内のnullは生成段階で検証を主張しない印であり、実行証拠は個別receiptを参照する。

## 性能上の反証も検査する

全出力を残すpayloadは変更前後とも `O(Σ child_dim)`。全階層でpass間cacheが持続するわけでもない。
変更後は親で一括zero-fillするため、逐次first-touch・NUMA局所性が悪化し得る。
短い隣接行のfalse sharing、大容量bufferをScratchに残すことによるpeak増も測る。
現行でも別のfree bufferを拾う場合があり、毎childが必ずmallocするとは数えない。
[source監査](../source-audit/README.jp.md)に残るfold・task・写像コストを区別した。

採用には同一条件の変更前後でF32/I16の品質・state・停止軌跡・保存内容を照合し、
旧3comboと[新2ケース](../fixtures/README.md)を分けて測る。
1 workerの回帰、CFR/EV/BR時間、総time-to-target、全process memoryを残し、
単にallocationが減ったことを速度・メモリ改善と同一視しない。

## 再生成

```text
python -B experiments/hu-postflop-r1/flop-scaling/flat-chance/prepare.py --source-root . --out .cache/flat-chance-reproduce
rustfmt --edition 2024 --check .cache/flat-chance-reproduce/solver.rs
```

出力先は新規directoryを指定する。`prepare.py` は本体を編集せず、rustfmtで整形した研究sourceと
差分・manifestを生成する。今回のformatter版はmanifestに保持し、再生成したsource hashが
`4a58bfa9bfa384e98e5a92f477f6322d39baff975a3810ceef8533f5b6fabafa` に一致することを確認した。
型検査・smoke用の全engine copyは本体と同じ各ファイルにこの一箇所の差分を置いたもの。
実行commandはreceiptにあるが、machine固有のcacheパスを含むため汎用実験runnerではない。
