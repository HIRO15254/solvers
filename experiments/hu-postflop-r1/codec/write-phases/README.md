# SOL writer の区間計測準備

これは **未実行の研究用計測パッチと前向きプロトコル**である。
[VM07 codec 結果](../vm07-report.md)の Flop 保存中央値
303.004095 → 367.827635 ms という観測を保持したまま、次の実験で区間を切り分ける。
原因を認定した資料でも、保存高速化・R1合格の証拠でもない。
production、旧96標本、旧runner、入力、quality targetを変更しない。
本準備は追加クラウド支出・VM起動の許可を与えない。

## 固定対象と適用

baselineは`88ffa5dd4583e5c8bae84e20e9b8390cb719e4f0`、candidateは
`2fecc099b9911511a0938fb2700bbb124bc1046e`。
[source-pins.json](source-pins.json)は両Git revisionのCargo workspace/cratesを
それぞれ195/197ファイルとしてbyte数・SHA-256で固定する。
`sol_indexed.rs`は両版で同一の18,669 bytes、SHA
`100864cc30b62de759b46161e6c1eec02ec8c4e27d8b20a3679cb8cfa8b91d7e`。
baselineには旧計測と同じexample（SHA
`ed972ffce351fcd31dfbab74771a06a059ef0edb43e44d65397e65f9a85cbcea`）だけを加える。
CRLFへの変換を含め、元byteがpinと異なるsourceは拒否する。

[apply.py](apply.py)は元ディレクトリを読み、新しい別ディレクトリにのみ書く。
`--out`は未存在で、元sourceの内側・祖先ではならない。
Cargoファイルとcrates以外のdocs/実験証拠はコピーせず、compiled-source scopeをmanifestに記す。
未知のcratesファイル、変更された元byte、重複/欠落anchorは適用前に拒否する。
instrumented版はwriterの元関数をそのまま残し、研究専用関数を追加する。
新copyの変更は`sol_indexed.rs`、`lib.rs`の研究専用export、同じexampleのみに限定する。
`instrumentation.patch`と`r1-write-phase-source.json`に前後全file identityを残す。
instrumented copy作成時は明示した`--rustfmt`で追加Rustを整形し、その実体hash/versionと整形後byteも
manifestへ記録する（Cargo buildではない）。元writerのprefix byteが整形で変わった場合は拒否する。
`--rustfmt`は`rustup which rustfmt`等で得るtoolchain実体を指定し、argv0に依存するrustup proxyは拒否する。
copy後にfmtや手編集した場合はこのafter manifestが無効になるため、そのまま測定に使わない。

```text
python experiments/hu-postflop-r1/codec/write-phases/apply.py --source CLEAN-BASELINE --role baseline --mode plain --example FROZEN-sol_codec_bench.rs --out NEW-BASELINE-PLAIN
python experiments/hu-postflop-r1/codec/write-phases/apply.py --source CLEAN-BASELINE --role baseline --mode instrumented --example FROZEN-sol_codec_bench.rs --rustfmt RUSTFMT-EXECUTABLE --out NEW-BASELINE-INSTRUMENTED
python experiments/hu-postflop-r1/codec/write-phases/apply.py --source CLEAN-CANDIDATE --role candidate --mode plain --example FROZEN-sol_codec_bench.rs --out NEW-CANDIDATE-PLAIN
python experiments/hu-postflop-r1/codec/write-phases/apply.py --source CLEAN-CANDIDATE --role candidate --mode instrumented --example FROZEN-sol_codec_bench.rs --rustfmt RUSTFMT-EXECUTABLE --out NEW-CANDIDATE-INSTRUMENTED
python experiments/hu-postflop-r1/codec/write-phases/test_write_phases.py -v
```

このpackageのRust build・実機計測は未実施。適用/負例のPython検査はRust型検査や
syscall動作検証の代わりにならない。[準備検査の記録](preparation-checks.json)には
14 Python tests、documentation checkerの8 tests、現workspaceのfmt check成功を残す。
root担当の実行前buildでは、Rust/Cargoの実体hash、
同一release flags/toolchain、4つの別fresh target、成功ログとsource manifestを保持する。
専用[run.py](run.py)にfreeze/有限runnerを実装した。偽実行テストのみで、Linux実環境での
preflight・build・実測は未実施。[protocol.json](protocol.json)の全`required_before_execution`を満たすまでは実行しない。
既存`run_codec.py`のschema/96標本条件を変更して流用しない。

