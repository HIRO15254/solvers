# P2方式の再設計（2026-10）

[P2方式の再設計計画](../../docs/plans/p2-method-redesign.jp.md)（SOL-25）の判断と受入に使う実験の索引。
作業状態は[Linear](../../docs/status.jp.md)にある。

読む順番：

1. 下の「現在地」。
2. [物差しの一覧](metrics.md)。数値を読む前に、その数値の物差しのIDと、比べてはいけない組を確かめる。
3. 下の索引から各実験のREADME（詳細）。「アーカイブ」の実験は、今の実装と方法を決めていない。
4. 大きいfileの置き場所と、2026-10-29に消したfileは[artifacts.md](artifacts.md)。

GTO Wizardの値を含む記録はlocal-onlyで、配布しない（`packaging/gtow-local-only.json`）。
索引では、そのREADMEをlinkにせず、pathだけを書く。

## 現在地（2026-11-04）

- 製品のP2は、今も暫定方式（`solver.kind = "range-vector"`、ES-MCCFR）で解く。
  trunk・L0・L1は実験用のcodeで（`crates/mw-preflop/src/lib.rs`の`trunk`は製品の契約の外）、製品の既定値は変えていない。
- L1の推奨の組は`CardSpec::recommended()`（`crates/mw-preflop/src/trunk/l1/spec.rs`）で、相対bucket 16 × 強さのlevel 4
  （flop・turn・riverとも、nodeでの相手のreachに対するshowdownのequityの等分。flopのSPRが2.5以下のL1 leafはlevel 2）、
  prior 10 combo、`draws`と`pairs`、flopの群は各L1 leafの2人のrangeで重みを付けた49群（leafごとの群。どのseatも届かないleafは同梱の49群）である。
  leafごとの群と強さのlevelを除いた部分は`r16g49q`の組（実験のtokenでは`rel16x2@raise5+g49+hdq`）。2026-10-29に利用者の決定でE2の組
  （`rel8x2@raise5+g25+hd`）から`r16g49q`に替え（計画のP2D11）、2026-10-30にleafごとの群を加え（P2D12）、
  2026-11-04に強さのlevelを替えた（P2D14。leafのrangeを固定した36本の比較は[`eval-level.md`](gtow-fixed/results/abs-level/eval-level.md)）。
  全体の解は2回に分けて解く（1回目は`--l1-leaf-flop-groups none`、2回目はそのprofileから`own:`で群を作る。計画の第3.4節）。
  2回に分けて解いた全体の解のKPIも測った（下の表の`r16g49qo`、P2D14の前の組）。P2D14の組の全体の解は未測定で、
  推奨を暫定から外すかは利用者の判断を待つ。
- メモリ・計算時間と精度を交換する改善（turn・riverの分割など）は、利用者が選ぶ部分とする（計画のP2D13）。
- 実験の基準は、木`b4s-8h/b4s-gtow-raise5.toml`（攻撃5回。GTO WizardのPreflopのmenuを使うのでlocal-only、非配布）と、全体の解E2 `traise5+p10+g25+hd`の2000 it・seed 0
  （[`bench-env.sh`](gtow-fixed/gcp/bench/bench-env.sh)の`E2_PROFILE`）である。推奨の組を替えた後も、比べる基準はE2のままにしている。
- KPIは`m-kpi`（利用者決定 2026-10-15、計画のP2D10）。L1の値と、同じ木を複数のbet sizeで解いたP1の値との差を、各解の自分のレンジで測る。
  P2D8の主指標（`m-main`）はsolverの収束の物差しで、KPIではない。
