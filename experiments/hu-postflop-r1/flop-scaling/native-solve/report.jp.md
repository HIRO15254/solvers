# 全street Flopの短いnative solve照合

現行版とflat chance出力候補を、それぞれ1／2 workersで2反復実行した。
4条件すべてで全F32状態81,414,344 bytesと、公開APIから得たEV・BR・seat別gainの
JSON全bytesが一致した。FlopからRiverまでbet／raiseを持つ木でも、今回の候補が
この短い実行の計算結果を変えないことを確認した。

これは未収束の正しさ検査であり、速度改善、同等品質までの所要時間、32 workersの
並列効率、R1受入を認定するものではない。productionへの候補適用も行っていない。

## 入力と実行境界

[narrow入力](../fixtures/narrow.toml) と同じ型付きAPIを
[solve.rs](solve.rs) で構築した。boardはQs Jh 2h、pot 200、stack 900、min_bet 10。
全streetに75% bet／raise、各cap 2、all-in追加なし、iso無効、NoRake／ChipEv。
開始supportは34／30 hands、正規化joint massは870、public nodesは367,662。
CLI TOML normalizationの検査ではない。

F32／DCFR、planned iterationsも2、alpha 1.5／beta 0／gamma 3／pow4_reset true。
ParConfigはdepth 2／min_children 12に固定した。同じ木と同じsolver条件で
baseline-1、baseline-2、flat-1、flat-2を順に実行した。

baselineは現行のcards／engine／game／hand-index／holdemを
[native preflight](../native-preflight/README.jp.md) で直接コンパイルした組合せ。
flat側は同じcards／hand-indexと外部dependencyを使い、候補engineに対して
game／holdemも再コンパイルした。外部dependencyは既存cacheを使用しており、
workspace全体のfresh Cargo buildやall-features検査ではない。

保持時のHEADは`bab8493cf074c9acaff80f308a87985328316d99`。
production sourceは`11e4062ba1735e58b60d12999cb23ed10fd1a163`以降不変で、
比較に使う207ファイルのpinを別途確認する。flat側の`solver.rs`は58,650 bytes、
SHA-256 `4a58bfa9bfa384e98e5a92f477f6322d39baff975a3810ceef8533f5b6fabafa`。
[build receipt](proof01/flat-build/receipt.json) と8ファイルのsource archiveで固定した。
compiler本体と明示した直接依存rlibのhashを記録しているが、このproofには同梱していない。
推移依存cache全体のhash固定は行っていない。
保持内容と実行の対応は検査できるが、このarchiveだけによる独立再ビルドを保証しない。

## 状態と品質の一致

`state.bin`は反復数・node数・配列長・root global combo IDを含むheaderに続き、
全regretsと全strategy_sumをraw F32 bitsで保存する。各配列10,176,768要素。
writerは借用配列を小bufferで逐次出力し、全値が有限であることを確認した。
品質APIを呼ぶ前に保存をflush／syncした。CFVは保存していない。

4条件すべての元stateを直接byte比較し、各ファイルのSHA-256も照合した。
共通hashは`35981691a7736e2df5a89734b1102ee8ffc1bacaab7719476b0152e459eb2700`。
重複を除き、[共通state](proof01/shared-state.bin.gz) をgzipで1つ保持する。
manifestには4つの元ファイルそれぞれのサイズ・hashも残した。

| 公開APIの値（chips） | P0 | P1 |
|---|---:|---:|
| expected_value | -14.63106391292879 | 14.63104995201374 |
| best_response_value | 316.8959826239224 | 157.72235278864017 |
| exploitability配列のseat別gain | 331.5270465368512 | 143.09128887571137 |

これらは4条件でf64 bitsまで一致する。zero-sum経路のgainは
`[BR0 - EV0, BR1 + EV0]`であり、独立に評価したEV1との丸め差を変換で消していない。
EVはsolver内部のchip utilityで、starting-shareの報告補正なし。
gain合計のNashConvは474.61833541256254 chips、半分の通常Exploitabilityは
237.30916770628127 chips。十分な解に到達したという判定はしていない。

## 実行資源と証拠

Windowsのdebug buildで、別のローカル計算と競合する環境の単発検査だった。
各processはJob全体のcommit上限512 MiB、rootのbelow-normal priority、wall上限60秒、
開始前のhost available commit下限1.5 GiBを設定した。4条件とも正常終了し、
supervisorのidentity照合・子process清掃も成功した。
[wrapperと校正](../native-preflight/README.jp.md) の境界を引き継いでいる。

元の時間・OS peak working set・Job peakは各recordにそのまま保持する。
各設定1実行、debug、競合負荷ありで、正式性能protocolも適用していないため、
これらからspeedup、メモリ削減率、候補の採否を判断しない。

[manifest](proof01/manifest.json) は75 payloadを固定する。
4条件のraw stdout／stderr／samples／supervisor record／invocation／result／quality、
baselineとflatのbuild receipt・ログ、両binaryのgzip、候補source archive、共通stateを含む。
再検証は次のコマンドで行う。

```text
python -B experiments/hu-postflop-r1/flop-scaling/native-solve/verify.py
```

保持payload・圧縮内容・state header／combo ID・品質bits・source／binary／実行の対応を
読み取り専用で検査する。検証ログは[checks01](checks01/)に保持する。

expanded入力のsolve、長い収束軌跡、I16でのこのnative入力、CFV、SOL／checkpoint保存、
独立した外部品質照合はこの実験の範囲外。候補採用には通常のworkspace
fmt／clippy／testsと必要なoracle／storage／parallel検査、および実測前に固定した
性能protocolに沿う比較が必要になる。
