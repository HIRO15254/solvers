# Multiway収束改善・第3段階（2026-09-09）

目標は、強力なクラウドで数時間以内にGTOWと同等、または概ね同じ傾向の戦略を
得ることである。現時点では未達。今回は参照ゲームの条件合わせ、保存した全戦略の
独立評価、クラウドへ持ち込む実験の準備を進めた。

## 参照モデルの修正

[現在のUI観測](gtowizard-target-2026-09-09.json)は、6max・100bb・NL50 General。
現在のUIで5% rake / 4bb cap、各positionのopenと、先行callerのいない15組の3bet
サイズを確認した。rake適用時点、丸め、side pot配分は未照合である。

位置ごとの3betを設定できるよう、既存BettingStateから導く
`last_preflop_aggressor_position`を追加した。規範仕様、実装ガイド、parser/runtime、
テスト、CLI reference、user guide、例を同期した。最後のpreflop raiserを表す値であり、
常に最初のopenerを表す値ではない。新たなcheckpoint保存fieldはない。

[部分参照fixture](../../examples/bench_multiway/6max_100bb_nl50_partial_reference.toml)
は通常のopen、3bet、および確認できたUTG 2bb→HJ 6.5bb後の4bet/callメニューを反映。
固定100bbで通常raiseと100bb jamを区別するSPR条件を併用し、jamへのcallを保持した。
このSPR条件は他のstackへの一般則ではない。

GTOWのUTG→HJ 3bet後は新規CO/BTN/SB/BBのcallが表示されない一方、
BTN 2.5bb→SB 12bb後のBBにはcallがある。一律の「3bet後のcold call禁止」は
この反例を壊すので採用していない。通常raise、jam、元openerのcall、BBの反例を
fixtureから実際のlegal actionsへ通す回帰テストで確認した。

未取得のsqueeze、limped pot、4bet/5betには近似サイズが残る。postflopは50% pot、
2.5倍raise、all-inの段階モデルで、GTOWのMulti Size全木ではない。
uniform heads-up EHS2 percentileもGTOWの抽象化と同一ではない。

## 全checkpointを使う評価

[mw_checkpoint_audit](../../crates/cli/examples/mw_checkpoint_audit.md)を研究用exampleとして追加。
production sessionとfingerprint検証を通して`.mwckpt`を読み、保存したpostflop policyも
含めて評価する。`.mwsol`も全streetの観測済み平均戦略を保存するが、生のregretや
平均質量0のfallback状態は保存せず、確率の量子化もあり得る。checkpoint監査では
このsolver状態を保持し、評価候補とfallbackを再現する。

各seatのdeviator候補を一度訓練して固定し、独立した複数のheld-out seedで評価する。
候補は保持できた情報集合以外ではmain regret-greedyへfallbackする。
結果はseed別に残し、最良seedの選択や混合はしない。これは有限候補による改善余地の
診断であり、完全best response、exploitability、Nash証明ではない。

preflop各nodeの169クラスに加えて、physical joint worldをサンプルし、そこへ至る
全行動の平均戦略確率を掛けて到達重みを計算する。これにより先行foldとcard removalを
反映したconditional action rate、到達率、ESS、delta-method標準誤差を出力する。
到達分母が0なら率はnull。current-regret/uniform fallbackに依存する到達重みも別に
示す。raw strategy massは相手に条件付けたrange massではなく、単純加重には使わない。
この比推定量の有限標本での不偏性は主張しない。

## 実測と測定条件の訂正

[数値・fingerprint・raw artifact一覧](multiway-convergence-round3-2026-09-09.json)に記録した。

初期のscratch生成処理が`flop = 4`を文字列置換した際、`preflop = 4`にも一致していた。
そのため旧cap1/cap2実験は**全4streetの上限が1/2**であり、postflopだけを変えた比較
ではなかった。旧入力を上書きせず、実際の上限とSHAを記録して残した。生成処理は
行頭・行末に一致させ、TOMLを再parseしてpreflop=4をassertするよう修正した。

修正後の観測fixtureでのpreflight:

| Preflop上限 | 各postflop上限 | 各street K | 完全count | Decision nodes | 戦略領域 |
|---:|---:|---:|---|---:|---:|
| 4 | 1 | 32 | はい | 2,671,933 | 1,526,165,400 bytes |
| 4 | 1 | 256 | いいえ | 2,006,324まで | 8GiBを超過 |
| 4 | 2 | 256 | いいえ | 1,906,701まで | 8GiBを超過 |

不完全countは途中prefixであり、木全体のサイズではない。戦略領域はprocess RSSでもない。
preflight・audit・build/testが同じローカルCPU上で重なるため、今回のwall timeから
厳密な高速化倍率を主張しない。

旧all-street-cap1/K256 smoke runの4096 sweeps checkpointで、3 seed×8192 worlds、
固定deviator 100,000 traversals/seatの独立監査は成功した。5つのRFI nodeの到達重みは
全て観測済み平均戦略で、fallback割合は0だった。ただしこのモデルは3betを許さず、
GTOW比較の対象にはしない。3betを含む修正版の測定は別runとして記録する。

