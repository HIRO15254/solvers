# EV scratch修正上のflat chance候補

現行EV scratch修正を保持し、既存[flat-chance変換](../flat-chance/prepare.py)を
研究コピーへ適用した候補。[solver.rs](solver.rs)は未採用。
[native検査](native/README.jp.md)の2入力・通常/計測版・1/2 workersの全8条件で、
全F32状態と公開品質が基準とbyte一致した。2 workersでは要求allocation bytesが減少した。
[汎用境界の検査](generic-checks/README.jp.md)も8件成功した。
これらは時間・RSS・32 workers性能の改善を認定するものではなく、以前のflat単独の
結果を、この組合せの実行証拠として扱わない。

[prepare.py](prepare.py)はproduction sourceと旧generatorのSHA-256を検査した後、
generatorの`BASE_SHA256`代入1個だけをメモリ内で差し替えて既存`transform`を呼ぶ。
chanceのアルゴリズムを別実装へ複製していない。旧generatorとproductionは書き換えない。
[provenance.json](provenance.json)に両入力、メモリ内generator、未整形/整形済み候補、
preparer、formatter versionと[差分](candidate.patch)のpinを保持する。

差分は可変長出力sliceを作るhelperと、CFR/valueの並列chance枝だけ。
元の2枝へ戻してhelperを除去すると、入力sourceとbyte一致することを検査する。
両EV combineのscratch再利用、BR、CFV recorder、並列深度・閾値、storage分割、
逐次経路はその範囲外で不変。出力はchildごとのmapped dimensionを加算検査付きで
連結し、0長のchildも実行する。全child完了後に元順で加算し、flat bufferを親の
Scratchへ返す。子taskのScratch寿命は変更しない。

```text
python -B experiments/hu-postflop-r1/flop-scaling/flat-ev/prepare.py
python -B experiments/hu-postflop-r1/flop-scaling/flat-ev/prepare.py --check
python -B experiments/hu-postflop-r1/flop-scaling/flat-ev/test_prepare.py
```

初回生成は既存の候補出力を上書きしない。保持済みcheckoutでは`--check`を使う。
source pinが変わった場合は明示的な別のrebaseが必要。Python検査は入力pin拒否、
変更範囲、EV combine改変の検出だけで、Rustの型検査や0次元実行試験ではない。
軽量実行記録は[checks.json](checks.json)に保持する。

`candidate.patch`の空context行はunified diffのprefix空白を含むため、追加ファイル全体への
`git diff --cached --check`はその1行を報告する。patchの元bytesは保持し、patch以外の同検査と
候補のrustfmt・再生成照合は成功した。Rust sourceの末尾空白エラーではない。

無計装時間の比較は[事前固定手順](timing/protocol.md)に従う。
親側でのゼロ初期化、容量保持、
追加の寸法/参照vectorにより時間やpeakが悪化する可能性もあり、確保削減や
32worker scalingをこのsource準備から認定しない。
