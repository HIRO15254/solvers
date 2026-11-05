# Ghostline（E05）

| profile | near-uniform | (a) own低 | (b) opponent低 | (c) 両方到達 |
|---|---:|---:|---:|---:|
| it000000 | 340,197 | 0 | 0 | 340,197 |
| it001000 | 217,448 | 205,352 | 5,169 | 6,927 |
| it002000 | 217,293 | 213,936 | 1,221 | 2,136 |
| it003000 | 217,243 | 214,499 | 1,061 | 1,683 |
| it004000 | 217,200 | 214,710 | 1,030 | 1,460 |
| it005000 | 217,188 | 214,908 | 1,059 | 1,221 |
| it006000 | 217,170 | 215,002 | 1,129 | 1,039 |
| final | 217,170 | 215,002 | 1,129 | 1,039 |

最終profileのnear-uniform内訳: (a) 99.00%、(b) 0.52%、(c) 0.48%。
閾値0.005: 216,550 cells、厳密一様: 215,780 cells。
ghost-node（相手reach < 1e-6 / 1e-4 / 1e-3）: 882 / 1319 / 1538。

最終actor-reach mass (a)/(b)/(c): 1.8253638e-06 / 0.53699986 / 0.25883016。
(c) at 1000: 6,927、1000〜6000の全保存点で一様を維持: 6,804、途中で離脱: 123。6000の(c) 1,039中、全保存点で一様: 1,014。
1000の(c)から一様を維持したうち、6000も(c): 962、6000には(a)/(b): 5,842。一様の維持と両方到達の維持は別。
両端で(c)の確率変化（最大L∞）: 0.00196386。[INFERENCE] 持続は未学習候補だが、無差別や小さい累積信号とも区別できない。

GTOW参照 135 nodes中 133 matched（compare.pyの近似raise対応）。
(b)/(c)の到達重み付きshare（Σcombo/1326 × own × opponent、全matched cellsを分母）: (b) 0.000000% / (c) 0.029510%。
該当node（history、cells b/c、node内到達重みshare b/c）:

- R2: 0/1; 0.0000%/0.4525%
- F-R2: 0/1; 0.0000%/0.4525%
- R2-F-F-F: 0/1; 0.0000%/0.4525%
- R2-R6.5-F: 0/1; 0.0000%/0.3017%
- R2-R6.5-F-F-F: 0/1; 0.0000%/0.3017%

## 判定
[INFERENCE] まずE04 TPWLを到達帯別に測定する。最終の大多数は(a)で、到達する(c)は1000から減り、参照nodeでの(c)到達重みも小さいため、広範なon-path未学習が主因という根拠は弱い。持続する(c)にはinstrumentationで信号を確認する。Q01 QRE annealing / Q03 MMDを直ちに選ぶ証拠はない。Q04 zero-reach continuation後埋めは(a)/(b)向けの次候補だが、TPWLで改善価値を先に確認する。今回の閾値は厳密zeroでなく平均方策のproxyなので、そのまま埋める根拠にはならない。

actor-reach massはΣ(combo数/1326 × own reach)、joint massはさらに相手reachを掛ける（nodeを跨ぐ和であり確率ではない）。相手reachはstill-in-hand席のみの独立積。fold済み席の履歴確率・card removalを無視するため、実際のline確率/CFR反実仮想到達ではない。保存平均方策の積は学習時の各反復の到達とも異なる。閾値はstrict <、厳密一様は浮動小数点で1/nと完全一致。
per-cell update counts / positive-regret sumsはsnapshotに無い。未学習・無差別・相手非到達を因果的に判別するにはsolver instrumentationが必要。
wall time（8 profileの解析＋JSON読込、出力前）: 29.41 s。詳細・top20・各時点の参照nodeはsummary.json。