## 区間の意味

[runtime.rs.inc](runtime.rs.inc)は`Instant`の整数nanosecondsを使う。
同一処理の呼出し順・codec bytes・group境界・zstd設定・同期を保ち、追加の圧縮bufferを作らない。
通常のON実行はexampleのwriter直前に`R1_SOL_WRITE_PHASE_OUTPUT`を読み、研究関数を呼ぶ。
OFFは同じinstrumented binaryで環境変数を**unset**にして元writerを呼ぶ。
plainは無変更binaryで、同じくunsetにする。空文字はOFFではない。

| leaf | 元writerの境界・含む仕事 |
|---|---|
| `validation` | meta/block validationの全pass |
| `size_and_group` | 全pairの`serialized_size`とgroup分割・group配列作成。一要素ごとのclockは入れない |
| `metadata_prepare` | config/meta cloneとmetadata struct作成 |
| `serialize` | metadataと各groupの`postcard::to_allocvec`。raw全buffer確保を含む |
| `metadata_compress` | metadataの`zstd::encode_all`。メモリ上の圧縮先Vecを含む |
| `temp_create` | 元の`NamedTempFile::new_in` |
| `pair_refs` | 各groupの既存borrowed pair Vec作成 |
| `hash` | header config hash、metadata raw hash、各group raw hash |
| `chunk_compress_excluding_file` | 各groupのencoder new/checksum/write_all/finish包絡時間から、内部File write/flushの実測時間を引いた値 |
| `file_write` | header/group/directoryが実際に呼ぶFile `write`/`write_all`/`write_vectored`/`flush`。同じmethod・同じbyte sliceを転送 |
| `file_seek` | File `seek`/`stream_position`。専用overrideを保つ |
| `sync` | 元のFile `sync_all` |
| `persist` | 元のtempfile `persist`とそのerror変換。返されたFileのdropは未分類側 |

圧縮のinclusive包絡は別fieldに残すがleaf合計に加算しない。
`compression_envelope_ns = compression_nested_file_ns + chunk_compress_excluding_file.ns`。
内部File時間は既に`file_write`に含まれる。引算はchecked、負値をclampしない。
各groupの実際に返されたwrite byte数と`stream_position`差も一致させる。
write/flush試行数、成功byte数、エラー数を保持する。通常Err中のwrite_all部分成功byte数は
正確に取得できず、未完了データで性能判定しない。

差分は**File呼出しを除く圧縮区間**であり、純CPU圧縮時間ではない。
encoder割当・tap bookkeeping・scheduler待ちを含む。File呼出し時間もpage cache/writeback
待ちを含む壁時計で、physical disk時間と呼ばない。zstdの`finish`中の末尾出力もtapに入る。
flush追加、buffer変更、clone削除、reserve追加、圧縮方式変更はしていない。

内側totalは計測用配列初期化後からwriter本体returnまで。leafの外の条件分岐・範囲検査・
entries操作・BufWriter内のbuffer操作・raw/pair/File drop・clock/bookkeepingは
`unclassified_ns = inner_total_ns − sum(leaves)`として**差分**を残す。
これは未計測工程の推計配分でも強制的なゼロでもない。
外側parentはexampleの実呼出しを囲み、研究用初期化/終了処理も含む。
`sum(leaves) + unclassified_ns == inner_total_ns <= parent_total_ns`を必須とし、
親との差も表示する。phase JSONの生成/同期/atomic publishは両timerの終了後に行う。
それによるプロセス時間・後続sampleへのIO影響は消えないため、下の校正対象に含める。

通常Errでもphaseは`outcome=error`、元writer errorは先にstderrにも残す。
出力は新規only・atomic persist。出力先失敗、panic、kill、timeout、preemptionでは
phaseが存在しないことがある。欠測を0へ置換せず、`not_evaluated`としてraw証拠を保持する。

```text
python experiments/hu-postflop-r1/codec/write-phases/validate.py --phase NEW-PHASE.json --sample RUN/output/result.json --rewritten RUN/output/rewritten.sol
```

