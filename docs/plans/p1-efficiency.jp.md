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
  card・betの非可逆な抽象化とML近似は[製品定義](../products.jp.md)が除外しているので使わない。nodelockはゲーム自体を変えるので高速化の手段にしない。
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
| C4 | nodelock・lock & re-solve後の点検（profileの無いrunの回帰）と、速度・memoryの改善候補の並列試作（2026-10-29の利用者指示） | 必須検証とrelease受入。profileの無いrunの出力はbit一致。採る変更は16・32 threadの交互計測で速くなるかmemoryが減り、他の条件を遅くしない |
| C5 | C4で判明した2点（2026-10-29の利用者指示）: profile付きrunの評価を両seat同時の走査にする。`MemoryEstimate`がstorage以外を数えない不足を直す | 必須検証。profile付きの評価値はbit一致し、16・32 threadの交互計測で評価が遅くならない。見積りが測ったsolveのmax RSS以上になり、規範・CLI reference・利用ガイド・試験を同期 |
| C6 | 次の改善案5件の並列試作（2026-10-29の利用者指示「どれも良さそうなので試してください」）: freeze_upstream時の部分木solve、relock warm start、i16の精度床を踏まえたauto、Linuxのhuge page、AVX-512 build。あわせて実験索引を現行の根拠と履歴に分ける | 採る変更は必須検証と出力のbit一致、16・32 threadの交互計測で遅い条件が無いこと。契約・既定・配布を変える案は利用者判断へ回す |
| C7 | PF12の実装と、relock warm startの追加調査（偏りか均衡の多重性か、incentiveの大きさ・lockとの組合せ） | PF12: 必須検証、`freeze_upstream`の無いrunの出力はbit一致、報告値が全木評価と一致、旧版との通しの時間を記録。規範・CLI reference・利用ガイド・試験を同期。warm start: 利用者判断の材料を記録 |

利用者決定:

| ID | 日付 | 決定 |
|---|---|---|
| PF11 | 2026-10-09 | P1の`[solver] storage`に`"auto"`を追加して既定にする。memory上限に収まればf32、収まらなければi16-f32avgを選び、どちらも収まらなければ従来どおり資源errorで止める。autoはi16を選ばない |
| PF12 | 2026-10-29 | `freeze_upstream`のrunは、profile対象の最大部分木だけをCFRで反復し、部分木の外は基準`.sol`の平均戦略のままにする。停止判定と報告するExploitability・EVは従来どおり全木の値（案A）。storageを部分木の分だけ確保する省memory化は後の段階 |

## 4. 計測

- 反復数は機械に依存しないので、候補の判定はまずローカルで反復数を測る。時間の受入は[受入機](../development.md)
  （5.4節）で16・32 threadの対を同じVMで交互に測る。本計画のC1・C2・G1はGCP c2d-highcpu-32 Spotで測り、
  2026-10-13からはAWS c6a.8xlarge（AMD EPYC 7R13）を使う。利用者決定（2026-10-29）: Spot以外のinstanceは使わない。
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

GTOW由来のrange・頻度・EVを含む比較記録は
`experiments/hu-postflop-reference/cases/ks7h2d-flop/README.md`（local-only、配布対象外）に保持する。
公開版には当方の性能測定だけを残す。
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

### C4: nodelock後の点検と改善（AWS c6a.8xlarge Spot、16・32 thread）

[記録](../../experiments/p1-efficiency-2026-10/nodelock-perf-20261029/README.md)。nodelock前`7b6492b3`と後`2b80338d`では、
profileの無いrunの1反復・afterBuild・peak・0.1%potまでの反復数は変わらず、出力はbit一致した
（評価1回だけ32 threadで1.010〜1.017倍）。改善候補を4系統で並列に試し、次の8 commitを採った。

- CFRの戦略正規化: 列の合計bufferの再利用と、i16の正のregretの1回だけの復号。
- profile付きのrunで、lockとincentiveの無い部分木をflat batch経路で処理する。
- buildの後に`node_info`を解放してheapをOSへ返し（Linux）、同じ終端の配当表を共有する。
- profileの無い評価で両seatのEV/BRを1回の走査で求め、入れ子のchanceの展開を抑えてpeakを増やさない。

