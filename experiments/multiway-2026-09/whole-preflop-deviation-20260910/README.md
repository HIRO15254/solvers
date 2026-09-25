# 全プリフロップ逸脱評価器と6席pilot

ポストフロップを固定する全プリフロップ評価器を実装したが、8192回/席のfitでは
**両方式・全6席・両検証seedの24組すべてで逸脱利得が負**になった。通常探索の全12組と
列挙探索の10組はpointwise 95% CI上限も0未満で、残る列挙UTGの2組は0を含む。
この予算では有効な正利得候補を得られていない。低い検出利得をソルバーの強さや
収束の証拠として扱わず、次に評価用fitの検出力を確認する。

通常探索・レイズ後列挙の両方で、既存診断と全6845 Preflop nodeのraw fingerprintは
保存済み結果と一致した。追加の評価、時計、および事前指定した説明文の変更だけが差分である。
このpilotは数値検証と費用を確認し、有限fitの弱さを切り分けた。1学習seedで得た有限の逸脱候補の利得から、
探索方式の優劣やproduction default変更は決めない。

[完全な診断・coverage・provenance JSON](result.json) /
[実行前の固定計画](plan.md) /
[以前の全tree supportと探索費用](../raised-opponent-20260910/README.md)

## 何を評価したか

`evaluate_preflop_deviation` は各席を単独でfitし、その席の全Preflop判断を変更できる。
本人の過去の行動確率が0の先もfitする。相手全席、本人のPostflop判断、8 fit visits未満の
未採用keyは同じcandidate baselineへ固定し、別のgreedy方策で補わない。
各席のtableを同時に適用した共同戦略の評価ではない。

held-outは同じphysical worldとaligned action RNGをbaseline/単独逸脱へ与え、
全サンプルの符号付き差をWelford集計する。負値も未採用keyも除外しない。
1回に保持するsample結果は最大4096件で、sample-id順に集計するためスレッド数によらない。
fit tableのメモリは訪問Preflop key数に応じて増え、この4096件制限はsolver全体のRAM上限ではない。
fit訪問counterは旧all-street fitも含めchecked u64へ変更し、u32飽和を解消した。

CLIの4つの `--preflop-deviation-*` flagは明示指定時だけ有効。出力は
`solvers.multiway-preflop-deviation/v1`、scopeは
`all-preflop-decisions-with-frozen-postflop`。通常の停止、学習default、checkpoint/solution
wire形式は変更していない。fit fingerprintは採用action tableの識別子であり、baseline識別子ではない。

## 固定条件と測定費用

6max / 100bb / 5% rake・4bb cap / EHS² K32 current-street / range-vector。
部分的なGTO Wizard Simple観測に基づく既存fixtureを変更せず使用した。limpはなく、
未観測のraise menuとPostflopは近似Treeである。GTO Wizardと同じゲームの解とは扱わない。
学習seed 0、batch 4、8192 sweep、8 threads、8 GiB設定、warm EHS cache。
両方式とも同じresearch-feature binary。列挙以外の学習条件を揃え、逐次実行した。
同sweep数の比較であり、同計算時間の比較ではない。

各席8192 fit traversals、fit seed 2601。held-outは各seed 32768 worlds、seed 2701/2702。
予算・順序・600秒timeoutを実行前に固定し、観測利得によるseed選択や再fitは行っていない。

| 方式 | 構築 s | 学習 s | 全6席fit s | 検証2701 / 2702 s | 全process s | peak bytes |
|---|---:|---:|---:|---:|---:|---:|
| 通常 | 45.102557 | 32.468759 | 5.452285 | 11.282666 / 10.719795 | 105.815842 | 1437986816 |
| レイズ後列挙 | 45.167050 | 36.465265 | 5.824585 | 11.239659 / 9.887151 | 109.363588 | 1440112640 |

peakはWindows lifetime PeakWorkingSet64を50ms間隔で読む全process測定で、
最後の未観測区間を取りこぼす可能性がある。構築、学習、評価、JSON、破棄を含む。
単発の時計を速度の信頼区間として扱わない。GCP resourceは開始していない。

## 全席・全検証seedの結果

利得の単位はbb/hand。各cellは **mean / paired SE [95% CI]**。
CIは固定候補・各席の近似pointwise区間で、席/seed全体の同時保証でもfull BR boundでもない。
異なる方式ではfitされた候補自体も異なるため、小さい検出利得だけで強い解と断定できない。