- 候補のKPI（[`own-kpi.md`](gtow-fixed/results/improve/own-kpi.md)、bb、±は標準誤差）：

  | 帯 | E2 | `r16g49q` | `r16g49qo` | `r16g49q` − E2 | `r16g49qo` − `r16g49q` | `r8g25q` − E2 |
  |---|---:|---:|---:|---:|---:|---:|
  | SRP | 未測定 | 未測定 | 未測定 | 未測定 | 未測定 | 未測定 |
  | 3bet（5本） | 1.392 ± 0.101 | 1.052 ± 0.066 | 0.875 ± 0.045 | −0.339 ± 0.120 `better` | −0.177 ± 0.080 `better` | −0.021 ± 0.147 `equal` |
  | 4bet（16本） | 2.515 ± 0.252 | 2.089 ± 0.195 | 1.580 ± 0.068 | −0.426 ± 0.318 `equal` | −0.509 ± 0.206 `better` | −0.003 ± 0.413 `equal` |
  | 5bet | 1.868 ± 0.588 | 1.915 ± 0.432 | 0.792 ± 0.310 | +0.047 ± 0.730 `equal` | −1.123 ± 0.531 `better` | −0.001 ± 0.710 `equal` |

  `r16g49q` = `rel16x2@raise5+g49+hdq`（全体の解は`traise5+p10+g49+hdq+rel16x2`）、`r8g25q` = `rel8x2@raise5+g25+hdq`。
  `r16g49qo` = leafごとの群を使い2回に分けて解いた全体の解`traise5+p10+g49+hdq+rel16x2+own`（1回目は`r16g49q`のit001000）。
  記録は[`full-solve.md`](gtow-fixed/results/leaf-own/full-solve.md)（計算時間・費用・rangeの動きを含む）。
- まだ測っていないこと：SRPの帯のKPI（どの解も。SRPの複数sizeのP1がない）。SRPは1 sizeのP1とのAだけ測った。
  `r16g49qo`の1 sizeのP1との差（カード近似）。
- 調べていること：raiserのsuited connectorの偏り（`gtow-fixed/notes/26-sc-strategy.md`、local-only）。
  SCに限らず、Preflopでその行動を混ぜるclassほどL1 − P1が負で、いつも選ぶclassは正になる（3bet・4betの帯で大きい）。
  主因はflopの群とturn・riverのcardで戦略を共有すること（1 flop 1群やturn・riverの分割で縮み、組の数・iterationでは縮まない）。
- 帯を名指ししない直し方を、`r16g49q`の解のレンジでbenchのlineを1本ずつ学習し直して測った
  （[`eval-own.md`](gtow-fixed/results/leaf-own/eval-own.md)、[`eval-own-vs-own.md`](gtow-fixed/results/leaf-own/eval-own-vs-own.md)、
  `gtow-fixed/notes/27-leaf-own.md`、local-only）。
  leafごとに自分のレンジで重みを付けた49群は、Aを−0.118 ± 0.035（4帯）、KPIを−0.278 ± 0.096（3bet・4bet・5bet）変える。
  turn・riverの分割は、分割後のslotが上限以下の小さい木だけに掛けると効かない（+0.001 ± 0.002）。
  後ろの木が大きいleaf（分割後7M slot超）だけに掛けると、さらに−0.015 ± 0.004（KPI −0.026 ± 0.009、全体のslotは2.2倍）。
  この境は結果を見てから決めた。利用者の決定（2026-10-30）で、leafごとの群を推奨値に加えた（P2D12）。turn・riverの分割は推奨値に入れない（P2D13）。
  Rustの実装（`trunk::l1::own`）とPythonの群の照合、群を作るiteration数は[`rust-check.md`](gtow-fixed/results/leaf-own/rust-check.md)。
  全体の解での確認は`full-solve.md`。

## 索引：現行の実装と方法の根拠