[validate.py](validate.py)は単一ON成功記録の区間整合を検査する。
全leaf、型、call数、group数（SOL v3 header）、byte count、親timerとの一致を要求する。
OFF/plainではphaseは「計測なし」であり、0ではない。
このvalidator単独ではsupervisor・source/host・canonical・campaign完了を認定しない。
それらは専用runner/retained checkerの必須責務として残す。

## 有限の前向き手順と校正

入力は[既存inputs.json](../inputs.json)のsource06 Full River/Turn/Flopをそのまま使用する。
既存source06品質値やVM07結果を書換えない。各caseは両codec×plain/off/onの6 arms。
1回ずつ除外warmup後、固定6blockの均衡順を使う。各armは各順番に一度現れ、総計126process。
同じplain/off/onのbaseline/candidateを隣接させ、codec順はblockごとにB/CとC/Bを交互にする。
1process 60秒、campaign全体1200秒、4 GiBメモリ上限、free RAM 2 GiB・disk reserve 4 GiB。
約5 GiBのcanonicalを含む新出力を見込み、開始前にその分とreserveを確保する。
先着失敗でabortし、retry・標本置換・反復追加・入力変更をしない。

実際の4 binariesとsource-copy manifest、patch/template/validator、入力、compiler、
supervisor/Python、CPU model/features・boot・OS/kernel・filesystem/mount・outer cgroupを
warmup開始前にhash/identity付きでfreezeする。毎stage前後にsource/binary/inputとCPU/bootを再照合する。
native targetをCPU/boot変更越しに再利用しない。失敗時もprocess-tree cleanupを完了させる
既存supervisorと外側cgroup/deadlineが必要。全126sample中はSCP・回収・圧縮・build・他の実験を行わない。

入力は直前SHA/BLAKE3と各codec自身の`read_sol`でwarmになる。cold disk計測は行わない。
preloadのresident Vec配置が両codecで異なりうるので、対象は「各codecのpreload後のwriter」。
canonical出力・readbackはtimer外だが後続sampleのcache/writebackへ影響しうる。
全6条件で出力・canonicalの完全一致と通常full readbackを必須とする。

性能比較の主結果はplain同士のみ。各case/codecの`median(off)/median(plain)`と
`median(on)/median(off)`を6個のraw paired blocksと共に別々に公表する。
いずれも事前固定の`[0.95, 1.05]`内の場合に限り、そのcase/codecの区間を記述的な帰属に使う。
外れた場合はduration/overheadを表示するだけで帰属は`not_evaluated`。
この±5%は新実験の計測摂動予算であり、統計的同等性・外部精度・R1受入閾値ではない。
測定値をoverhead比で補正しない。plain/off間でcode layout、on/off間でclock/IO tapの影響を点検する。
全条件で中央値/min/max/各block差を残し、VM07の悪化を今回の良い標本で置換しない。

phase-specific RSS、cold storage、physical disk、solver性能、保存BR、外部参照品質、R1全体の合格は
この計測から導かない。性能が変わっても、旧Flop回帰の原因が特定されたとは限らない。

## 準備物の検査範囲

Python 14テストでmissing phase、二重合算、負値/bool/NaN、呼出し数・byte数・parent不整合、
通常Errの非認定、順番/有限数、両Git writerへの同一patch、元byte保持、dirty source拒否を検査した。
Git archiveから作った一時sourceでplain/instrumented copyの適用・after manifest照合も通した。
生成writer/exampleのrustfmt parseは成功した。Rust型検査・Cargo build・実機性能測定は未実施である。
独立静的レビューで見つかったFile tap欠落受理を修正し、負例を追加した。

## 専用runnerとfreeze入力

`run.py`はLinux cgroup v2の専用systemd service内でfreezeとrunを行う。
CLIはsource/build/VMを作らず、既存supervisorの`main(argv)`を呼ぶ。
そのインメモリのLinux backend `sample()`を研究用subclassで包み、元の監視結果に加えて
CPU/boot・actual cgroup controls/events・他process一覧を検査する。異常時は例外を元supervisorへ返し、
その既存`finally`によるprocess group終了・空集合確認を行う。異常後の追加cleanup sampleでは
同じ監視例外を繰り返さない。他者のprocessをkillしない。

