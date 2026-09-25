# HU Postflop 初回ローカル実行票（SOL-5 / R0-05）

[R0作業票](../r0-execution-plan.jp.md) §7の研究用実行計画。現行CLIの契約・既定値や品質閾値は変更しない。通常規模のローカル計算は許可済みで、R1の開始を有料資源の支出判断へ結び付けない。R0では参照候補の基本条件までを確認し、全数値取得・固定fixture・本格計測はR1へ渡す。[候補台帳](cases.csv)、[資産対応表](asset-map.md)、[測定仕様](measurement-protocol.md)、[source/host記録](provenance.md)を併用する。

## 最初の3件と着手条件

| 順 | case | 選定理由 | 開始前にR1で確定すること |
|---|---|---|---|
| 1 | `HU-R0-019`：4bet後のRiver `Qs7h2c4d9s`、pot 40.5bb、残55bb、75bb Simple NL500 | 日常8件に含まれる唯一のRiver。狭い4bet到達rangeとRiver開始で初回の小さい木を狙う。**木サイズ・所要量は未測定** | T1-01が両seatのcombo range、参照solutionのrake/utility、開始pot・残stack、Riverの全bet/raise/all-in menuと丸め、EV基準・版を固定。T1-05が監視・停止器を準備し、configのtree見積りを確認 |
| 2 | `HU-R0-007`：20bb SRP Turn `As7d2c4h`、pot 4.5bb、残18bb | 日常の浅いTurnで、River chanceと継続を追加して資源増を測る | T1-01でTurn以降の全menu・range・rake条件を固定し、1件目の全工程時間/RAMを踏まえて枠を再設定 |
| 3 | `HU-R0-008`：20bb cEV Single Size Flop `QsJsTd`、pot 5.5bb、残17.5bb | 浅いstackとrakeなしでFlop→Turn→Riverを測る。Single Sizeの動的menu差は未解消 | T1-01で全継続menu・range・card removalを固定。現行入力で表現できない差はT2-01へ送り、代用menuを同一ゲームとして扱わない |

この順序は「Flop開始も末端まで解く」[計算範囲](coverage.md)に従う。初回候補が条件未取得・表現不能・事前見積り超過なら、`HU-R0-002`（標準SRP River）を次候補とし、それも過大なら同じfixtureからrange/menuを縮めた**別IDの資源診断用ケース**を作る。元caseの同一ゲーム比較と混同しない。変更理由、元/代替ID、差分、承認済み資源枠を実験記録に残す。数分という高速近似の努力目標を厳密CFRの合否に使わない。

