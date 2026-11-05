# P2の実験の大きいfile

P2方式の再設計の実験が使う大きいfileの置き場所と、2026-10-29に消したfileの記録。
大きいfileはgitの外にある。gitにあるのは、ここと各実験の`*.sha256`の記録だけである。
実験の索引は[README](README.md)、数値の物差しは[metrics.md](metrics.md)。

| 場所 | 大きさ（2026-10-29） | 中身 |
|---|---:|---|
| `.cache/p2-trunk/` | 約20 GB | 実験ごとのcache（`.gitignore`の対象）。`gtow-fixed/`が14.4 GB |
| `../_goal/` | 約11 GB | repoの外の作業領域（worktreeの隣）。手元とcloudの実行のscratch |
| `$LOCALAPPDATA/solvers/ehs2/` | — | EHS²の表（`--ehs2-cache`、`--l1-relative`の強さの表） |

## 今も使うもの

`gtow-fixed`の実験は、[`bench-env.sh`](gtow-fixed/gcp/bench/bench-env.sh)の変数（`CACHE`、`BENCH_CACHE`、`GTOW_PROFILE`、`E2_PROFILE`、`E2_L1_SNAPSHOT`、`SCRATCH`）でこれらを指す。
pathは`.cache/p2-trunk/gtow-fixed/`からの相対である（`../_goal/`で始まるものを除く）。

| path | 大きさ | 役割 | hashの記録 |
|---|---:|---|---|
| `v2/e2/` | 1.14 GB | 基準の全体の解E2 `traise5+p10+g25+hd`（seed 0）。2000 itのprofileが`E2_PROFILE` | `gtow-fixed/results/v2/retained.sha256` |
| `v2/e2sd1/` | 1.14 GB | E2のseed 1 | 同上 |
| `raise5-plateau/` | 0.45 GB | E2の6000 it（頭打ちの確認） | `gtow-fixed/results/raise5-plateau/retained.sha256` |
| `improve/` | 0.42 GB | 候補の全体の解（`im-d-r16g49q-sd0`など。KPIのrangeの元） | `improve/aws/*.final.tgz`だけ`gtow-fixed/results/bench/retained.sha256`にある。展開したdirは記録なし |
| `gtow-raise5.profile.json` | 0.01 GB | GTO WizardのPreflopのprofile（攻撃5回の木、`GTOW_PROFILE`） | なし。sha256 `49e77437e53ac0ab62e0ee9a116850933294fca022583106df58d66130b397c4` |
| `gtow.profile.json`、`gtow-p10.postflop.bin` | 小 | 攻撃4回の木の同じもの | `gtow-fixed/results/retained.sha256` |
| `replan/gtowflop-sd1/rel8x2@raise5+g25+hd+sd1/` | 1.05 GB | E2のL1をGTO WizardのPreflopで学習し直したもの（`E2_L1_SNAPSHOT`） | なし。`postflop.bin`のsha256 `11630e6505972f8bd6fb5750342288c7037cbc2f290d77f1336f2f1f33e6333d` |
| `replan/gtow-agg/` | 0.02 GB | 36本のlineの出所（GTO WizardのFlopのreportの集計。local-only） | `gtow-fixed/results/replan/gtow-agg.sha256` |
| `bench/train/gtow/rel16x2@raise5+g49+hdq+sd1/` | 3.50 GB | `r16g49q`のL1（GTO WizardのPreflop）。`postflop.bin.v2`は2026-10-29のmerge後のfingerprintに付け替えたもの。2026-11-05のmerge後は`REBIND=1`が`postflop.bin`から`postflop.bin.v3`を作る | `gtow-fixed/results/bench/retained.sha256`。`postflop.bin.v2`はsha256 `b081b311f91ba0ce5ba749455344347ed31d3999ec9d6d82131443b7b1ac703d` |
| `bench/train/gtow/rel8x2@raise5+g25+hdq+sd1/`、`bench/train/gtow/rel8x2@raise5+hd+sd1/` | 0.46 GB | `r8g25q`のL1と、群なしの比較（GTO WizardのPreflop） | `gtow-fixed/results/bench/retained.sha256` |
| `bench/train/e2/rel8x2@raise5+g25+hd+sd1/`、`bench/train/e2/rel8x2@raise5+hd+sd1/` | 0.27 GB | E2のPreflopで学習し直したL1 | 同上 |
| `bench/train/r16g49q/`、`bench/train/r8g25q/` | 2.20 GB | KPI（`m-kpi`）用に、各候補の自分のprofileで学習し直したL1 | なし。`postflop.bin`のsha256は`r16g49q` `2fd996eaa655a81588a9bc13ae493a8a4c31a7d48cf64cfdda55645262737fd5`、`r8g25q` `a9b33f28800863ea38cfdc9d815dc26ea9685784006047a5511fecda7b481027` |
| `bench/l1/`、`bench/l1.tgz` | 1.47 GB | 36本の物差しとKPIのL1の診断の値 | `bench/l1.tgz`だけ`gtow-fixed/results/bench/retained.sha256`にある |
| `bench/p1/` | 0.07 GB | 36本の物差しとKPIのP1の値（1 size・複数size） | なし |
| `bench/aws/` | 1.15 GB | 採った案と正解のcloudの結果のtgz（`own-l1`、`im-b-r16g49q`、`im-b-r8g25q`、`l1-e2`、`l1-gtow`、`p1-*`、`pilot-*`、`im-c-*`、`own-*-4c`、`trunk-eval`） | `gtow-fixed/results/bench/retained.sha256` |
| `bench/pilot/`、`bench/ruler*/`、`bench/trunk*/` | 0.27 GB | 物差しの入力、試し測定、trunkの評価 | なし（cloudの結果は`bench/aws/`の`pilot-*`・`trunk-eval`のtgzにある） |
| `leaf-own/` | 0.08 GB | leafごとの群の実験（`gtow-fixed/notes/27-leaf-own.md`）のL1の診断の値・学習のlog、leafごとの群、全181 leafのreach | `gtow-fixed/results/leaf-own/retained.sha256`、`gtow-fixed/results/leaf-own/inputs.sha256` |
| `../_goal/gtowtree/` | 0.22 GB | L0の表（`--tables-dir ../_goal/gtowtree/tables`） | `gtow-fixed/results/qre-genuine/inputs.sha256` |
| `../_goal/fixed/flops.csv` | 小 | 同梱のflopの群（`flop_subset.py`の49群・25群）の入力。`flop_features`の出力 | `gtow-fixed/results/quadrature/inputs.sha256` |
| `../_goal/sc/` | 6.43 GB | SCの調査のP1の解（`run-{1,2,3}/solution.sol`）とdump | なし |
| `../_goal/v2/` | 0.84 GB | E2・`e2sd1`のcloudの結果のtgz | なし |
| `../_goal/bench/` | 2.11 GB | 物差しとKPIのcloudの実行の入力と、小さい結果（`*-up/`、`res.tgz`） | なし |
| `../_goal/compat/`、`../_goal/merge/`、`../_goal/aws/` | 0.07 GB | merge前後の一致の確認、mergeの作業、AWSの作業 | なし |