監視間隔は0.05秒、観測gapが1秒を超えた場合は失敗する。process一覧はPID/start ticks/comm/
command hashをfreeze時に固定し、自己・その祖先・実行中の子孫を除く一覧の変化を拒否する。
SCP、rsync、cargo、rustc、solver、tar等が既に存在するwindowも拒否する。
各stage外では自分の未回収子孫とcgroupの無関係processを拒否する。
この方式は**標本化された一覧とoperatorの専用window宣言**に基づく。
監視間隔内に終了したprocessや同じdaemon内の全活動の不在を証明したとは主張しない。
既存OS daemonの通常PID変更でも安全側にabortする。

actual制約は`/proc/self/cgroup`と`/sys/fs/cgroup/...`から読み、memory.maxは有限かつ4 GiB以下、
swapは0、pids.maxは有限かつ512以下、cpu.maxは有限quotaを要求する。
systemctl実体の`show`で同じControlGroup、active、KillMode=control-group、SendSIGKILL=yes、
RuntimeMaxUSec≤1500秒、TimeoutStopUSec≤15秒を確認する。
memory.events/pids.eventsの増加も成功へ変換しない。deadlineはownerのUTCとrunnerのmonotonic
1200秒の両方を使い、次の60秒stage＋11秒cleanup余裕がない場合は開始しない。
source/binary/入力/ツールの全hashはstage前後で検査し、outputのmount/deviceも固定する。
約800 source filesとbinary/compiler等の再hashはtimer外だが、20分のcampaign枠とcache状態へ影響する。
その時間を除いた無制限の実行枠や、計測全体に負担がないとの扱いにはしない。
開始前に10 GiB以上のfree diskが必要。window lockは他の同runnerとの同時実行を拒否する。

window入力は`r1.write-phase-window/v1` JSONで、次の全fieldを実値として用意する。
`systemctl`は他のFileRefと同様、実体のabsolute `path`・`bytes`・`sha256`を持つ。

```json
{
  "schema": "r1.write-phase-window/v1",
  "dedicated_host": true,
  "no_transfer_build_or_other_work": true,
  "deadline_utc": "actual future UTC timestamp with timezone, at most 1500 seconds from freeze",
  "systemd_unit": "actual-dedicated-unit.service",
  "systemctl": {"path": "absolute actual executable", "bytes": 1, "sha256": "actual 64 hex SHA256"},
  "lock_path": "absolute new/existing lock file under an existing directory"
}
```

build入力は`r1.write-phase-build/v1`、`status=completed`。
`freeze`、`limits`、`outer_before/after`、`frozen_inputs`、`stages`も必須である。
consumerは下記driverと同じ有限制限、freezeのsource/tool/owner、実行環境、
開始・終了時のcgroup controls/eventsとsystemd期限の一致、11 stageの順序・成功・監視制限を照合する。
制限の欠落・無限化や、freezeと異なる記録は受理しない。保持後も同じoffline検査を行う。
`host`は計測host上の`run.host()`出力そのもの（CPU、flags、boot、kernelを含む）。
`compiler`と`cargo`はtoolchain実体のFileRef。
`settings`は`profile=release`、同じ`rustflags`文字列、明示`target` triple、`fresh_targets=true`。
`copies`には`baseline-plain`、`baseline-instrumented`、`candidate-plain`、`candidate-instrumented`を
一度ずつ置く。各copyの全fieldは以下で、四つのsource/output targetを分離する。

| field | 内容 |
|---|---|
| `source_manifest` | apply.pyが作った`r1-write-phase-source.json`のFileRef |
| `binary` | build後の`TARGET_DIR/TARGET_TRIPLE/release/examples/sol_codec_bench`のFileRef |
| `build_record` | 成功した既存supervisor JSONのFileRef |
| `build_environment` | 下記のbuild直前記録のFileRef |
| `argv` | `[cargo実体, "build", "--release", "--locked", "-p", "formats", "--example", "sol_codec_bench", "--target", target triple, "--target-dir", absolute target dir]`の厳密な配列 |
| `target_absent_before` | 新target未存在を確認した`true` |
| `host_before`, `host_after` | build前後の同じ`run.host()`結果 |

