# CFR strategy scratch の上書き専用取得候補

未コンパイル・未測定の研究パッチ。本体には採用しておらず、速度向上や
16→32 worker のスケール改善を主張しない。

`Scratch::take` は毎回 `clear` + `resize` により全要素をゼロにする。
CFR の strategy 配列を取得する2か所では、直後の
`StorageOps::regret_matching` が全要素を代入するため、この初期値は使わない。
F32/I16 と各 view は、それぞれの正規化関数の正の合計・uniform の両分岐で
全 action × hand を代入する。空の hand 次元では配列も空になる。

候補は `pub(crate) Scratch::take_for_overwrite` を追加し、その2か所だけを
切り替える。free-list から取り出した初期化済み `Vec<f32>` に `resize` だけを
行う。既存部分の値を保持し、伸長時の新しい末尾だけをゼロ初期化する。
未初期化メモリ・unsafe・MaybeUninit は使わない。呼び出し元は読み取り前に
全要素を上書きしなければならない。

既存の `take`、蓄積用配列、reach map、pool の出し入れ順、演算順序、
並列化、ストレージ、oracle は変更しない。average strategy の3か所も対象外。
flat-chance / flat-EV / worker-scratch と異なり、出力配置や pool の寿命は変えない。
既存候補の否定的結果を覆す根拠にはならない。

ソース上の不要な初期化は確認したが、最適化後の命令に残るか、時間のどの割合か、
メモリ帯域や SMT がスケール不良の原因かは未確認。worker 数によって初期化の
論理量が増えるという主張もしない。

## 再現と検査

`provenance.json` は scratch・solver・未変更 storage の元ファイルと候補、
patch、生成コードの SHA-256 を固定する。元ソースは
`a0baa8bb56d1a11b4f619913518f55c548f6f80a` で確認したもの。
`prepare.py` は2つの call site と追加 method / test appendix を逆変換して
元ファイルとの完全一致を検査し、別箇所への変更を拒否する。

```text
python prepare.py --check
python -m unittest -v test_prepare.py
python prepare.py --apply-to /path/to/fresh/research-source --receipt /path/to/fresh/application.json
```

適用は元ソースの固定値を検査してから別の研究用コピーにだけ行う。
適用後のハッシュと `compiled: false` を receipt に残す。配布時はこのディレクトリの
`prepare.py`、`tests.rs.in`、`candidate.patch`、`provenance.json` を一緒に保持する。
小さな source-only / formatter 検査の実施記録は `source-check.json`。

追加Rustテストは5件。dirty値・NaN payload・負のゼロの保持、同長・縮小・伸長・
空配列、既存 `take` のゼロ化と LIFO、F32/I16 の full / full view / offset付き
split view での dirty-vs-zero 出力bit一致を検査する。ゼロ・負のみ・正混在・
単一正 action の各列と空の hand 次元を含む。これは上書き契約の回帰テストであり、
独立した solver oracle ではない。Rust テスト自体はまだ実行していない。

## 将来の採否条件

将来のGCP実験でまずコンパイル、追加テスト、既存 engine/game と凍結 oracle の
比較を行う。性能比較前に両 backend の full state と quality の同一性を確認する。
その後、同一マシン・同一 build 設定で baseline/candidate を順序交替して測る。
測定条件、反復、ばらつき・時間・RSS のガードは実行前に別途固定する。
16/32 worker の CFR、quality、全体時間を分け、両レンジで比較する。
この提案には実験実行の承認やクラウド runner は含めない。

本体採用前には通常の `cargo fmt --all --check`、
`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`
も必要。現時点の source-only 検査はこれらを代替しない。