| Seed | Seat | 通常: gain / SE [CI] | レイズ後列挙: gain / SE [CI] |
|---:|---|---:|---:|
| 2701 | BTN (0) | -0.658973 / 0.108952 [-0.872519, -0.445427] | -0.912604 / 0.100445 [-1.109475, -0.715732] |
| 2701 | SB (1) | -0.729416 / 0.092253 [-0.910232, -0.548600] | -0.702509 / 0.092741 [-0.884282, -0.520736] |
| 2701 | BB (2) | -0.335503 / 0.068248 [-0.469269, -0.201736] | -0.394982 / 0.075937 [-0.543818, -0.246146] |
| 2701 | UTG (3) | -0.409147 / 0.105316 [-0.615567, -0.202726] | -0.040116 / 0.086742 [-0.210131, 0.129899] |
| 2701 | HJ (4) | -0.303935 / 0.100506 [-0.500927, -0.106943] | -0.514553 / 0.098431 [-0.707478, -0.321627] |
| 2701 | CO (5) | -1.011831 / 0.120734 [-1.248469, -0.775193] | -1.055328 / 0.119858 [-1.290249, -0.820406] |
| 2702 | BTN (0) | -0.810244 / 0.109194 [-1.024264, -0.596225] | -0.724688 / 0.100400 [-0.921472, -0.527904] |
| 2702 | SB (1) | -0.641807 / 0.091269 [-0.820695, -0.462920] | -0.610149 / 0.089610 [-0.785784, -0.434513] |
| 2702 | BB (2) | -0.204485 / 0.071082 [-0.343807, -0.065164] | -0.429334 / 0.078489 [-0.583172, -0.275496] |
| 2702 | UTG (3) | -0.431606 / 0.105538 [-0.638461, -0.224751] | -0.069984 / 0.079357 [-0.225524, 0.085556] |
| 2702 | HJ (4) | -0.494361 / 0.101647 [-0.693590, -0.295132] | -0.552470 / 0.101864 [-0.752123, -0.352817] |
| 2702 | CO (5) | -1.123985 / 0.115324 [-1.350021, -0.897949] | -1.049979 / 0.121043 [-1.287223, -0.812735] |

| Seed | Seat | baseline bb: 通常 / 列挙 | Preflop採用訪問率: 通常 / 列挙 |
|---:|---|---:|---:|
| 2701 | BTN | 0.338907 / 0.224489 | 72.101% / 73.277% |
| 2701 | SB | -0.649201 / -0.463630 | 62.501% / 64.532% |
| 2701 | BB | -0.778593 / -0.618370 | 33.771% / 44.327% |
| 2701 | UTG | 0.320253 / 0.175233 | 93.697% / 94.894% |
| 2701 | HJ | 0.125822 / 0.003217 | 88.287% / 88.390% |
| 2701 | CO | 0.214628 / 0.271356 | 80.817% / 82.436% |
| 2702 | BTN | 0.186335 / 0.178742 | 72.023% / 73.076% |
| 2702 | SB | -0.660616 / -0.594475 | 62.437% / 64.358% |
| 2702 | BB | -0.748047 / -0.605596 | 33.583% / 44.666% |
| 2702 | UTG | 0.303377 / 0.148863 | 93.507% / 95.021% |
| 2702 | HJ | 0.337502 / 0.219204 | 87.856% / 88.025% |
| 2702 | CO | 0.158783 / 0.239070 | 81.116% / 82.545% |

採用訪問率はその席の逸脱trajectory上のPreflop判断訪問数を分母とする。
unique infoset coverage・ESS・全public nodeの学習率ではない。Postflopのtrained visitsは
全席・両seedで0であり、そこでのfallbackは意図した固定continuationである。

## Fitの広さ

| Seat | 通常: 訪問key / 採用key / 訪問数 | 列挙: 訪問key / 採用key / 訪問数 |
|---|---:|---:|
| BTN | 3349 / 287 / 11448 | 3180 / 294 / 11383 |
| SB | 3310 / 342 / 11201 | 2972 / 356 / 10472 |
| BB | 3270 / 217 / 7948 | 3076 / 263 / 8367 |
| UTG | 2851 / 179 / 12694 | 2743 / 173 / 12210 |
| HJ | 2912 / 240 / 11839 | 2676 / 239 / 11306 |
| CO | 3174 / 269 / 11509 | 2882 / 266 / 11294 |

