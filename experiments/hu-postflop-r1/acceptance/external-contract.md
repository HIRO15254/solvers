# 将来の外部比較：校正記録と判定器

[external-compare.py](external-compare.py)は、**今後の比較前**に条件・baseline校正・明示的な閾値を固定し、
同じ有限ゲームのroot seat EVと内部BRを判定する小さなoffline経路である。
solver、参照UI、外部サービスを実行しない。新しい数値閾値・CLI既定値は導入しない。
既存の017/019診断も、既存のR1総合判定も変更しない。

この機構の完成だけで、参照条件が揃ったことやT1-06完了を認定できない。
局所の`quality_status=pass`が出ても`overall_r1_acceptance=not_evaluated`を維持する。

## 入出力

```text
python experiments/hu-postflop-r1/acceptance/external-compare.py freeze --evidence-root /path/to/evidence --calibration /path/to/calibration.json --out /path/to/new-threshold.json
python experiments/hu-postflop-r1/acceptance/external-compare.py compare --evidence-root /path/to/evidence --threshold /path/to/new-threshold.json --candidate /path/to/candidate.json --out /path/to/new-result.json
python experiments/hu-postflop-r1/acceptance/external-test.py -v
```

`freeze`は実際のUTC発行時刻・実行中validatorのSHA-256・校正入力を記録し、全内容のhashを版IDにする。
比較はそのvalidator hashと版を確認し、発行より後に開始したcandidateだけを対象にする。
candidateの完了と閾値発行は比較実行時刻以前であることも要求し、CLIの判定記録には`compared_at`を残す。
日時だけを書き換えると版hashが不一致になる。APIの時計引数はtest用であり、CLIにはbackdate optionを設けない。
ただし署名・第三者タイムスタンプサービスではないため、人が記録全体を偽造できないという保証はない。
外側の実行provenanceも保持する。

出力pathは新規のみ。exit 0は閾値発行または当該比較scopeのpass、1はfailまたはnot_evaluated、
2は矛盾・未知field・hash不一致などの無効記録。JSONのquality欄と理由を確認し、exitだけで総合認定しない。
不足・未確認・timeoutは`not_evaluated`、成立した比較の数値超過だけが`fail`。
既存のthresholdなしで017/019の`observed.json`を`--candidate`へ渡しても、
`prospective threshold_version unavailable`として未評価になる。これは校正の実演ではない。

全FileRefは`{path, bytes, sha256}`。pathは`--evidence-root`内の相対pathで、各参照を実際に再hashする。
JSONは2MiBまで、参照fileは64MiBまで。範囲外path・未知field・重複JSON key・非有限値を拒否する。
metricの数値は**十進文字列**で記録し、nullを0へ補完しない。
各記録のcase ID、精度の説明、閾値の採用理由は空白だけでない文字列が必要で、配列やbooleanを受け入れない。
case/resultのcanonical hashはscriptの`encode()`による内容identityで、raw file hashと同一とは限らない。
原FileRefと元入力ファイルも別途保持する。

## 入力契約

完全な人工fixture構築例は[external-test.py](external-test.py)の`fixture()`にある。
そこにある数字とproofはunit test専用で、実験用thresholdの推奨値でも実測でもない。
実caseの入力を自動的にこの例で埋めない。

| 記録 | schema / 必須内容 |
|---|---|
| calibration | `r1.external-calibration/v1`。case ID、`basis=baseline_only`、`candidate_results_used=false`、4入力FileRef、対象profile、seat別root EV margin、内部NC/seat gain条件、採用理由とその証拠 |
| conditions | `r1.external-conditions/v1`。`condition_match=confirmed`、承認時刻/証拠、missing/differenceが空、own/referenceのcanonical有限ゲームFileRefが同一hash、下記8checkとscope |
| reference | `r1.external-reference/v1`。case/game hash、unit/EV基準、観測時刻、個別solution版と精度の証拠、joint reach、seat別EVと表示丸め |
| baseline / candidate | `r1.external-evaluation/v1`。role、正常完了区間、source/binary/effective config/numeric model FileRef、game hash、unit/basis、評価したprofile、reach、EV/BR/gain/NCと表示丸め |
| correctness | `r1.external-correctness/v1`。baseline record/source/game/numeric modelのhash、完了時刻、独立ルールEV/BR・同じ有限ゲームのBR・数値誤差上界・storage roundtripの4check、seat別誤差上界と証拠 |