build_environmentは`r1.write-phase-build-environment/v1`で、`observed_at`、`host`、
`cwd`（source copyのoutput_path）、`target_dir`、`target_exists=false`、
`environment`（freezeのcompiler/Cargo環境全体と同一）を持つ。
compiler、source_manifest、build_environmentをbuild supervisorのidentity-fileにも含める。
これはproducerが記録したbuild環境をhashで結ぶ仕組みであり、署名付きreproducible buildの証明ではない。
buildログ・実際のcommand/environment・新target確認のレビューは引き続き必要。
下記の`build.py`がこの形式のsource/build記録を生成する。実際の4 buildsは未実施であり、
実行成功とidentityが確認されるまでは計測用planをfreezeできない。

```text
python experiments/hu-postflop-r1/codec/write-phases/run.py freeze --build ACTUAL-BUILD.json --window ACTUAL-WINDOW.json --supervisor ACTUAL-run_supervised.py --inputs PREPARED-PINNED-INPUTS --run-root NEW-RUN-DIRECTORY --out NEW-PLAN.json
python experiments/hu-postflop-r1/codec/write-phases/run.py run --plan NEW-PLAN.json
python experiments/hu-postflop-r1/codec/write-phases/test_run.py -v
```

planは`r1.write-phase-plan/v1`。自身の`plan_sha256`以外をsorted compact JSONにしてSHA-256を計算し、
protocolの元bytes/body、source manifestsと全after files、4 binaries、build環境/記録/log、入力3、
compiler/Cargo/Python/supervisor/研究script、host/cgroup/mount/process baselineを固定する。
`all_frozen_refs(plan)`は元absolute pathを持つFileRefの一意・path昇順配列を返す。
`verify_plan(plan, live=False)`は`read`/`identity`/`Path`を移設用に差し替えて照合可能で、
現在のWindowsで旧Linux cgroupが存在するとは仮定しない。live版のみactual hostへ接触する。

## runnerの保持schema

campaignは`r1.write-phase-campaign/v1`、`status=running|completed|failed`。
`result.json`は開始時と各標本後/全出口でatomic更新し、`started_at/ended_at`、planのFileRef/digest、
`initial_snapshot/final_snapshot`、成功済み`samples`、`first_failure`を持つ。
126件と最後のsource/host照合が通るまでは`completed`にしない。
最後のidentity検査で失敗した場合も、126件あってもcomparisonをpublishしない。
失敗時は最初の原因・stage labelと、そのstageに存在する全fileのFileRefを残す。
通常の後処理失敗で先のsupervisor失敗理由を上書きしない。kill等でfinalを書けなければ
`running`や未完成証拠を保持し、別checkerは非認定とする。

stage pathは`NNN-case-block-arm/`で、`block=0`がwarmup、`1..6`が測定、`NNN=000..125`。
各sampleは`index,case,arm,block,excluded`と次のFileRefsを持つ。

| field | 相対file |
|---|---|
| `before`, `after` | `before.json`, `after.json` |
| `guard` | `stage-guard.json` |
| `record`, `stdout`, `stderr`, `samples` | `supervisor.json`, `stdout.log`, `stderr.log`, `supervisor.samples.jsonl` |
| `report`, `canonical`, `root_canonical`, `rewritten` | `output/result.json`, `output/canonical.bin`, `output/root-canonical.bin`, `output/rewritten.sol` |
| `phase`, `phase_validation` | ONだけ`phase.json`, `phase-validation.json`。他は明示null |

sampleには元reportの`timing,metadata,input`とsupervisorの`measurement`も保持する。
supervisor identity順は選んだbinary、Python、supervisor、case入力、runner、protocol、validator。
重複pathがあれば最初の一つを残す。cwdは選んだsource-copy output_path。

snapshotは`r1.write-phase-stage-snapshot/v1`。
`status=clear|failed`、`observed_at`、全実測`identities`、`host`、`environment`、`cgroup_events`、
`process_scan`、`phase_env`、`errors`を保持する。成功時は全expected identityと一致、errors空。
`phase_env`はONのabsolute phase path、OFF/plainはnull。
campaign両端は`initial-snapshot.json`/`final-snapshot.json`でphase_env=null。
process_scanは`status=clear`、元`baseline_foreign`一覧、`foreign_same=true`、
`no_unrelated_cgroup_members=true`、上記assurance文字列を持つ。