`2b80338d`→採用後（`a9e30065`）を同じVMで交互に測った（flop_srp・flop_3bp・c_gtowb、f32・i16、16・32 thread）。

| 項目 | 新/旧 |
|---|---|
| i16の1反復 | 0.770〜0.834 |
| f32の1反復 | 0.972〜0.994 |
| 評価1回 | 0.844〜0.905 |
| afterBuild | 0.30〜0.41（flop_srp 351→143 MiB、flop_3bp 836→308 MiB、c_gtowb 1,665→535 MiB、32 thread） |
| peak | f32 0.821〜0.965、i16 0.725〜0.933 |
| 0.1%potまでの時間 | profile無しのf32 0.988〜0.993、i16 0.797（反復数は同じ）、lock付き0.847〜0.866 |
| 0.1%potまでのmax RSS | profile無し0.851〜0.960（c_gtowb i16は別VMの組で0.942）、lock付き0.988〜0.994 |

profileの無いrunの出力はbit一致した（bench・solveの`NashConv`、`bitcheck.sh`）。profile付きのrunは、部分木のCFRが
f32で再帰経路と異なる順に演算するため軌跡がわずかに変わる（反復数368→369、375→373）。lock付きのrunは評価が従来の
seat別の走査のままなのでpeakがほとんど減らず、両seat同時の走査へlockとincentiveを入れることが次の候補である。

### C5: profile付きrunの両seat同時評価とmemory見積り（AWS c6a.8xlarge Spot）

C4で次の候補とした2点を実装した（利用者指示「現在判明している2点に関しては実装を行ってください」、2026-10-29）。

- [両seat同時評価](../../experiments/p1-efficiency-2026-10/profiled-dual-eval-20261029/README.md): lock・incentive付きのrunも
  `dual::evaluate`で評価する。flop_srpにlock_flop・lock_turn・incentive_turnを足した木の16・32 threadで、評価1回が0.866〜0.905倍、
  評価がpeakへ足す量が112〜259 MiBから18〜42 MiBになり、peakは0.950〜0.977倍だった。benchの`nashConv`とsolveの出力はbit一致した。
- [memory見積り](../../experiments/p1-efficiency-2026-10/memory-estimate-20261029/README.md): 常駐量M（process領域・compiled game・
  profile slot・thread数分のengine scratch）とcheckpoint encoderのEを数え、見積りを`max(S + E, S − R + W + C) + M`にした。
  変更前の見積りはsolveのmax RSSを最大5.1 GiB下回っていた。新しい見積りは31件のsolveの全てでmax RSS以上で、
  flop_srp・flop_3bp・c_gtowbでは1.01〜1.16倍だった。

### C6: 次の改善案5件（AWS Spot、2026-10-29）

利用者指示（2026-10-29）: 5件を試す。i16で精度床が上がる点も考慮する。実験の履歴を現在の実装を支えるものの目次と詳細に分ける。

| 案 | 結果 | 判断 |
|---|---|---|
| [Linuxのhuge page](../../experiments/p1-efficiency-2026-10/huge-pages-20261029/README.md) | 64 MiB以上のregret・戦略arenaへprefault前に`MADV_HUGEPAGE`。THP既定`madvise`のc6a.8xlargeで1反復が8条件とも1.5〜6.8%短い（平均3.6%）。peakは最大6 MiB差、出力はbit一致 | 採用 |
| [AVX-512 build](../../experiments/p1-efficiency-2026-10/avx512-20261029/README.md) | Zen 4・Sapphire Rapidsでv3・v4・nativeを比較。i16の評価はv4で最大約14%短いが、1反復はZen 4で全条件v3より遅く、5%以上の一貫した改善なし | 不採用（RD6のまま） |
| [i16の精度床とauto](../../experiments/p1-efficiency-2026-10/storage-floor-20261029/README.md) | i16は後半に0.02〜0.09% pot付近で停滞・悪化、i16-f32avgは0.005〜0.03% pot付近まで下がる。32 threadの1反復はi16-f32avgがi16とほぼ同じ（flop_srp 0.137 / 0.135秒、f32 0.156秒）。floorを事前に予測する規則は得られない | PF11を維持 |
| [freeze_upstream時の部分木solve](../../experiments/p1-efficiency-2026-10/subtree-freeze-20261029/README.md) | 固定祖先の下の最大対象部分木だけを走査（`p1_bench --subtree-only`、engineの部分木pass）。全木走査とbit一致。1反復はlock_flop 0.66倍、lock_turn 0.34〜0.38倍、深い1 node 0.007倍。全木storageのまま。部分木の停止時点で全木NashConvは基準解の残差を含め0.011〜0.018 BB | 利用者判断で案Aを採用（PF12、C7で実装） |
| [relock warm start](../../experiments/p1-efficiency-2026-10/relock-warm-start-20261029/README.md) | checkpoint全引継ぎはincentive_turnで0.1% potまで385→81反復だが、lock 3種では増える。regret保持・sum消去は4 profileとも0.91〜0.98倍の反復。対象nodeの戦略差が残る | 追加調査（C7）の後、利用者決定で見送り（試作は`3f153de6`に残す） |

