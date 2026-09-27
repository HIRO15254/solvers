# EV作業配列再利用の回帰検証

`expected_values_where`と`ev_pass`の平均戦略用配列を、各走査taskの`Scratch`から
取得・返却する変更を検証した。公開API、演算順、CFV記録位置、CFR更新式は変更しない。
基点は`efaa535`、対象solver.rsのSHA-256は
`69063b31c3433b2eb4798b2f41015311200301a7ba1887b05bbefdb936d4e81a`。
Git差分、新規test、206ファイルのsource snapshotと各実行前後のpinsを保持する。

| 検査 | 結果 | 監視区間の秒数 |
|---|---|---:|
| Windows Jobの較正 | 小さいcommit成功、上限を越すcommit拒否 | 0.215 |
| `cargo fmt --all --check` | 成功 | 2.834 |
| 新規`value_scratch` test | 2成功 | 5.233 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 成功 | 48.299 |
| `cargo test --workspace` | 956成功・30ignored・失敗0 | 684.952 |
| releaseのparallel / value_scratch | 10成功 | 223.926 |
| releaseのoracle_diff、ignored含む | 3成功 | 30.942 |
| releaseの選択したpostflop ignored tests | 4成功・33filtered | 32.550 |

新規testは手計算した小ゲームのF32/I16を1/2/4 workers・chance depth 0/1で検査する。
0・可変手札次元、未学習の一様戦略、EV/BR、全件/選択/無選択のCFV、storage不変を含む。
非選択の`None`と零次元の`Some(empty)`を区別する。一般和のseat別gainも確認する。
既存parallel testsは学習済み非一様strategyと、chance/actionの混在した走査を補完する。

releaseの追加範囲は、独立oracleとの200反復/初期反復/一様profileの比較、
`flop_solve_is_zero_sum`、`allin_runout_matches_direct_equity`、
`i16_storage_matches_f32_on_small_turn_spot`、`iso_quotient_matches_full_tree_per_hand`。
`cfr-ref`の実装は変更していない。すべてのignored testを実行した結果ではない。

## 実行条件と保持

Windows 11、Rust 1.97.0、offline/locked、Cargo build jobs 1、debug情報なし、
`RUSTFLAGS`空、`RAYON_NUM_THREADS=1`を指定した。test harnessはworkspaceのみ4、他は1。
parallel tests内では専用の複数worker poolを明示的に作る。releaseはCargoのthin LTO /
codegen-units 1を使う。既存Cargo cacheを利用し、変更したcrateと依存先の再buildを原logで確認する。

各processをsuspendedで起動し、1GiBのaggregate Job commitとBelow Normalを設定・照会してから
再開する。開始前のhost available commitは2GiB以上、physical reserve 1.5GiB、disk reserve 1GiB。
sampled working setの停止値は960MiB、wall上限はstageごとに15〜1800秒。
全8stageでidentity不変・正常終了・子process掃除を確認した。
workspace実行のJob peak commitは836,898,816 bytesだった。
較正では拒否された要求もkernelのpeak計上へ反映されるため、そのpeakを上限内と要求しない。
設定の照会と実際の拒否結果を照合する。

上表の時間は再buildとtestを含む監視区間であり、他のローカル計算も稼働していた。
速度比較・隔離性能・process RSS削減の根拠には使わない。
[Flopの全状態一致とallocation診断](../../flop-scaling/ev-scratch/README.jp.md)は別の最適化binaryの証拠。
この変更から32 workersの比例スケールやR1全体の受入は認定しない。

[proof01/manifest.json](proof01/manifest.json)は72 rawファイルとsource archiveの
計73payload、1,389,889 bytesを保持する。[checks01](checks01/receipt.json)は保持処理と
独立再検査の原出力・入力前後pins・command・exitを記録する。保持処理2.149秒、再検査0.436秒で成功した。
`verify.py`は保存sourceを実行せず、source・raw bytes・command・test件数・Job・cleanupを検査する。
外部toolchain binaryとCargo生成物はhash/commandのみで、hermetic rebuildの証明ではない。

```text
python -B experiments/hu-postflop-r1/validation/ev-scratch/verify.py experiments/hu-postflop-r1/validation/ev-scratch/proof01
```

再実行は`check.py`または`extra.py`へstageと新規`runs/`子directoryを渡す。
再試行や上限引上げは自動では行わない。元のsourceとrawが揃う場合だけ`retain.py`で別のproofを作る。

補助検査は[checks02](checks02/README.md)に別保持した。Python 39 testsと文書49ファイルの検査が成功。
Python初回の開始前資源拒否も残し、Rust検証8stageの成功と混同しない。