guardは`r1.write-phase-stage-guard/v1`。
`status=clear|failed`、`checks`、`max_gap_seconds`、`poll_seconds=0.05`、`first_failure`、`assurance`。
成功時はchecks≥1かつsupervisor sample_countと等しく、first_failure=null。
失敗時はfirst_failureの`at,type,reason`を保つ。全てのFileRefと元recordを保持し、summaryのみで
raw canonicalのbyte一致を代替しない。

`comparison.json`は`r1.write-phase-comparison/v1`で、全126件完了後のみ作る。
各caseの6 armに6 raw durations・median/min/max、plain pairsとcandidate/baseline比、
両codecのOFF/plain・ON/OFF校正比と`eligible_descriptive_only|not_evaluated`を別に残す。
phase自体は各ON記録の13leaf・inner/parent/未分類をそのまま保持し、欠測を集計時に埋めない。
retained checkerによる独立再計算が、このrunner内の比較出力を読むだけの検査を補う。

[test_run.py](test_run.py)は12件の小さな偽実行テストで、正常126件、途中失敗/期限、最後のidentity変化、
環境復元、監視異常後cleanup継続、既存supervisor API引数、byte不一致、校正不成立を検査した。
OS containmentの実機検証・実測結果を代替しない。

## 回収後の独立検査

[retain.py](retain.py)はcollectorのarchiveと隣接する`.manifest.json`・`.sha256`を読む。
任意の展開先や元VM pathへfileを作らず、保持された実行binaryも起動しない。
元の126順序、全入力/source/tool bytes、各stageのidentity/CPU/boot/cgroup/制限イベント、
終了・cleanup・監視gap、stdoutと保存JSON、全rewrite/canonical/root-canonicalのbyte一致を照合する。
ONのphaseは[validate.py](validate.py)で再計算し、OFF/plainの欠測を0とは扱わない。
plainの6組と校正を独立再集計し、校正不成立時も値を補正・除外しない。
必要なraw bytesが欠けたarchiveは、hashだけが存在しても受理しない。

```text
python experiments/hu-postflop-r1/codec/write-phases/retain.py --bundle LABEL ARCHIVE EXPECTED-SHA256 --run-root ORIGINAL-LINUX-CAMPAIGN-PATH --out NEW-REPORT.json
```

source/build/runtimeの補完bundleがある場合は`--bundle`を繰り返す。既存reportへの上書きを拒否する。
出力の`status=verified`は保持証拠の整合性であり、`r1_acceptance=null`を維持する。
[test_retain.py](test_retain.py)の25件は合成証拠で不足・変更・順序・時刻・cleanup・監視gap・
phase欠落・校正の境界を検査する。実際の126実行やLinux cgroupでの起動成功は未検証である。

[runner-checks.json](runner-checks.json)は受理検査の補強前に実行した、上記12+25件と既存14件、
合計51 testsを記録する。
実行前後のscript/protocolのhashが一致し、[生ログ](runner-checks.stderr.log)をGitで保持した。
変更前のrun.py/test_build.pyは[元source](pre-review-sources/manifest.json)を保持している。
これは実測の成功記録ではない。

## 四つの実buildを作る専用driver

[build.py](build.py)は四つの実行binaryと、そのsource/build対応を記録する。
実際の四つのbuild・Linux起動・126標本の実測は未実施である。
このdriverは既に承認された資源枠についてownerが用意する具体的なbudget/resource記録を必要とする。
追加の利用者承認を要求する仕組みでも、新たな支出やVM作成を許可する仕組みでもない。

build用には計測用とは別の有限systemd unitを用意する。driverは実際のactive ControlGroup、
KillMode=control-group、SendSIGKILL=yes、RuntimeMax≤3300秒、TimeoutStop≤15秒、
memory.max≤4 GiB、swap=0、pids.max≤512、有限cpu.maxを読み取る。
全体3000秒、tool版確認15秒、copy適用60秒、各Cargo build600秒、grace/kill待機各5秒で、
固定stage時間と11秒の後処理余裕が残っていなければ次を開始しない。
supervisorはresident memory上限4 GiB、host free memory下限2 GiB、disk下限4 GiBを監視する。
開始前にはreport側・target側の両filesystemで10 GiB以上の空きを要求する。
build後は同じCPU/boot上の新しい専用**計測unit**でfreeze/runする。build用3300秒unitを
計測側の1500秒条件へ読み替えない。

