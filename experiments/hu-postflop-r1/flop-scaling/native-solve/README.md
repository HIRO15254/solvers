# Flop native bounded correctness probe

`solve.rs` は [narrow](../fixtures/narrow.toml) / [expanded](../fixtures/expanded.toml)
と同じ型付きゲーム設定を使う、短い F32/DCFR 検査用アダプター。
[native preflight](../native-preflight/mapping.json) と同じ range / DSL parser、
board `Qs Jh 2h`、pot 200、stack 900、min_bet 10、各 street の
`replace bet [75] / replace raise [75]`、cap 2、all-in 追加なし、
threshold `None`、iso 無効、preflop aggressor P0、NoRake / ChipEv を指定する。
型付き API の検査であり、CLI TOML normalization の検査ではない。

```text
solve.exe narrow 1 1 NEW_OUTPUT_DIRECTORY
solve.exe narrow 2 1 ANOTHER_NEW_OUTPUT_DIRECTORY
```

引数は case、worker 数、正確な反復数、新規出力ディレクトリ。
worker と反復数はそれぞれ 1 または 2 に制限する。fixture の 100 反復・30 秒制御を
実行するものではなく、指定 N を planned iteration にも揃えた有限 correctness probe。
ParConfig は depth 2 / min_children 12、DCFR は alpha 1.5 / beta 0 /
gamma 3 / pow4_reset true。外側 supervisor に wall time、memory、disk 制限が必要。
事前の tree build 成功だけで solver のメモリ上限内完了を保証しない。

同じアダプターを baseline / flat-chance の一貫した crate dependency set にリンクする。
直接リンクには cards / engine / game / holdem / rayon と transitive dependency の検索先が必要。
外側 receipt が fixture・アダプター・workspace source・明示した直接依存・binary・実行引数を固定する。
推移依存cache全体は固定しておらず、保持物だけによる独立再ビルドは保証しない。
実行証拠は [2反復の照合結果](report.jp.md) と [保持manifest](proof01/manifest.json) に分けて保持する。

構築時には CLI 同様の node_info を作り、構造と root support を確認してから viewer 用文字列を解放する。
Solver の F32 配列は通常どおり確保するが、保存は `state_ref()` の借用から直接行う。
一時的な全状態コピーは作らず、16 KiB の変換 buffer と 16 KiB の出力 buffer を使う。

`state.bin` は次の順の小エンディアン表現。worker 数や時間を含まない。

1. 8 bytes ASCII magic `R1F32S01`。
2. u64 × 8: 完了反復、planned 反復、public nodes、action refs、P0 hand 数、P1 hand 数、regrets 長、strategy_sum 長。
3. P0、P1 順の root global combo ID（u16）。
4. 全 regrets、全 strategy_sum の f32 raw bits（u32）。空配列も合法で、長さを用いた除算や zero-size chunks は行わない。

各配列の有限値を確認し、保存を flush / sync してから品質 API を呼ぶ。
両席の `expected_value` と `best_response_value`、続いて `exploitability` を
公開 API から直接呼び出す（この zero-sum ゲームでは計 7 walks）。
`quality.json` はその戻り値と 16 桁 hexadecimal f64 bits を記録する。
ここで `exploitability` 配列は公開APIのseat別gainで、zero-sum経路では
`[BR0 - EV0, BR1 + EV0]`。別計算したEV1の丸めがあるので第2項を`BR1 - EV1`へ置き換えない。
配列の合計がNashConv、半分が通常の零和Exploitability、単位はchips（10 chips = 1 bb）。
EV は solver 内部 chip utility のままで、starting-share 報告補正は加えない。
root / 全 node CFV は保存しない。外部品質認定・収束閾値・性能改善は主張しない。

同じ case / N 間では `state.bin` と `quality.json` の全 bytes を比較できる。
`invocation.json` / `result.json` は worker 数や時間を含むので単純同一比較の対象外。
全ての品質呼出が正常完了した場合だけ `quality.json` と completed `result.json` を作る。
stdout の phase event は都度 flush する。timeout や失敗時に残る state / JSON の一部は
完了証拠ではなく、外側 process 結果と合わせてそのまま保持する。