実験索引は[`experiments/README.md`](../../experiments/README.md)を「現在の実装を支える実験」と「詳細・履歴」に分けた。
dev・labのSpotは06:04（UTC）、代わりのlabは06:56に回収され、c_gtowbの深い曲線とwarm startの一部の重みは未測定のまま残る。

### C7: PF12の実装とwarm startの追加調査（AWS Spot、2026-10-29）

利用者決定（2026-10-29）: 部分木solveは案A（PF12）。warm startは追加調査の結果で判断する。

- [PF12](../../experiments/p1-efficiency-2026-10/subtree-freeze-product-20261029/README.md): `freeze_upstream`のsolve・resumeは
  最大対象部分木だけを反復し、外側のstrategy累積を基準解の平均戦略で初期化する。評価は部分木の値に開始時の外側の定数を足し、
  最終値は全木で評価する（独立した全木評価との差は最大1.2e-8 BB）。頻度目標、rootまたは空の対象、外側の戦略が基準解に無い場合、
  外側の残差だけで停止目標に届く場合は全木へfallbackし、`run.json`の`profile.solve_scope`・`fallback_reason`に記録する。
  旧版のfreeze checkpointからの再開は拒否する。flop_srp f32、c6a.8xlarge 32 threadで全木0.1% potまで:

| 対象 | 反復 旧→新 | solve 秒 旧→新 | process 秒 旧→新 |
|---|---|---|---|
| BB check後のBTN（flop） | 370 → 373 | 60.2〜60.6 → 40.1〜40.3 | 100.9〜101.2 → 77.3〜77.7 |
| xx後の全turnのBB | 325 → 148 | 52.0〜52.2 → 9.0〜9.4 | 88.2〜88.5 → 38.4〜38.6 |
| xx後のAc turnのBB | 330 → 50 | 54.2〜54.3 → 0.057 | 94.9 → 28.5〜28.6 |

  max RSSは5.5〜5.9 GBでほぼ同じ。残る時間はcheckpoint・`.sol`の保存（dev追試で17〜33秒）と基準戦略の読出し・外側の初期化（約5〜7秒）で、
  storageを部分木の分だけにする段階と合わせて次の候補である。freezeの無いrunの出力はbit一致した。
- [warm startの収束差](../../experiments/p1-efficiency-2026-10/relock-warm-start-bias-20261029/README.md): turn incentive（+5% pot）では
  checkpoint引継ぎとcoldの差が精度とともに縮み、0.01% potで真EV差−0.00019 / −0.00038 BB、到達重み付き戦略差0.018（cold自身の
  0.05%→0.01%の変化は0.040）。永続的な偏りの証拠は無いが、同一極限の証明でもない。反復の削減はturnの小さいincentive
  （+1%・+5% pot）で0.1% potまで304〜368反復あり、+20% potでは16反復、flop incentive・lock併用では逆に増える。checkpointは3.03 GBで
  setupが約6.8秒増える。利用者決定（2026-10-29）により製品化は見送る（試作は`b0bc637b`に残す）。