owner入力は次の実値を持つ`r1.write-phase-build-owner/v1` JSONである。
`budget_record`はownerによる残予算・資源確認の既存記録、`systemctl`は実体のFileRef。
期限は開始時点から3300秒以内のtimezone付きUTC。FileRefはabsolute path、bytes、SHA256を持つ。

```json
{
  "schema": "r1.write-phase-build-owner/v1",
  "resources_authorized": true,
  "budget_record": {"path": "actual absolute preflight record", "bytes": 1, "sha256": "actual SHA256"},
  "systemd_unit": "actual-build.service",
  "systemctl": {"path": "actual absolute executable", "bytes": 1, "sha256": "actual SHA256"},
  "deadline_utc": "actual future UTC deadline"
}
```

```text
python experiments/hu-postflop-r1/codec/write-phases/build.py --baseline-source PINNED-BASELINE --candidate-source PINNED-CANDIDATE --example FROZEN-EXAMPLE --inputs THREE-PINNED-SOLS --cargo ACTUAL-CARGO --rustc ACTUAL-RUSTC --rustfmt ACTUAL-RUSTFMT --supervisor ACTUAL-run_supervised.py --cargo-home PRESEEDED-CARGO-HOME --owner OWNER-JSON --out NEW-REPORT-ROOT --targets NEW-CARGO-TARGET-ROOT --target ACTUAL-HOST-TRIPLE
```

cargo/rustc/rustfmtは解決後の実ELFを指定する。rustupのsymlink/hard-link proxyは拒否する。
全copyに同じRust flags（既定`-C target-cpu=native`）、明示RUSTC、incremental=0、jobs=2を使う。
既存環境のcompiler/Cargo overrides、sourceまたは親directory/cacheのCargo configは拒否する。
依存cacheは先に準備し、`CARGO_NET_OFFLINE=true`で追加取得を行わない。
`--offline`をargvへ加えず、既存`run.verify_build`の固定build commandを維持する。
`NEW-REPORT-ROOT/sources/`にsource copy・patch・manifestを、`stages/`に版確認/copy/buildの
supervisor・stdout・stderr・sampling記録を残す。Cargo生成物は別の新target rootだけに置く。
入力sourceや既存targetの削除・再利用、失敗stageのretryは行わない。

`freeze.json`で全source/input/tool/owner bytesを最初のstage前に固定し、各stage前後と最後に再検査する。
4 source copyは既存apply.pyをsupervisor経由で作り、四つともfresh release buildする。
成功出力`result.json`は上記`r1.write-phase-build/v1`の厳密なcopy/argv/environment形式を持ち、
既存`run.verify_build`による照合が通ってから公開する。途中停止や最終source/boot変更は
`status=failed`と最初の原因・残存stage file一覧を保持し、成功した先行buildを全体成功としない。
これらはsource/command/binaryの結合記録であり、compiler挙動の独立証明ではない。

[test_build.py](test_build.py)は偽のstage実行で、既存consumerとのschema互換、
途中失敗、最終source/boot変更、期限、既存target保護、wrapper/config/proxy拒否、
supervisor APIと遅いconsumer検査失敗を確認する。有限制限・freeze・outerの受理検査も含む。
実compilerやLinux containmentは起動していない。
[build-checks.json](build-checks.json)と[生ログ](build-checks.stderr.log)は補強前12件の独立再実行で、
上記51件とは別の検査である。前後hashに対応する変更前sourceは上記snapshotに保持する。

[reviewed-checks.json](reviewed-checks.json)と[生ログ](reviewed-checks.stderr.log)は補強後の全69件
（build18、runner12、retainer25、既存patch/validator14）の成功記録である。
71.38秒で完了し、実行前後の全script/protocol/input選択のhashも一致した。
これは合成検査と生成sourceのformat検査であり、実build・実測・性能認定は含まない。
