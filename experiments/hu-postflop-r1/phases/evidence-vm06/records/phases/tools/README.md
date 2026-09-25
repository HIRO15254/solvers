# HU Postflop phase 計測専用 patch

T1-05 の区間分解用。製品ソースを変更せず、固定 source の**使い捨てコピー**へ
`apply_instrumentation.py` を適用する。candidate1 の既存 phase `null` は変更しない。

対象は baseline `9632d8b` と candidate v3 の `current-03.tar.gz`
（SHA-256 `ac5d493a129cd97be9322598867fa3ab5795ba6d3241a93e2719beaa2dd89970`）。
[source-versions.json](source-versions.json) の Rust/Cargo build input 全集合・hash を照合し、
全挿入箇所が一意に一致してから変更する。違う版、追加ソース、二重適用は拒否する。
元/後の全 hash と計測コード identity をコピー直下の `r1-phase-source-manifest.json` に保存する。
`lib.rs`、`solve.rs`、`sol.rs` のみを修正し、研究用 `r1_phase.rs` を追加する。
engine/formats/計算回数・処理順・設定・停止条件・`run.json` の意味は変更しない。

```sh
python3 experiments/hu-postflop-r1/phases/apply_instrumentation.py \
  --source baseline9632 --root /opt/r1/phase-baseline
python3 experiments/hu-postflop-r1/phases/apply_instrumentation.py \
  --source candidate03 --root /opt/r1/phase-candidate
# 各コピーを別 target で release build。manifest 作成後は rustfmt 等でコピーを変更しない。
# stage の親は事前に存在すること。phase.json は新規 solve 出力ディレクトリの外へ置く。
R1_PHASE_OUTPUT=/opt/r1/phase-runs/stage/phase.json \
  /opt/r1/phase-candidate-target/release/solvers solve case.toml \
  --out /opt/r1/phase-runs/stage/run
python3 experiments/hu-postflop-r1/phases/validate_phase.py \
  /opt/r1/phase-runs/stage/phase.json \
  --manifest /opt/r1/phase-candidate/r1-phase-source-manifest.json
```

既存 `R1_PHASE_OUTPUT` は上書きせず起動を拒否する。unset 時は計測時計・JSON 出力を無効化し、
同一 binary の軽量な on/off 較正を可能にする。環境変数なしでも追加の分岐は残るため、
元の非計測 binary とも比較する。計測 patch 自体がソース identity を変えるので、既存 campaign
に結果を混ぜず、新しい binary hash・source manifest を保管する。

| leaf | 挿入境界・意味 |
|---|---|
| `input_preparation` | `solve::run` の raw 読込直前から `solve_postflop` の board/range/tree 入力構築後。間の RunRecorder、schedule、pool 作成も内包 |
| `initialization` | `solve_postflop` の memory dry run 直前から loop 用時計直前。tree/rank/storage 生成、restore を含む |
| `cfr_updates` | `run_loop` の `solver.run(chunk)`。iteration ごとの時計は追加しない |
| `periodic_ev_br` | baseline `solver.exploitability()` / candidate `RootEvaluation::measure()` |
| `checkpoint` | `checkpoint_now` の writer 呼出し式。baseline の `solver.state()` clone と一時領域破棄も含む。checkpoint event 書込みは overhead |
| `final_ev_br` | postflop の最終 EV/BR 呼出しから summary 数値確定。candidate の cache 再利用と残る P1 EV 計算をそのまま測る |
| `summary_publish` | `done:` の表示開始から `print_done` / `print_summary` が `solve_postflop` へ戻るまで。最終 checkpoint・`.sol` 生成は後続の別区間 |
| `sol_preparation` | `export_sol` entry から payload 構築完了。CFV、reach、量子化、sort を含む |
| `sol_serialization_and_write` | `write_sol` 呼出し全体。validation/encoding/compression/checksum/fsync/persist を含み、純ディスク時間ではない |
| `overhead` | CLI parse、ログ/metrics、停止判定、`.sol` 後処理・drop、run.json/manifest 発行等の残り |

基準位置は baseline `solve.rs:563/565/645/817`、`sol.rs:199/318`、
snapshot03 `solve.rs:621/623/708/893`、`sol.rs:207/308`。計測のための処理移動・再評価はない。
baseline の重複最終評価・重複最終 checkpoint・state clone は保存される。