修正版（preflop上限4、postflop上限1、K32）も4096 sweepsで終了した。
2 seed×2048 worlds、固定deviator 20,000 traversals/seatの監査で、最大候補gainの
平均値は1.984 / 1.685 BB/hand。対応するCI上限の最大値は3.737 / 3.276であり、
まだ大きな改善余地がある。通常raiseとjamを分離したnode頻度は次の通り。

| Position | 通常raise | Jam | Raise計 | GTOW参照raise計 |
|---|---:|---:|---:|---:|
| UTG | 15.83% | 2.07% | 17.90% | 17.6% |
| HJ | 17.13% | 1.59% | 18.72% | 21.6% |
| CO | 21.73% | 2.55% | 24.28% | 28.9% |
| BTN | 13.11% | 7.87% | 20.98% | 42.0% |
| SB | 18.33% | 9.03% | 27.37% | 37.5% |

SB limpは28.31%（参照11.1%）。GTOW列は
[9月8日の丸め済みUI観測](gtowizard-preflop-2026-09-08.json)。自作モデル列は8192個の
physical worldからの推定値で、標準誤差とESSはJSONに保存した。全5nodeでfallback
到達重みは0。ただしゲーム木・抽象化の差と学習不足が混在するため、この表だけで
アルゴリズムの改善効果は判定しない。特にBTN/SBの差が残り、目標達成とは扱わない。

このrunはwall 440.86秒、run内elapsed 189.56秒、最終quality行64.36秒だった。
初期化と最終保存の割合が大きい。コード調査では最終`.mwsol`生成で全公開ツリーを
再列挙し、大きなmetadataを生成する経路が見つかった。公開ノード267万件に対し、
checkpointは14.2MB、solutionは405.8MB。次段階では列挙・変換・圧縮・書込みを
分けて計測し、起動時の情報の再利用を検討する。時間差だけから原因を断定しない。

## 品質評価の並列化

`evaluate_profile_with_threads`を追加し、checkpoint auditから使用する。
sampleごとのworld、baseline、候補deviationを独立に計算し、最大4096sampleのchunk
をsample ID順に集計する。乱数、Welford集計順、最初のerrorを保持する。
既存`evaluate_profile`は1threadで動き、main CLIの停止判定の既定動作は変えない。
1/2/8thread、deviatorの有無、反復、ゼロthread拒否を回帰テストで確認した。

同じ凍結checkpointで2 seed×2048 worldsを評価した結果:

| 実装 | 評価部分の合計時間 |
|---|---:|
| 旧版・逐次 | 13.845秒 |
| 新版・1thread | 11.252秒 |
| 新版・8thread | 3.101秒 |

新版の1→8threadは評価部分が約3.63倍。old/new全3runの評価値（mean、stderr、CI、
deal attempts）、deviator訓練coverage、node exportは完全一致した。
小規模checkpointでの単一組のローカル計測で、全solveやクラウドの高速化倍率ではない。
新版1/8threadの測定は、他のsolver、audit、cargo検証が終了してから順番に実行した。
旧版の測定には他作業との重複があり得るため、速度の主比較は新版1/8threadとする。

raw JSON・command・binary/checkpoint SHAは
`runs/multiway-gtow-model-20260909/audit-parallel-benchmark/`へ保存した。

## 検証結果

- `cargo fmt --all --check`: 成功。
- `cargo clippy --workspace --all-targets -- -D warnings`: 成功。
- `cargo test --workspace`: 712 passed、30 ignored、46 suites、失敗0。
- `cargo test -p cli --example mw_checkpoint_audit`: 10 passed。
- `python -m unittest discover -s tools/tests -v`: 24 passed。
- 規範仕様とguide contract map: 23見出しが順序を含め一致。
- 最終source archive: 全163fileをmanifestのsize/SHAと照合し、別directoryへ展開。
  新しいtarget directoryでoffline/lockedのsolver・audit例build checkに成功。

## クラウド実験の方針

[GCP計画](multiway-gcp-budget-2026-09-09.md)はC4-highmem-32 Spot、32 vCPU / 248GiB、
初回最大4時間。既存$20総予算を維持する。最初に木の完全countと実RSSを確認し、
学習、checkpoint保存、独立評価の時間を分ける。小さなwarm pilotをローカルと同条件で
比較してから、長い学習へ進む。単にvCPU数から速度向上を断定しない。

まずpreflop上限4を保持してpostflop上限/Kを段階的に増やす。次に同一モデル・同一
計算時間でbatch、discount、sampling設定を比較する。判定にはGTOWのposition別頻度、
ハンド選択とraise/call/foldの構造、独立seedの差、checkpoint間の変化を併用する。
収束誤差とゲーム木・抽象化の誤差を混同しない。

GTOW公式の[solution説明](https://blog.gtowizard.com/status-and-info-about-our-solutions/)
にあるpot比の精度値は、そのまま多人数preflopの停止基準へ流用しない。
今回クラウド上でsolverはまだ実行していない。