T1-01の実施者が参照値・固定fixtureとHU TOMLを作成し、別担当または同実施者の監査で`solvers validate --write-effective`の出力、両range・rake・全menuを照合する。`tree.source`付きraw configのdirect solveは現状、保存後の読込に失敗する[既知差分](asset-map.md#33-外部tree-sourceの保存差分と実行可能な回避手順)がある。解消前は**正規化済みeffective TOML**をsolveへ渡す。T1-05の実施者が資源枠、監視、実行と計測・保存後確認を担当する。

## 初回pilot枠と停止

[準備時host記録](../../../experiments/hu-postflop-r0/readiness/host.json)（2026-09-25）はi7-10700KF、8物理/16論理core、物理RAM 34,275,098,624 byte（31.92 GiB）、空き7,227,535,360 byte（6.73 GiB）、C:空き約78.2 GiB。**solve直前値ではない**。同時実行1件、build/testとの同時稼働も避け、初期`[run] threads = 8`（`min(16, 8)`）とする。初回の全工程はprocess起動から終了・成果物読戻しまで**600秒を観測枠**とし、内部CFRには例として`[run] max_time = "8m"`、`check_every = 25`、`[run] iterations = 1000`を初期の有限予算として明記する。8分は保存等の余白を設ける暫定値であり、600秒の保証ではない。`target_nash_conv`はT1-06の基準確定前に合格閾値として設定しない。HUのthreads/timeはTOMLの`[run]`で指定し、Multiway専用の`--threads`/`--memory`/`--max-time`を使わない。

RAMの初期**観測・停止トリガー**は`min(物理RAM×0.50, solve直前の空きRAM×0.75)`。準備時の値を仮に代入すると`min(17,137,549,312, 5,420,651,520) = 5,420,651,520 byte`（約5.05 GiB）だが、実行直前に[source/host記録](provenance.md#host取得と欠測)の手順でRAM・pagefile/commit・出力volume空きを再取得し、実値から再計算する。明示されたVM/container/job上限があればさらに小さい値を採用する。pagefile余力不足で並列compileが失敗した[既往](../../../experiments/hu-postflop-r0/readiness/README.md)を踏まえ、build/testは`CARGO_BUILD_JOBS=1`で順番に行う。現行`solve`の`storage`見積りはprocess起動後に出るため、その表示だけでは起動前の入場判定はできず、process peakでもない。T1-05が別の安全な事前見積り手段か上限付きpreflightを整える。推定が枠へ近い、空きdisk不足、または監視器が動かない場合は本計算を開始しない。まずcase・menu/rangeを診断用に縮めるか、値と根拠を記録して枠を再設定する。

現行コードでは`holdem::memory_usage`の全木dry run、game/tree/storage構築の**後**に`run_loop`の時計が始まる。`max_time`判定は`check_every`ごとのCFR・定期BR・checkpoint後で、実行中の長いchunkも止めない。`summary.wall_secs`は概ねloop区間で、最終EV/BR、最終checkpoint、`.sol`生成・保存、process終了を含まない（[solve.rs](../../../crates/cli/src/solve.rs)、[測定仕様 §4](measurement-protocol.md#4-時間と資源の区間)）。初期化、最終BR、保存を別区間として計測し、全工程を外部単調時計で見る。RAM見積りも強制上限ではない。

T1-05は**初回solveに先立ち**Windowsの外部supervisorを実装・小さいtoy runで較正する。process起動前に単調時計を開始し、PID/子processを追跡して1秒以下の間隔で`PeakWorkingSet64`（Windows peak working set）と現在working setを取得する。開始・終了時刻、取得間隔、欠測、子processを含めたかを記録する。観測値は真のpeak RSSと同義とせず、超過検出後の停止にはpoll間隔分の遅れがある。600秒到達、採用RAMトリガー超過、空きdisk/commitの危険、または手動停止時は理由・直前測定を記録し、まずforeground CLIへCtrl-Cを送ってcheckpoint境界での協調停止を待つ。猶予内に止まらない、初期化中で協調停止できない場合は対象process treeを強制終了し、残ったcheckpoint/manifestの整合を検査する。WindowsでCtrl-Cを対象PIDへ確実に届ける方法、強制終了猶予、子processと保存中断の扱いもsupervisorの試験で確定する。**現時点で全工程の自動停止やRAMのハード上限は未実装**であり、値を書くだけで制御済みとみなさない。OS job objectによる強制RAM上限が必要ならT1-05で追加する。ユーザーの手動Ctrl-Cも`check_every`境界での協調停止であり、即時停止ではない。

時間/RAM超過時はrunを`timeout`/`resource_exceeded`、`quality_status=not_evaluated`とし、内部停止・外部停止・強制終了を`stop_reason`に区別する。収束していないrunを合格扱いせず、checkpoint再開可否を検査する。次回はrange/menu/反復数を診断用に縮めるか、実測の初期化・CFR・BR・保存時間とpeakから枠を再設定する。教師の大量生成はこの枠に含めない。

## 記録・完走後確認・拡大計算表

各runの生出力は`runs/<run-id>/`へ置く。採用するconfig、source/host/binaryのmanifest、supervisorログ、結果、validator、hashは`experiments/hu-postflop-r0/<experiment>/`へ残して[実験索引](../../../experiments/README.md)に追加する。`target/`へ研究成果を置かない。[記録時点](provenance.md#1-caseの記入時点と不変性検査)に従い、build前のsourceとconfig hash、build直後とsolve直前のbinary hash、solve直前のhost/枠、終了後の再照合を記入する。実行後にrun manifestのstate/command/configHash、process終了code、`run.json`・`checkpoint.ckpt`・`solution.sol`のiteration、保存後の`export summary`/EV読戻し、条件照合と測定ログを確認する。中断時は残存ファイルを成功証拠にしない。

| 集計単位 | 件数 | 全工程時間の見積り | RAMの見積り（同時1件） |
|---|---:|---|---|
| 日常：River/Turn/Flop | 1 / 3 / 4 = 8 | case別実測/見積り`Σ t_i`。street代表値だけなら`1t_R + 3t_T + 4t_F` | case別`max(r_i)`。代表値なら`max(r_R,r_T,r_F)` |
| 拡張：River/Turn/Flop | 6 / 6 / 12 = 24 | case別実測/見積り`Σ t_i`。代表値だけなら`6t_R + 6t_T + 12t_F` | case別`max(r_i)`。代表値なら`max(r_R,r_T,r_F)` |

`t_i`は入力準備から保存読戻しまでの秒、`r_i`はprocessのWindows peak working set byte。未測定caseは`null`のままにし、代表値による外挿は**粗い計画値**として範囲と根拠を付ける。特に4bet Riverの実測をSRP Flopへ比例換算しない。日常8件は拡張24件に含まれるため、両suiteを別日に走らせない限り32件として加算しない。各caseの開始/終了、初期化・CFR・最終BR・保存、peak、停止理由、tree見積りを記入し、最も遅いcase/最大RAMと再試行率を含めてローカル所要時間・必要diskを更新する。枠内で全工程が完走し、保存後確認と参照条件が通ったケースからTurn→Flop→日常8→拡張24へ進む。品質の`pass/fail`はT1-06の事前固定閾値とvalidatorが揃ってから判定する。

ローカル枠を超えるcaseが残る場合だけ、[2026-09-20のクラウド概算](../../research/solver-cloud-cost-estimate-2026-09-20.jp.md)を更新する。必要なcase数・再試行率・実測の全工程秒/peak・保存容量を先に出し、候補VMでのメモリ適合と実測または小規模試行のthroughputを確認する。次に現行の一次料金（VM、disk、保存、転送、税等）を再確認し、`VM稼働時間×現在単価＋その他費用`と予備枠を算出して有料実行の判断に渡す。旧$55〜200等は当時の仮定に基づく参考額で、現在の必須支出・実測費用ではない。

## 本票の検証（2026-09-25）

`python tools/check_docs.py`（40 Markdown files）、`cargo fmt --all --check`、`CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets -- -D warnings`、`CARGO_BUILD_JOBS=1 cargo test --workspace`、`git diff --check`は成功。通常testのignored caseと本格solver計測は実行していない。この検証は実行票の整合確認であり、pilotの時間/RAM・品質の実測証拠ではない。
