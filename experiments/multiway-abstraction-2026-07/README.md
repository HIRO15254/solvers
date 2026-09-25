# Multiway抽象化: 既定値を選んだ理由と限界

状態: **終了した研究の判断記録・historical-only**。2026-07-25のS3 screeningと
2026年8月の運用判断を要約する。現在の契約は[規範仕様](../../docs/multiway-preflop-v1.jp.md)、
現在の作業は[開発状態](../../docs/status.jp.md)に従う。

## 保持する判断

| 条件 | 当時の選択 | 適用範囲 |
|---|---|---|
| 共通の既定 | EHS² percentile、F/T/R = 128、current-street | Tournament 6max/50bbのanchorを基に単一既定を選んだ |
| Tournament 6max/50bb | K128 current-street | 2つのreference内でpoint estimateが最良。正式な全候補順位ではない |
| Cash 6max/100bb | K256 current-street | 明示指定の候補。全caseの既定やcash全域の最適値ではない |
| 6〜9max・他のstack | 外挿を実証していない | 既定値の存在は全対象での品質・最適性の認定を意味しない |

2026年8月に研究を終了し、追加測定を待たない運用判断としてK128を既定へ昇格した。
測定範囲や品質の確実性が増したわけではない。Cashの1 anchorを全体へ外挿するK256既定や、
utilityにより黙って既定を切り替える方式は採らなかった。

## 判断に使った測定

S3は10,000 sweeps、abstraction seed 0、solver seed 1011。評価はseed 424242、
8,192 samples、seatごと1,600,000 deviator traversals。limpなしの指定canonical Treeを使った。

- Tournament K128とCash K256はstrict candidate coverage（全体0.995、各street 0.95）を通過し、
  10,000 sweepsを完走した。EHS² K512とrollout K512/R4096の各reference内でpoint winnerだった。
- Tournament rollout K128はrollout referenceでinferior。Cash rollout K256との差は両referenceで
  統計的に未解決であり、EHS²の全般的な優越は主張しない。Cash rolloutのriver coverageは
  `1369 / 1446 = 0.9467496542185339`で0.95 gateを下回った。
- EHS² K64 full-recallはTournament 3,695、Cash 4,267 sweepsでsolverのinternal capへ到達し、
  10,000 sweepsを完走しなかった。current-streetの代替fallbackが検証されたわけではない。
- rollout assignment cacheとfull-recall sparse policyの実行中の成長は、productionの事前確保方針に
  合わなかった。現在の受理/拒否条件と保存形式はこの記録ではなく規範仕様で確認する。

## 認定していないこと

実行済みは予定した3 seed pairsのうち1 pair。2 referenceは別々のproper-subset filterで、
統合されたunfiltered rankingはなく、判定は `screening_only`。rollout referenceにE64 controlはない。
Cash comparatorのcoverage不足、cold cache build時間とresume開始sweepの欠測も残った。
fixed-ten transferと6〜9max/全stackへの一般化は未実行。card-onlyのuniform dealsによる誤差を、
実戦略のreach・range・position・ICM/rakeを含むsolve品質へ読み替えない。
収束済みexploitability、global optimum、全設定の資源安全性を示す結果ではない。

## 原記録と保存状態

当時のconfig・CSV・raw artifactはこのディレクトリには存在しない。再実行可能な現行資産として扱わない。
2026-09-25の要約前に参照した詳細報告は、repositoryのcommit
`93c95533dbaca2e8388e82235af5519071fd880f` の
`docs/validation/multiway-abstraction-optimization-2026-07-25.md`
（Git blob `972cf0b3e53eacdf448c515738d1a1d854b0317f`）に残る。
これは詳細報告の保管位置であり、そこで言及する全入力の存在や再実行成功を保証しない。
