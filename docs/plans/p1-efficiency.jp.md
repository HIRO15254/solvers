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
| C3 | PF11: `[solver] storage = "auto"`を追加して既定にする。Codex | 必須検証。上限内ならf32、超えればi16-f32avgを選び、run成果物には選んだstorageを記録する。規範・CLI reference・利用ガイド・template・試験を同期 |

利用者決定:

| ID | 日付 | 決定 |
|---|---|---|
| PF11 | 2026-10-09 | P1の`[solver] storage`に`"auto"`を追加して既定にする。memory上限に収まればf32、収まらなければi16-f32avgを選び、どちらも収まらなければ従来どおり資源errorで止める。autoはi16を選ばない |

## 4. 計測

- 反復数は機械に依存しないので、候補の判定はまずローカルで反復数を測る。時間の受入はGCP c2d-highcpu-32 Spotで
  16・32 threadの対を同じVMで交互に測る（GCP projectのvCPU quotaは全体で32なので、他の作業とVMを順番に使う）。
- ローカルPCは他の計算と共有しているので、時間は交互実行の中央値を参考値とする。

## 5. 結果

### C1: i16 storageの更新（ローカル8 thread、交互実行の中央値）

[記録](../../experiments/p1-efficiency-2026-10/i16-kernels-20261009/README.md)。量子化は最大値の逆数をf64で掛けて
偶数丸めし、範囲を示したうえでpacked命令の変換を使う。復号・更新・lane最大値を1回の走査にまとめた。

| 木 | storage | 1反復 旧→新 | 短縮 |
|---|---|---|---:|
| c_turn2 | i16 | 3.791 → 3.018 ms | 20.4% |
| c_turn2 | i16-f32avg | 3.344 → 3.030 ms | 9.4% |
| c_flop1 | i16 | 682.1 → 531.5 ms | 22.1% |
| c_flop1 | i16-f32avg | 705.4 → 527.7 ms | 25.2% |

f32は変えていない（差は±2%の計時の揺れ）。0.1%potまでの反復数は3比較とも変更前と同じ（310・250・190）。

### C2: 厳密評価の走査（ローカル、評価1回の中央値）

[記録](../../experiments/p1-efficiency-2026-10/eval-speed-20261009/README.md)。偶然手番を含まない小さい部分木では、
EV・BRを再帰でなく平らな走査で求め、同じ種類の終端を最大4本ずつf64のlaneにまとめて評価する。
和の順序は1本ずつの評価と同じなので、結果はbit一致のままである。

| 木 | f32 旧→新 | 比 |
|---|---|---:|
| c_turn2 | 6.18 → 5.34 ms | 0.864 |
| c_flop1 | 1.219 → 1.005 s | 0.824 |
| c_river | 0.544 → 0.560 ms | 1.030（揺れの範囲） |

i16・i16-f32avgではturn・riverで0.68〜0.79倍。EV・NashConvは全storageでbit一致し、Flopの`.sol`は計時欄以外の
全payloadが一致した。

### G2: GTOW Single Sizeの再現（Ks7h2d、cEV）

[記録](../../experiments/hu-postflop-reference/cases/ks7h2d-flop/README.md)の2026-10-09節。
GTOWで観測したsizeを`donk`・`cbet`・`aggressions`の条件で写した木（985,178 node、f32 storage 3.96 GiB）を0.049%potまで解いた。
EVはOOP/IP 1.978/3.522 bb（GTOW 1.97/3.53）、rootのcheck 100%（GTOW 100%）、BTN stab 79.4%（84.1%）、
turn BBのbet 61.6%（53.1%）、river BBのbet 31.8%（38.3%）で、2026-07-09の記録よりGTOWに近い。
ローカル12 threadで0.3 / 0.1 / 0.05%potへ91 / 179 / 231秒、GCP 32 threadで0.05%まで57.8秒だった。

### C1・C2のGCP受入（c2d-highcpu-32、16・32 thread）

[記録](../../experiments/p1-efficiency-2026-10/gcp-accept-20261009/README.md)。変更前`c09c0af`と変更後`df321c73`を同じVMで交互に測った。

| 木・storage | thread | 1反復 | 評価1回 | 0.1%potまでのsolve |
|---|---:|---:|---:|---:|
| c_flop1 f32 | 32 / 16 | 0.987 / 0.994倍 | 0.831 / 0.797倍 | 0.983 / 0.990倍（192反復で同じ） |
| c_flop1 i16 | 32 / 16 | 0.820 / 0.801倍 | 0.834 / 0.845倍 | 0.825 / 0.813倍（187→189反復） |
| c_gtowb f32 | 32 / 16 | 0.986 / 0.997倍 | 0.831 / 0.804倍 | 0.986 / 0.982倍（316反復で同じ） |
| c_gtowb i16 | 32 | 0.808倍 | 0.835倍 | 0.806倍（352→351反復） |

f32のsolveは評価の短縮ぶん（約1.5%）だけ速い。`check_every = "auto"`では評価がsolveの数%しか占めないためである。
i16は1反復の短縮がそのまま効き、0.1%potまでが約19%短い。peak RSSは変わらない。

### G1: 木の雛形（GCP 32 thread、f32、0.1%pot）

[記録](../../experiments/p1-efficiency-2026-10/templates-20261009/README.md)。

| 木 | f32 storage | 反復 | 32 thread solve | peak | EV OOP / IP |
|---|---:|---:|---:|---:|---|
| 従来の同梱flop_srp（全street 33・75%、raise 3x） | 28.41 GiB | 311 | 288.3秒 | 31.43 GiB | 1.521 / 3.300 |
| 新しい同梱flop_srp（flop 33%のみ、donk無し） | 3.70 GiB | 389 | 53.6秒 | 4.70 GiB | 1.516 / 3.331 |
| 上にflop 75%を追加 | 6.73 GiB | 371 | 90.5秒 | 7.99 GiB | 1.531 / 3.325 |
| flop_3bp（3BP、flop 20・56・122%） | 1.44 GiB | 260 | 17.8秒 | 2.18 GiB | 16.587 / 3.905 |
| flop_4bp（4BP、flop 13・38・67%・all-in） | 0.10 GiB | 99 | 1.4秒 | 0.30 GiB | 4.978 / 40.559 |

GTOWに倣って木を絞ると、0.1%potまでの時間は5.4分の1、peakは6.7分の1になり、OOPのEV差は0.005 BBだった。
同梱例・規範第14節の例・利用ガイドをこの木へ更新した。

### C3: storage auto（PF11）

`[solver] storage`の既定を`"auto"`にした。準備段階でmemory見積りと上限からf32かi16-f32avgを選び、`run.toml`・checkpoint・
`.sol`の実効configには選んだstorageを書く。正規化した入力（`validate --show-effective`）は`auto`のままである。
同梱flop_srpを`--memory 3700MiB`で解くとi16-f32avgを選び、`run.toml`と`.sol`のmetaは`i16-f32avg`だった。`--memory 2GiB`は
両方の必要量を示して終了code 75で止まった。