条件checkは`variant_seats_units`、`history_board_pot_stack`、`both_ranges_card_removal`、
`full_continuation_tree`、`rake_utility_settlement`、`abstraction_recall`、
`ev_basis_conversion`、`reference_version`。各々`{status: pass, evidence: FileRef}`が必要。
正当性checkも同じ形で、名前はscriptの`CORRECTNESS`定数に固定する。
current diagnosticのroot menuや表示桁だけを、全木確認・個別solution精度の証拠にしない。

scopeはNLHE / `[OOP, IP]`、`utility_unit=chips|prize`、
`ev_basis=subgame_start_utility`、開始pot、chips/BB、
`economics=constant_sum|general_sum`とconstant utility sum（一般和ではnull）を持つ。
EVを変換する場合は、変換済み値だけでなく変換前と式を条件証拠に残す。
FileRefの同一hashは、承認されたcanonical有限ゲームが一致するという入力条件を検査するもの。
このscript自身がGTO Wizardの木を解析したり、証拠の文章からその正しさを証明するわけではない。

## 閾値と算術

- `root_ev_margins`はOOP/IPそれぞれ`{operator: <|<=, value: decimal-string, unit}`。
  比較対象は**変換後raw EVの絶対差**。seat間の符号相殺も、丸め下限への無断置換もしない。
- `internal_quality.nash_conv`も同形式。一般和では`seat_gains`に両seatの明示条件を追加する。
  定和では`seat_gains=null`。0.1% / 0.02%やNC 0.04をこの経路の既定値にしていない。
- 生のgain/NCはJSON round-tripしたf64で`gain=BR−EV`、`NC=Σgain`を照合する。
  評価側のEV/BR/gain/NCは`Decimal(value) == Decimal(str(float(value)))`を満たすことを要求する。
  f64で消える十進末尾を使って厳密な閾値を回避できない。参照値・閾値は元のDecimal精度を保持する。
  gainの負値をclampしない。校正されたseat別数値誤差上界を超える負値は比較不能として記録する。
  この検査はbaselineの閾値発行前とcandidateの比較時の両方で行う。
- 定和certificateがある場合だけ`Exploitability=NC/2`。chip utilityだけ開始pot比を出す。
  一般和ではexploitability欄はnull。prizeをchip potで割らない。
  定和時には両seatの表示区間を加算し、別途与えた数値誤差上界の和だけ上下へ広げた区間に
  証拠のconstant sumが入るかをbaseline発行前・candidate比較時に照合する。
  非対称区間を対称な半径へ置き換えない。これは全terminalの定和性の証明ではない。
- 表示丸めは`nearest`と明示quantum、または`explicit_interval`のlower/upper。
  quantum 0は表示丸めなしという記録であり、浮動小数演算誤差0という意味ではない。
  切り捨てなどをnearestと仮定せず、未知なら未評価。
- 表示区間が離れる最小距離を診断として併記する。nearestなら
  `max(0, abs(ΔEV)−q_ref/2−q_own/2)`に一致する。
  数値誤差上界は別欄に保持し、EV許容差や内部残差から自動生成・差引きしない。
- baselineの`joint_reach_mass=0`はroot EVを校正できないため閾値を発行しない。
  参照またはcandidateの`joint_reach_mass=0`はexternal EVを`not_applicable`、全体を`not_evaluated`とする。
  欠測したEVや精度は0にしない。頻度・hand/action比較は未実装と明示し、root比較のpassに含めない。

この根拠は[測定仕様 §2–5](../../../docs/plans/hu-postflop-r0/measurement-protocol.md)の
profile区分、seat別EV、表示丸め、zero reach、比較前の版固定、未校正の扱いである。
特に外部EV許容差を内部exploitabilityから導出しない。

## 校正証拠の信頼境界

校正はbaseline-onlyであることを明記し、その完了後・candidate比較開始前に発行する。
同じ入力の複数評価やstorage差の測定だけで、未証明の「真の数値誤差上界」が自動的に得られるとはしない。
上界の方法・対象・前提と個別参照精度はreview済み証拠として与える必要がある。
曖昧なapproval flagだけで参照条件を新たに承認するための道具ではない。

`numeric_model`は使用した数値評価実装・表現・前提を指すreview済みmanifestである。
candidateはbaselineと同じnumeric modelであることを要求する。IO変更でもsource全体hashは異なり得るが、
数値方式が変わった場合にbaselineの誤差上界を流用しない。別の校正/基準版へ分ける。
source/binaryが本当にそのmanifestからbuildされたことは、外側のbuild証拠でも確認する。

このscriptができるのは、与えられた証拠のidentity、指定した前提、時系列、算術、判定gateの照合まで。
承認内容の真偽や全suiteの代表性は別レビューであり、root seat EVの一致だけを
全hand/actionの一致、全ゲームの正当性、外部24条件やR1の総合受入へ拡大しない。
