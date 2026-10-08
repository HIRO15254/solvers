# P1の資源効率（S5、2026-10-09〜）

少ない資源（時間・memory）でExploitabilityの低い解を得るための実行計画である。作業状態はLinear SOL-32
（[管理先](../status.jp.md)）、計算と成果物の契約は[P1規範](../hu-postflop.jp.md)を正本とする。
前段の速度改善（S3、T1〜T26、SOL-15）は[P1性能計画](p1-performance.jp.md)にある。証拠は
[`experiments/p1-efficiency-2026-10/`](../../experiments/p1-efficiency-2026-10/)に置く。

## 1. 目的と前提

- 利用者の指示（2026-10-09）: P1において、より少ないリソース（時間・メモリ等）で良好な（Exploitabilityが低い）解を
  得るためにあらゆる手段を用いる。Treeの設定や理想的な収束後の戦略はGTO Wizard（以下GTOW）を参考にする。
  仕様が明確な実装はCodexのsubagentへ任せる。
- 主指標は前段と同じく、NashConv/2が開始potの0.1%以下になるまでの時間（16・32 threadの対で判断）とpeak memory。
  GTOWの公開値では、事前計算libraryの精度は0.2〜0.3%pot、GTOW AIのflop解は平均0.12%potである
  （[AI benchmarks](https://blog.gtowizard.com/gto-wizard-ai-benchmarks/)、
  [status](https://blog.gtowizard.com/status-and-info-about-our-solutions/)）。0.1%potはこれと同等以上に厳しい。
- 「あらゆる手段」には算法の変更も含む。ただし有限木の厳密BRで測るExploitabilityの意味は変えない。
  card・betの非可逆な抽象化、ML近似、nodelockは[製品定義](../products.jp.md)が除外しているので使わない。
- 規範・既定値を変える場合は[AGENTS.md](../../AGENTS.md)の同期規則に従う。

## 2. 現状の位置（2026-10-09）

### 公開実装との比較

同一の木（decision nodeの履歴・額まで一致を確認）でpostflop-solverと比べた
（[記録](../../experiments/p1-efficiency-2026-10/pfs-compare-20261009/README.md)、8 thread）。
P1は0.05%potまでの時間が0.28〜0.48倍で、反復数（0.36〜0.74倍）・1反復（0.61〜0.77倍）とも先行している。
残る差は、厳密評価1回の費用（P1 1.8〜2.3反復分、pfs 0.9反復分）、i16 storageの1反復の費用、8 threadでの伸び
（P1 4.5〜4.7倍、pfs 5.0〜5.1倍）、storage以外のmemory（flopで約232 MiB）である。

### 不採用にした候補

[記録](../../experiments/p1-efficiency-2026-10/rejected-20261009/README.md)。

| 候補 | 結果 |
|---|---|
| Regret-based pruning（全走査の間に刈る行動の枝を飛ばす。追いつき有無） | 刈れる仕事は最大約25%。DCFR（β=0.5）では刈った行動がすぐ戻り、反復数の増加が上回る。全変種で遅くなった |
| 偶然手番サンプリングMCCFRで温めてから厳密DCFR | MCは20,000反復（13.8秒）でも7%pot。移した後も冷えた開始より多く反復した |
| PGO build | c_turn2・c_flop1とも速くならなかった |

前段で不採用にしたもの（PDCFR+、PCFR+、HS-DCFR、linear CFR、CFR+、1326-dense kernel、sparse reach kernel等）は
[P1性能計画](p1-performance.jp.md)と[T26後の改善案](../research/2026-10-08-p1-t26-improvements.jp.md)にある。

## 3. 段階

| 段階 | 内容 | 受入条件 |
|---|---|---|
| E1 | postflop-solverとの同一木比較、RBP・MC warm start・PGOの試作 | 記録済み（第2節） |
| C1 | i16・i16-f32avg storageのregret・戦略累積更新を速くする（量子化を逆数の乗算・偶数丸めにし、復号・更新・最大値探索を1回の走査にまとめる）。Codex | 必須検証。i16・i16-f32avgの1反復が速くなり、f32は不変。0.1%potまでの反復数が変更前の+3%以内 |
| C2 | 厳密評価（EV・BR）の走査を速くする。結果は変更前とbit一致のまま。Codex | 必須検証。評価1回の時間が縮み、EV・Exploitability・`.sol`がbit一致 |
| G1 | GTOWの公開情報に基づく木の雛形（SRP・3BP・4BP）を作り、各木の資源（node数・memory）と0.1%potまでの時間を16・32 threadで測る。同梱の`examples/hu-postflop/flop_srp.toml`（30.5 GB）を置き換える | 雛形の資源表。同梱例を変える場合は規範・CLI reference・利用ガイド・試験を同期 |
| G2 | GTOW Single Size（cEV）のKs7h2d SRPを現在の木文法（donk・cbet・攻撃回数による条件）で再現し、GTOWの頻度と比べる | 2026-07-09の記録より近い木で、root・BTN stab・turn nodeの頻度・EVの差を報告 |

## 4. 計測

- 反復数は機械に依存しないので、候補の判定はまずローカルで反復数を測る。時間の受入はGCP c2d-highcpu-32 Spotで
  16・32 threadの対を同じVMで交互に測る（GCP projectのvCPU quotaは全体で32なので、他の作業とVMを順番に使う）。
- ローカルPCは他の計算と共有しているので、時間は交互実行の中央値を参考値とする。