8 visitsは採用の最低回数であり、十分な推定精度を保証する閾値ではない。
独立fitとheld-outを分けても、希少branchの候補が発見できない問題は残る。
次は同じ学習profileでfit予算を増やし、未使用のheld-out seedで検出力を測る。
負利得の原因を単一の実装不具合やゲームの収束と断定しない。sample数の少なさ、
候補選択のノイズ、後続の本人方策を学びながら累積したregretなどを切り分ける。
その後、公開metadataで事前選定する複数位置・call/multiway・深いreraiseのendpoint集団を
actual-prefix / opponents-prefix別に評価し、複数学習seedと比較可能なfit強度を確認する。
研究samplerのcheckpoint/resume identityは引き続き未対応で、大規模な再開可能runへ
進む前に扱う。目標全体は継続中である。

## 検証と再現

| 検証 | 結果 |
|---|---|
| `cargo fmt --all --check` | exit 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo clippy -p cli --examples --features research-draw-abstraction,research-regret-sampling -- -D warnings` | exit 0 |
| `cargo test --workspace` | 836 passed / 30 ignored |
| `cargo test -p cli --examples --features research-draw-abstraction,research-regret-sampling` | 43 passed / 0 ignored |
| `cargo test -p multiway --features research-average-sampling,research-regret-sampling --lib` | 315 passed / 1 ignored |
| `cargo build --release -p cli --example mw_checkpoint_audit --features research-regret-sampling` | exit 0 |
| Python pilot集計器 | 12 passed、全log保持 |

専用core10件は、複数Preflop判断、own-zero reach、変更すると有利なPostflopの固定、
未採用key、負gain、current/purify variant、4097件のbatch境界、1/2/8 threads、
独立二段階のmoment計算、solver不変性、u32越え/u64 overflowを検証する。
CLIは明示予算・seedの不整合を拒否する。集計器12件は全seat/seed保持、
coverage partition、postflop逸脱、fit cutoff、NaN/CI、raw変化・説明文の許可範囲を検証する。

最終180入力のsource ZIPと同一のlive入力で全7検証を実行し、binaryを保存した。
最初のsnapshot照合では並行更新した2仕様文書だけの変更を検出した。その記録と
文書のみの再freezeも保持したが、CLI説明文を追加修正した最終版はcandidate-v3として
全検証・buildを新たに完了した。数値runは最終版の2本のみである。

| 対象 | SHA-256 |
|---|---|
| sourceManifest | `89edf90ae36556a239cf58e61d5501dc563d637469e3ab21b2de5055dbbdf35b` |
| sourceZip | `f7e73146a7aae9882661b7ba646fd1c7610f4cadfa04b38447b45e3548cb2f9d` |
| binary | `63757d48f8d2011297eb43647b326d5ba0925d28b837d3d7eac5445ac9d50596` |
| verification | `96f97f904781dc1854e3ac6d186f35d2c799bb1c70d1e9385181655e7375dc61` |
| config | `3f551c252d54d7375ec5c9d4da118b8e267aa79ce85ecdd7b85738d5dc4fc89a` |
| preexecution | `3ef362b915fdbeb6bf269d1ea3e314b72327d400a5a65d583735b65173eedd79` |
| ordinary stdout | `b47836d97c1e8c4918f08b4612f322b4940249f583ee6835719b70f54dd7f0a8` |
| enumerated stdout | `e1fa597220ff95feb2e81c77d08750cf2379f90839a99f808c4532459c3bd5f9` |
| retained summary | `ac80a7b41823dd778c342d9b56c111d6cf43ad48133c2152743f6390b3e678c8` |
| published JSON | `ae1f0f84333167e127e69fa06c37f46347a76423dd471c69d484a67b211441f1` |

retained run: `runs/whole-preflop-deviation-20260910/`。base revisionは
`93c95533dbaca2e8388e82235af5519071fd880f`、uncommitted workspace source。実行時のliteral jobs、
source/config/binary識別子、全raw output、時計・peak測定と検証logを保存している。

```powershell
python runs/whole-preflop-deviation-20260910/validate_run.py
python -m unittest tools.tests.test_summarize_whole_preflop_deviation -v
```

新規実行は保存したbinary/jobを用い、fresh output directoryを指定する。
既存の測定directoryへの上書きはrunnerが拒否する。