| 実験 | 日付 | 問い | 結論 | 物差し | 再現状態 |
|---|---|---|---|---|---|
| [暫定方式のseed間の差](legacy-seed-noise/README.md) | 2026-10-06 | seedと計算量で、暫定方式の解がどれだけ変わるか | 6max 20bbで、到達確率で重み付けた行動のTVは30k sweepで8.6%、300kで3.1%。停止判定の`deviationGainLowerBound`は厳密な利得と合わない。計画の第1節の動機で、解は下の3実験の入力 | `m-seed-tv` | `partial` |
| [L0 hand-class tables](trunk-tables/README.md) | 2026-10-06 | L0評価器の表（T2、T3）は定義どおりか。生成時間は目標内か | T2は46.5秒、T3は133.6秒で目標内。2回でhashが一致し、検査に合格。`trunk`の表として使う | — | `verified` |
| [L0評価器の検査と暫定方式の測定](l0-evaluator-check/README.md) | 2026-10-06 | L0評価器は正しいか。暫定方式の解は、L0モデルの中でどれだけ得をされる余地があるか | 参照実装と独立実装に一致。B3の暫定方式の解の`NashConv`は30kで約0.35、300kで約0.053 bb/hand。評価器の定義はP2D1の根拠 | `m-l0` | `verified` |
| [L0の誤差の測定](l0-real-check/README.md) | 2026-10-06 | L0のseatの値と利得は、入力のゲームとどれだけ違うか | L0はどのseatの値も高く見る。差が約0.02 bb未満の解どうしは、L0の`NashConv`で区別できない可能性がある。`trunk::l0::real`を道具として使う | `m-l0`、`m-real` | `verified` |
| [入力のゲームでの最適応答](real-br-fit/README.md) | 2026-10-06 | 入力のゲームで、class単位の最適応答は暫定方式の解に対してどれだけ得をするか | 300kの解で、入力のゲームの`NashConv`は0.0395〜0.0445（L0では0.0531）。`fit_real_responses`を道具として使う | `m-real` | `verified` |
| [trunkのDCFR solver](trunk-solver/README.md) | 2026-10-06 | L0モデルの上の全幅DCFRはB1の目標に届くか。B3の解は暫定方式の解より良いか | B1は50 iterationで目標に届く。B3を200 iteration解くと、入力のゲームの`NashConv`は約0.0056〜0.0118で、暫定方式の約1/3〜1/5。`trunk::l0::solve`として採用 | `m-l0`、`m-real` | `verified` |
| [trunk solverの高速化](trunk-speedup/README.md) | 2026-10-07 | P2D7のsolverの中だけのK4の近似で、1 iterationはどれだけ短くなるか。`NashConv`は同じように下がるか | 近似したsolverの`NashConv`は厳密なsolverの0.97〜1.02倍。P2D7を採用（実験は`--solver-k4-samples 256 --solver-k4-min-samples 16`で解く。codeの既定は近似なし） | `m-l0` | `verified` |
| [L1の核と評価器](l1-core/README.md) | 2026-10-07 | L1をtrunkと同じiterationで解くと、B6でP2D8の主指標は目標に届くか | B6は500 iterationで目標に届く（2000 iterationで4096 boardの上振れする値0.00252、下振れする値−0.00200）。`l1::Options::default()`の根拠 | `m-main`、`m-aux` | `verified` |
| L1で6人の木を解く（`l1-6max/README.md`、local-only） | 2026-10-07〜08 | B7とB4 Simpleの元の木をL1で解いたP2D8の推移、評価のばらつき、16・32 threadの時間 | 1024 boardの評価では主指標を0.01より細かく区別できない。P2D7の最小標本数（m = 16・64）と、実装の変更A〜J（主にbit不変）を採用。openの表の比較は廃止した物差し | `m-main`、`m-aux`、`m-open-mae`（廃止） | `verified` |
| [trunkの並列化](trunk-parallel/README.md) | 2026-10-08 | 解く結果をbitで変えずに、並列化でtrunkの1 iterationをどれだけ短くできるか | 32 threadsで、B3のL0が0.88倍、B7のL1が0.96倍、B4 SimpleのL1が0.99倍。bit一致のまま採用 | 1 iterationの秒 | `verified` |
| L1の相対の組（`l1-relative/README.md`、local-only） | 2026-10-08 | L1のPostflopの組の作り方（到達rangeの中の順位・模様・前のstreet）で、補助指標はどれだけ減るか | 相対bucket（8 × 2）を採用。B6の補助指標は1.70から1.13、B4 Simpleでは20.93から10.15 bb/handに減った。模様・前のstreetの`refine`とriverのequityの組は採らない | `m-aux`、`m-open-mae`（廃止） | `verified` |
| 相対の組の到達rangeを直す（`relative-reach/README.md`、local-only） | 2026-10-09 | 到達rangeがほぼ空のnodeで組が潰れる問題を、事前の重み（prior）と平均の戦略の到達rangeで直せるか | prior 10 comboを採用（`CardSpec::recommended()`）。1 iterationの時間は変わらない。`--l1-relative-reach average`は採らない | `m-gtow-loss`（警報のみ）、`m-gtow-tv`（廃止）、`m-aux` | `partial` |
| GTO Wizard 100bb相当の木（`b4s-8h/README.md`、local-only） | 2026-10-09 | GTO Wizard Simple 100bb相当の木を長く解くと、GTO Wizardの解に近づくか | 木（`b4s-gtow-raise5.toml`は今の基準の木）と道具（`compare.py`、`cloud_progress.sh`、`gcp/`、`aws/`）は今も使う。GTO Wizardに近づくかという結果は、2026-10-15のKPIの変更で採否に使わない | `m-gtow-loss`（警報のみ）、`m-gtow-tv`（廃止）、`m-main` | `partial` |
| GTO WizardのPreflopを固定してL1を測る（`gtow-fixed/README.md`、local-only） | 2026-10-10〜 | L1の抽象化（カードの組・Postflopのmenu）の案を、実際のカードのP1を正解にして選ぶ | 前の推奨の組E2（25群、`draws`、prior 10）と攻撃5回の木を決めた。2026-10-29に推奨の組を`r16g49q`に替え（計画のP2D11）、2026-10-30にleafごとの群を加えた（P2D12）。KPIの候補は上の「現在地」。概要は`gtow-fixed/README.md`、現行に効く節は`gtow-fixed/notes/`、効かない節は`gtow-fixed/archive/` | `m-kpi`、`m-class`ほか | 節ごとに記録 |