`Instant` を起点とした相対 nanosecond の半開 leaf 区間を一本の timeline に記録する。
leaf は連続・非重複で、合計は `total_ns` と一致する。Rayon worker と CLI の双方が同じ
process 内の時計・短時間 Mutex を共有する。計測の lock/記録操作には微小なバイアスが残る。
`iteration` は更新区間では更新後 iteration、評価・保存では対象 iteration を示す。
`cfr_inclusive_envelope` は最初の更新開始から最後の更新終了までの派生区間で、**leaf 合計へ
加えない**。最後の更新後に実行される定期 EV/BR/checkpoint はその envelope の外にある。
これは [R0 の区間仕様](../../../../docs/plans/hu-postflop-r0/measurement-protocol.md#4-時間と資源の区間)
に対応するための境界情報であり、既存 `wallSecs` を純 CFR 時間に読み替えない。

`total_ns` は `main_impl` entry から dispatch return まで。phase 初期 JSON 書込みは overhead、
最終 phase JSON 書込みと process 起動/終了は範囲外。外部 supervisor の全工程時間と別に扱う。
初期 snapshot は `status=running`、時間は null。通常の成功と Rust `Err` では最終 snapshot を
atomic に保存する。エラー区間は `status=error` とし、完了区間に数えない。計測ファイル失敗で
元の command error を置き換えず、stderr に併記する。成功後の計測保存失敗は command を失敗にする。
強制 kill、panic、Clap の直接 exit では初期 snapshot しか残らず、区間は**欠測**。推計しない。
毎チェックの記録 flush/fsync は追加しない。resume・他ゲームの完全な区間定義は本 patch の対象外。

新 campaign は固定 River/Turn/Flop と品質・停止設定をそのまま使い、同一 VM の逐次・交互実行で
両版を測る。計測 on/off でも終了 iteration、live NC、公開 tree/strategy/EV を照合する。
保存 profile の BR 未評価、サンプリング RSS の欠測、native RSS の pre-exec floor は解消しない。

ローカル確認は `python -m unittest discover -s experiments/hu-postflop-r1/phases -p 'test_*.py'`。
両固定コピーへの適用・hash・二重適用拒否・一意置換・validator を検査する。candidate の archive
がない環境ではその適用 test を明示 skip する。release build・実測は GCP で別途行う。

## 固定 campaign を再利用する wrapper

[run_phases.py](run_phases.py) は snapshot03 の `run_campaign.py`
（SHA-256 `39cd41ddf3ba9cea9247a12093dc509cb3ce299c9a31ee5f3e97e04ce6e8030e`）を import する。
既存の品質判定、summary/profile export、completed-cap resume、supervisor の timeout・資源停止・cleanup
経路は同じ。新しい pilot は実行せず、元 v3 plan の config/iteration/target/cadence/limits/host/order を
そのまま継承する。元 plan と pilot の identity、および元 binary/source の identity も派生 plan に残す。

```sh
# 以下の既存 plan/binary/source-copy パスは VM の実際の配置を指定する。
python3 experiments/hu-postflop-r1/phases/run_phases.py freeze \
  --original-plan /opt/r1/v3-plan/plan.json \
  --baseline-binary /opt/r1/phase-baseline-target/release/solvers \
  --baseline-manifest /opt/r1/phase-baseline/r1-phase-source-manifest.json \
  --candidate-binary /opt/r1/phase-candidate-target/release/solvers \
  --candidate-manifest /opt/r1/phase-candidate/r1-phase-source-manifest.json \
  --out /opt/r1/phase-plan.json
python3 experiments/hu-postflop-r1/phases/run_phases.py run \
  --plan /opt/r1/phase-plan.json --mode on --out /opt/r1/phase-on
python3 experiments/hu-postflop-r1/phases/run_phases.py run \
  --plan /opt/r1/phase-plan.json --mode off --out /opt/r1/phase-off
python3 experiments/hu-postflop-r1/phases/run_phases.py analyze \
  --plan /opt/r1/phase-plan.json \
  --on /opt/r1/phase-on/comparison.json --off /opt/r1/phase-off/comparison.json \
  --original /opt/r1/v3-paired/comparison.json --out /opt/r1/phase-calibration.json
```

freeze は source copy の全 `after` hash と両版の同一計測 identity を確認する。
binary/source_evidence のみを計測 binary/manifest に置換し、`instrumented_pilot_performed=false`
を明記する。元 runner と supervisor の identity を保ち、wrapper・validator・applier・module template・
source versions の hash を追加する。固定後にこれらを編集しない。元 plan/config/pilot は元パスに保持する。
各 stage の前後で identity を検査し、追加の identity files も supervisor の監視対象へ渡す。

`on` は **solve argv の stage だけ** `R1_PHASE_OUTPUT=<stage>/phase.json` を設定する。
summary/tree/strategy/EV export と resume は常に unset、各 stage の `finally` で元の環境を復元する。
これにより `perform_solve` 内の summary subprocess が同じ phase path を再作成する事故を避ける。
`off` は全 stage で unset。各 mode は River/Turn/Flop に baseline/candidate を交互 3 回ずつ、計 18 solve。
追加の wall 上限や資源枠は作らず、元 plan の有限 campaign window と各 stage 上限を継承する。
元 supervisor の異常終了・cleanup 未確認を wrapper が成功に変換することはない。

成功した on solve は [validate_phase.py](validate_phase.py) で区間・合計・source identity を検査する。
各 stage の `phase-validation.json`、各 campaign の `phase-analysis.json`、campaign directory の兄弟
`<directory-name>.phase-session.json` を保存する。失敗・中断も session に記録し、部分結果は completed
campaign と区別する。元 `comparison.json` 内の `phase_timings` と pipeline analysis の欠測表示は不変。
新しい phase 値は別 analysis と `solve.r1_phase` にのみ追加する。

calibration analysis は元 VM や binary の存在を要求せず、固定された wrapper・validator・計測ファイル・
source manifest・元 plan/pilot の hash を再照合してから validator を読み込む。
identity 内の絶対 path と保存ファイルの配置は維持する必要があり、別 path への移設には対応しない。
各 case/version/repetition の on/off/original の iteration・live NC・summary・
公開 tree/strategy/EV hash・run config を照合する。on phase は raw JSON を再 hash・再検証する。
成功レコードは `overhead` を含む既知の全 leaf が必須で、未知の leaf・欠測は拒否する。
集計時も同じ集合を要求し、欠けた leaf を時間0・呼出し0で補わない。
各 leaf の nanosecond、呼出し回数、内部 total、外部 solve 時間を median/min/max と全値で保存し、
on/off と off/元 binary の時間比も記録する。全判定が通らない場合は eligible にしない。
mode ごとの別 campaign なので、cache・実行順・VM の揺らぎも差に含まれる。比を純粋な計測 overhead
や性能改善の保証と呼ばない。外部時間から内部時間を引いた値も新しい実測 phase ではない。

wrapper の模擬検証は、元 runner を使って original/on/off 各 18 solve と付随 export/resume を動かし、
環境変数の範囲・復元、同一出力照合、phase null の維持、欠測成功の拒否、supervisor 異常の伝播を確認する。