`.cache/p2-trunk/`の他の実験のdir（`l1-6max`、`b4s-8h`、`l1-relative`、`l1-core`、`relative-reach`など）は、2026-10-29には触っていない。

## 2026-10-29に消したもの

利用者の決定（2026-10-29）で、不採用・旧方式の実験の10 MiB以上のfileを消した。261 file、62.67 GB。
1つずつのpath・大きさ・sha256は[`artifacts-deleted-2026-10-29.tsv`](artifacts-deleted-2026-10-29.tsv)にある。
10 MiB未満のfile（log、text、hashの記録）は残したので、消したdirにも小さいfileが残っている。

| 群 | file | 大きさ |
|---|---:|---:|
| SCの調査のP1のcheckpoint（`../_goal/sc/run-{1,2,3}/checkpoint.ckpt`。`solution.sol`は残した） | 3 | 11.44 GB |
| cacheと同じ中身のtgzの写し（`../_goal/bench/*/final.tgz`。sha256が`.cache`のものと一致） | 6 | 約1.14 GB |
| `bench/train`の不採用の案（GTO WizardのPreflop） | 16 | 14.75 GB |
| `bench/train`の不採用の案（E2のPreflop） | 3 | 1.30 GB |
| `bench/aws`の不採用の案のtgz | 21 | 3.95 GB |
| `.cache/p2-trunk/gtow-fixed/`の旧方式・不採用の実験（`v2/b`、`qre-*`、`b01`、`flop-recall`、`own-reach`、`axes-full`、`raise5`、`hand`、`groups*`、`long`、`final`、`snap-traise+p10`、`replan/rep1`） | 82 | 12.75 GB |
| `../_goal/`の旧方式・不採用の実験（`replan`、`conv`、`b01`、`qre-*`、`flop-recall`、`own-reach`、`smoke`、`fixed`） | 92 | 13.47 GB |
| `../_goal/bench/`の不採用の案 | 38 | 3.87 GB |

- 消す前に、hashの記録があった77 fileは、記録のsha256と一致することを確かめた。
- 各実験の`*.sha256`のうち、pathがこのTSVにある行は、2026-10-29に消したfileである。
- 同じsha256が残っているfileにあることもある（写し）。たとえば`raise5-plateau/`は`../_goal/conv/full/aws-r5/`の写しと同じ中身で、`raise5-plateau/`の方を残した。
  sha256だけで消えたかどうかを決めない。

## 2026-10-29より前に消えていたもの

次の記録の行は、2026-10-29の前にfileがもう無かった。
2026-10-16の古い物差しの整理（commit `82c636e7`がresultsとscriptを消した）のときにcacheも消したと考えられる（推測。cacheはgitの外で、消した記録がない）。

| 記録 | 行 | 状態 |
|---|---:|---|
| `gtow-fixed/results/converge/retained.sha256` | 177 | 174行が前から無い。3行は2026-10-29に消した |
| `gtow-fixed/results/low-spr/retained.sha256` | 118 | すべて前から無い |
| `gtow-fixed/results/flop-recall/retained.sha256` | 48 | すべて前から無い |
| `gtow-fixed/results/replan/retained.sha256` | 45 | 37行が前から無い。8行は2026-10-29に消した |
| `gtow-fixed/results/quadrature/inputs.sha256` | 21 | `results/low-spr/*.toml`の3行が前から無い |

pathの基点が記録に書かれていない記録（`snap/…`や`final/…`で始まるもの）は、ここでは確かめていない。