## 索引：アーカイブ（現行の実装と方法に影響しない）

| 実験 | 日付 | 問い | 結論 | 物差し | 再現状態 |
|---|---|---|---|---|---|
| 3bet・4bet potのflopにGTO Wizardのsizeを入れる（`lowspr-menu/README.md`、local-only） | 2026-10-09 | 3bet・4bet potのflopに小さいbetを入れると、4betへのcallが出るか | 不採用。4betへのcallは出ず、flopのsizeの違いは原因ではなかった。1 iterationは1.85倍。木`b4s-gtow-lowspr.toml`は`gtow-fixed/notes/03-buckets-and-menu.md`が使う | `m-gtow-loss`（警報のみ）、`m-gtow-tv`（廃止） | `partial` |
| Postflopの最大sizeをgeometricに替える（`geo-menu/README.md`、local-only） | 2026-10-09 | Postflopのbetを「50%かall-in」から最大sizeをgeometricにした2 sizeに替えると、GTO Wizardの解に近づくか | 不採用。同じ1000 iterationで元の木より悪く、1 iterationは2.2〜2.5倍。4betへのcallは出なかった | `m-gtow-loss`（警報のみ）、`m-gtow-tv`（廃止） | `partial` |

`gtow-fixed/archive/`の節（QREの到達range・prior、flopのrecall、B07など）もアーカイブである。
ここに残したcodeの実験用のflag（`--l1-low-spr`、`--l1-qre-prior`、`--l1-regret-weight`、`--l1-hand backdoor,gutshot`など）は、製品の既定では使わない。
