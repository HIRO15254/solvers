# P1 N4: River頻度目標とEV loss順lock

再現状態: **partial**。下記のLinux実行は完了したが、sourceは未commitの作業木で、
その全差分のhashを保持していない。後日の別revisionでの一致は未検証。
source revision: `7b6492b307867331d8835870c135a645175b1b32` + **uncommitted working tree**。
測定日は2026-10-28、remote Linux、release build、1 thread、f32 storage、各4000反復、評価25反復ごと。
乱数seedは使わない。目的はN4の小さいRiver木での頻度誤差と素朴なlockの比較であり、一般的な優劣の証明ではない。

## 条件と方法

[`river.toml`](river.toml) は2 BB pot、`2s 7s Ks 4h 3d`、両席`AK,KQ,AA,QQ,JJ`、
rootでcheck / bet 1 BB、raise無し。rootのOOPが制約主体。頻度目標入力は
[`0.3`](frequency-030.toml)、[`0.5`](frequency-050.toml)、[`0.8`](frequency-080.toml)。

素朴法は無制約baselineを解き、各comboについて平均相手戦略に対するroot各actionの
counterfactual valueを互換相手reachで割った値を計算する。CLIの`ev` exportはnode単位で、
terminalも含む各action値を出さないため、補助exampleが`expected_values_at`を使う。
baselineのcheck確率から目標方向に、移動1単位当たりのEV lossが小さい順に確率を動かす。
同値はcombo名順、最後は分割して厳密に目標を満たす。全comboの結果をrootに固定し再solve。
lock TOMLは[`0.3`](naive-030.toml)、[`0.5`](naive-050.toml)、[`0.8`](naive-080.toml)として保持し、
再実行時にはignored `runs/`に再生成する。baseline action EVも`runs/`に置く。両方法の達成頻度は
`.sol`の`actions` export（保存時の量子化を含む）、誤差は達成値−目標。

## 結果

単位は頻度以外BB。EVは加点を除く真のEV、左がOOP（制約主体）、右がIP。
`g`は各runの`explP0` / `explP1`（BR利得）。[完全な数値](results.json)には最終λと
量子化前の達成頻度も記録する。

| 目標 | 方法 | 達成頻度 | 符号付き誤差 | g 主体 / 相手 | 真のEV 主体 / 相手 |
|---:|---|---:|---:|---:|---:|
| 0.3 | frequency | 0.301241 | +0.001241 | 0.001708 / 0.00000358 | 0.916751 / 1.083249 |
| 0.3 | naive lock | 0.300000 | +0.000000 | 0 / 0.00000001 | 0.895814 / 1.104186 |
| 0.5 | frequency | 0.499794 | −0.000206 | 0.000844 / 0.00002651 | 0.937983 / 1.062017 |
| 0.5 | naive lock | 0.500000 | +0.000000 | 0.00000130 / 0.00000461 | 0.899225 / 1.100775 |
| 0.8 | frequency | 0.801380 | +0.001380 | 0.000146 / 0.00001038 | 0.940165 / 1.059835 |
| 0.8 | naive lock | 0.799999 | −0.000001 | 0.00000479 / 0.00000122 | 0.929529 / 1.070471 |

3目標とも頻度目標の絶対誤差は0.01未満。この条件の真の主体EVは頻度目標の方が高かった。
ただし`g`の単純な大小は優劣を示さない。naiveはrootの全comboを固定するため、
そのBRも固定を尊重する一方、frequencyの主体の`g`は最終λの加点を含む一般和ゲームのBR利得であり、
固定lock下の真の利得BRとは異なる。頻度の制約集合そのものに限定したBRではない。
最終λと平均戦略の組合せに対する値で、λの反復中の変動も収束判定には影響する。

別のtoy検証としてPostflopProfileが`run::tests::toy_frequency_controller_converges_for_two_targets`を
remoteで実行し、Kuhnのhand別目標0.25・0.75で各絶対誤差<0.01を確認した。
これは上記River実験とは別の試験である。

## 再実行

リポジトリrootで（`CARGO_TARGET_DIR`は任意の独立したtarget directory、既存runは上書きしない）:

```sh
export CARGO_TARGET_DIR=/work/target-FreqExperiment
cargo build --release -p cli
cargo build --release -p hu-postflop --example frequency_action_ev
N4_RUNS=runs/n4-frequency-new python3 experiments/p1-nodelock/n4-frequency/run.py
```

実測はremote helper経由の同じbuild 2コマンドと、`N4_RUNS=runs/n4-frequency-3`のdriver。
`runs/n4-frequency-3/`に各solve、ログ、生成lock TOML、baseline action EV、集計JSONが残る。
結果JSONは要点だけをこのdirectoryに保持する。CLIのsolutionは量子化されるため、
frequency profileの非量子化頻度とexportの頻度は微差がある。

## 保持物SHA-256

| path | SHA-256 |
|---|---|
| `river.toml` | `1c415c8476568c2c6d78a37b484b7384475a6f7a737c810495624974a0b6dc4e` |
| `frequency-030.toml` | `1c94ecc33c8d05517e036b4f3263b9406b856da7c4ac7021807f11498cc419ff` |
| `frequency-050.toml` | `9643aec47b9494c2cc8c53ceead702f3d324bb362a4d18f382e00f541d0a1977` |
| `frequency-080.toml` | `c31618c452da30ae84a066d4c150aea1e5e35a481700e32bac141e3300cdc97e` |
| `run.py` | `283a0ca007e2fbd92ce3cf3b8e17b835edf72c2b3b4a2e03d63433406bac2c81` |
| `naive-030.toml` | `036089727082dc1a1513f0991a0fdc426993e6970ecb326322822a27919edfed` |
| `naive-050.toml` | `30a3ad6994283613ec7008bf7b64c825b78eac6f1ec3761f8ff3e33fd6bee578` |
| `naive-080.toml` | `0f0fc6c7710dd217005c9e9a2937922f87512a28f0477737774134c0857530b7` |
| `results.json` | `c9115176c3ffcfe51448f0985b6f6cb8fdba0783494cd0458abcc7bfa07cedb6` |
| `../../../crates/hu-postflop/examples/frequency_action_ev.rs` | `bfa3e86846db81266c84fc481fb437e9c94d7ee0d12bab811d21f1f14db42999` |
