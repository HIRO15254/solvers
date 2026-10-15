# P1 `.sol` v3の索引付きchunkと部分読込み（SOL-14）

2026-10-12。Linear SOL-14（T1-07）の受入の証拠。再現状態は`partial`である。大型の`.sol`とbinaryはignoredの`runs/`にあり、hashだけを残す。

## 問いと結論

`solvers export`は`.sol`全体を展開・逆量子化してから1 nodeを返していた。大きいFlop木では1回の照会に数十秒かかり、peakが数GBになっていた。

| 段階 | 変更 |
|---|---|
| A | v2のstreaming復号、`require_current_artifact`でのconfigだけの読込み、照会nodeだけの逆量子化 |
| B | `.sol` v3: 約1 MiBのchunkとdirectoryを持つ。chunkごとにBLAKE3で検査する。summaryはmetadataだけから作る。v1/v2は再solveを案内して拒否する |
| C | `export --node`を繰り返せるようにし、1回の読込みで複数nodeの行を出す |

採用したのはB＋Cである（Aの読み手はBで置き換えた）。export出力はbaseとbyte一致した。summaryは再solveで変わる`wall_secs`だけが異なる。

## 条件

- Machine: Windows 10.0.26300、16 CPU、31.9 GB RAM（他appと共用）。1回ずつ順に実行した。
- Source: base `8ea7ca33`、A・Bはその上の未commit差分（Bはこの実験と同じcommitで入る）。

| binary | sha256 |
|---|---|
| base（`8ea7ca33`） | `a7b6aac79f6e38e71de94f934ae5ad9a608ef9662b7c5dc8a2dd0a0414024b8d` |
| A | `2df171b9e195a8bcb931e7c1d62e7195178d8812274bfc860f999e8df5b42857` |
| B（最終） | `5745bc3c0956a9a263cd80abf1922dcc2a8907d5e2c037134dd84c05c20eb71f` |

- 対象: `experiments/hu-postflop-reference/fixtures-2026-10/README.md`（local-only、非配布）のc001・c013。
  - v2の`.sol`は、SOL-8のsolve（`runs/sol8/solve/<case>/`）が出したもの。
  - v3の`.sol`は、同じ`run.toml`を`[run] checkpoint_interval = "100h"`に変えてBで再solveしたもの。c013の定期checkpoint（8 GB）を避けるためである。`[run]`は互換性hashの対象外である。

| file | sha256 |
|---|---|
| c001 `run.toml`（SOL-8） | `6c290cf35c681ad08c4623e806d07e784a8f44af7756059860380d3a5b10007c` |
| c013 `run.toml`（SOL-8） | `4c8db6f5337d545db5175d3ba77347f21f8b450013e7e4737bf958357ad11ae1` |
| c001 v3 `solution.sol` | `1fdb754124b4906fd995977bc229ab2cabe0faed2c92b7ded2fe23b645a93050` |
| c013 v3 `solution.sol` | `64dab04e4a4773dd7033c24859519f7e02aadf009c9880e3f7a2e1b26ab9d6be` |

- 計測: [`measure.py`](measure.py)は、`export <view> [--node root] --format json`の壁時計とWindowsのpeak working setを測る。peakは20 ms間隔のpollingで取るため、0.1秒未満で終わるsummaryの値は下限である。
  [`multi.py`](multi.py)は、4 nodeを1回で出す場合と4回に分ける場合を比べる。結合した行が単独exportの連結とbyte一致するかも検査する。
- 生の結果: [`exports.jsonl`](exports.jsonl)（label `base`・`a`・`b`・`b13`）、[`multi-node.jsonl`](multi-node.jsonl)。

## 結果

1回のexport（`--node root`）:

| case/view | base 秒 | base peak GB | A 秒 | A peak GB | v3 秒 | v3 peak GB |
|---|---|---|---|---|---|---|
| c001 summary | 22.41 | 4.298 | 6.66 | 2.509 | 0.03 | 0.002 |
| c001 actions | 21.74 | 4.298 | 6.76 | 2.509 | 3.23 | 0.326 |
| c001 strategy | 21.56 | 4.298 | 6.72 | 2.509 | 3.26 | 0.327 |
| c001 ev | 21.63 | 4.298 | 6.70 | 2.509 | 3.19 | 0.326 |
| c013 summary | 96.91 | 10.674 | 48.40 | 8.176 | 0.04 | 0.008 |
| c013 actions | 92.37 | 10.674 | 48.29 | 8.176 | 38.80 | 2.761 |
| c013 strategy | 92.08 | 10.675 | 50.20 | 8.176 | 38.42 | 2.761 |
| c013 ev | 93.66 | 10.675 | 51.80 | 8.176 | 38.58 | 2.761 |

v3の非summaryに残る時間の大半は木の再構築である（c001で約2.1秒、c013で約25秒）。historyからnodeを引くために必要であり、この変更の対象外とした。

4 nodeの`actions`（`--node`の繰り返し 対 4回の呼出し）:

| case | 1回 秒 | 4回 合計秒 | peak GB | 行の一致 |
|---|---|---|---|---|
| c001 | 3.21 | 12.80 | 0.327 | 一致 |
| c013 | 38.65 | 154.55 | 2.761 | 一致 |

`.sol`のsize:

| case | v2 bytes | v3 bytes |
|---|---|---|
| c001 | 1,376,982,161 | 1,378,682,382（+0.12%） |
| c013 | 3,506,437,815 | 3,504,923,050（−0.04%） |

## 一致の検査

stdoutのsha256の先頭16桁で比べた。A・v3ともbaseと同じ値だった。

| view | c001 | c013 |
|---|---|---|
| actions | `5374c65a9be6d806` | `aebd9b4f68da8325` |
| strategy | `4497bd9b9e392e78` | `f617b1a34454fc0b` |
| ev | `46b6eac6c309f18b` | `ebf3173150823dd6` |

summaryは、Aではbaseと一致した（c001 `8f78536a967e9d72`、c013 `129057e5bece4e66`）。v3は再solveなので、`diff`の差は`wall_secs`の1行だけだった（c001 294.41→277.67、c013 1051.77→852.91）。iterationやEVを含む他のfieldは一致した。

`verify_save solution`でc001のv3同士を比べると、`file_bytes_equal: true`、402,890 strategy/value blocksだった。

## 再現

```sh
python measure.py SOLVERS c001,c013 LABEL SOLDIR   # runs/sol14/<LABEL>/へ出力
python multi.py SOLVERS SOL actions root x xr1.8 r6.4
```

measure.pyとmulti.pyは`psutil`を使う。
