# 現行sourceの工程時間・生成時resident counterを測る準備

production `11e4062ba1735e58b60d12999cb23ed10fd1a163` の**計測専用コピー**を再現する。
本体・既存の凍結runner・過去結果は変更しない。ここにはcopy生成器、固定source pins、
計測module、前向きprotocol、区間validatorと軽量testsを置く。
Rust/Cのbuild、solve、Linuxでのcounter校正、Cloud起動はまだ実施していない。
専用有限runner/回収checkerはこの準備には含まず、実行前に実装・レビューが必要である。

## 既存コードのままで取れるものと追加計測

| 対象 | 原版から取得可能 | このコピーで追加するもの |
|---|---|---|
| 全CLI | supervisor wall、native child `wait4`、artifact bytes、停止iteration/品質 | main dispatch内の非重複leaf。startup/exitと最終計測JSON保存は外部時間にのみ含む |
| 初期化 | 全CLIからは分離不能 | dry-run、tree/rank、solver storage準備。入力parse/pool作成はinput_preparationへ分離 |
| CFR・BR | 現行の停止時計はCFRと定期EV/BR等を含む | `solver.run(chunk)`、定期EV/BR、最終EV/BRを分離。呼出し回数・順序・計算は不変 |
| 保存生成 | `.sol/.ckpt`容量と全CLI時間 | checkpoint writer、SOLのCFV/reach/量子化/payload準備、SOL writerを分離 |
| 読戻し | 凍結codec helperのfull decode/root read時計とcanonical照合 | 同じ公開reader呼出しのresident counter。summary表示を保存profile BRと呼ばない |
| 工程memory | process全体のunreset counter、RSS sample | memory専用armで各境界のmm high-waterをreset。絶対resident counterでありallocation量ではない |

[source03 phases](../phases/README.md)の境界を現行anchorへ合わせた。
[writer phases](../codec/write-phases/README.md)のplain/OFF/ON校正・byte不変条件を再利用するが、
writer内部13項目や4個の旧新buildは追加しない。現行の一つのsourceを二つの新targetへbuildする。
計測はCLIの3ファイルだけへ挿入し、engine、kernel、formats、oracle、設定を変更しない。

## 再現するsource

[source-pins.json](source-pins.json)は、既存current32のsource manifestからCargo/crates/.cargoの
**207ファイル**を固定した。元codec helperは[codec-input.rs](codec-input.rs)へ原bytesを保持した。
生成後に変わる5ファイルも同manifestで固定する。新exampleはCLI crateに置き、既存依存だけを使う。
sourceの不足・余分なcrateファイル・symlink・byte変更・二重適用・出力上書きを拒否する。
`source-copy.json`へ元/後の全hash、生成器/module/protocolのhash、patch全文を残す。
sourceとoutは互いの内側でない未使用pathを指定する。以下は**将来実行する例**である。

```sh
python3 current-phases/prepare.py --source /opt/r1/source11e4062 --out /opt/r1/phase-plain --mode plain
python3 current-phases/prepare.py --source /opt/r1/source11e4062 --out /opt/r1/phase-instrumented --mode instrumented
# 同一boot、Rust1.97.0、別のfresh target、locked/offline、jobs2で
# cargo build --release -p cli --bin solvers --example current_phase_codec
# を各copyに一度。targetはCargo専用、source-copy manifest作成後にsourceは変更しない。
R1_CURRENT_PHASE_MODE=time R1_CURRENT_PHASE_OUTPUT=/opt/r1/stage/phase.json \
  /opt/r1/phase-instrumented-target/release/solvers solve CASE.toml --out /opt/r1/stage/run
python3 current-phases/validate.py /opt/r1/stage/phase.json --manifest /opt/r1/phase-instrumented/source-copy.json
```

OFFは両環境変数をunsetし、同じ計測binaryの追加分岐の影響をplainと比較する。
time/memoryは同じbinaryでMODEだけを変更する。既存出力・空/相対path・不明MODEは拒否する。
通常errorでは失敗spanを残し、panic/killではinitial `running`記録と外部stderr/supervisorのみが残る。
失敗や未flush区間を0へ補完しない。256 spanの上限を超えた場合も失敗とする。

## メモリcounterの意味と校正

Linuxの`clear_refs=5`はmmのpeak resident setを現在のRSSへresetする。
1〜4のpage-reference/soft-dirty操作は行わない。
[kernel公式説明](https://kernel.org/doc/html/v5.16/filesystems/proc.html)、
[man-pagesの仕様](https://www.man7.org/linux/man-pages/man5/proc_pid_clear_refs.5.html)を根拠とする。
各memory leafについてreset直後と終了時のVmRSS/VmHWMを原値で保持する。
`end.hwm`をそのreset窓のcounterとし、**前後差をpeakにしない**。
VmRSS/VmHWM自体にもkernel accountingの近似があるため、物理RSSの厳密なpeakとは呼ばない。
[counterの注意](https://www.man7.org/linux/man-pages/man5/proc_pid_status.5.html)

memory armは時間armと別実行である。boundaryの読取り・reset・observer footprintが含まれ、
時刻の半開区間とcounter窓は境界処理分だけ異なる。時間armの性能へ混ぜない。
初期計測JSON出版より前のメモリはreset窓の外にある。全体のnative peakが必要な場合は、
resetしないplain/OFF/timeを[既存native launcher](../focused-memory/native_rss.c)で測る。
memory実行後のru_maxrssを元の全process peakへ読み替えない。

[memory_probe.c](memory_probe.c)で128MiBの過去peakを作りmunmap、reset後の1MiB/64MiBを
識別できることを、最初のsolve前に固定閾値で検査する。失敗はcampaignを停止する。
同じVMのkernel/boot/configで実施し、GCC source/binary/command、raw7snapshot、exitを保持する。
native launcherを使う場合は既存の親high-water分離校正も必要で、別bootの成功を流用しない。
メモリ比のobserver screenはprotocolに固定したが、通過してもinstrumented counterの記述だけを許す。
`max(SOL準備peak, SOL書込みpeak)`が生成時のresident counterであり、solver本体の常駐分も含む。
二つのpeakを加算・entry RSSを減算せず、checkpointと読戻しも別に示す。

## 最小campaignと有限資源

[protocol.json](protocol.json)の3既存synthetic case、1worker、F32、Full、NC<0.04を使う。
各caseでplain/OFF/time/memoryのwarmup各1＋測定各3、**48 solve**。
その後、各caseのplain/block1の同じSOLをfull/root readの4armへ渡す**96 codec process**。
保存はCLIで既に測るため、対応helperのstandalone stream-writeは今回は実行しない。
3 measured blockの順序は固定し、全solveが終わるまで大きなcanonical生成を始めない。
全48 solveのlive軌跡・checkpoint・保存profile BRと全codecの意味/byte照合は計測後に完了する。
warmupも品質検査する。保存後品質のhelper/sourceは既存final-pipelineのものをpinして使う。

時間校正はcase/operationごとにOFF/plainとtime/OFFの中央値比が共に[0.95,1.05]。
plain中央値10ms未満は記述のみ。失敗時も値を隠さず、補正式・追加反復・別inputを使わない。
3回は小規模な摂動screenであり、統計的な同等性や一般的なphase比率の保証ではない。

残り未予約2 USDを上限とする計画で、4vCPU/16GiB Spot、40GiB disk、最大1時間、
最後15分を回収・検証・削除へ確保する。これは価格見積りや支出予約の変更ではない。
ownerが起動前の価格・残予算・実リソースを確認して固定する。
build各copy≤900秒、個別計測≤30秒、全作業≤2700秒。次stageの全timeout＋20秒が残らなければ開始しない。
OS/boot/CPU/source/binary/cgroupを前後照合し、12GiB/swap0、10GiB sampled-RSS stop、
空きRAM1GiB・disk4GiB、20ms監視、有限systemd unitとVM絶対STOPを使う。
first failureで停止し、failed/skippedを保持する。新bootでの継続・自動retryは行わない。
回収は停止後、原bytes/manifest/全logsとrawcounterを512MiB以内のCAS archiveへ保持し、
信頼するcheckout側のcheckerで検証してからVMを削除する。

32worker scratch allocation診断は今回入れない。`Scratch::new`は空Vecで、確保候補は
`take`のcapacity増加である。全workerをphaseへ集計するにはengine変更、atomic競合またはTLS回収を
要するため、この小VMの工程校正には適さない。既存32worker遅延の主因とは断定しない。

## 今回の軽量検査と残る実行前確認

```text
python -B -m unittest discover -s experiments/hu-postflop-r1/current-phases -p test_*.py -v
```

source copyの再現/hash/元source保護/拒否条件と、区間partition・identity・memory/null・校正を検査する。
計測moduleのRustコンパイル成功やLinux runtimeの正しさをPython testで代替しない。
採用前に生成diff、Rust/C build、reset校正、全arm出力一致、finite runner/retained checkerを確認する。
新しい実測は、過去のphase `null`やsource03の測定を上書きせず、独立した証拠として残す。

[最終軽量検査](test-evidence04/command.json)は28 tests成功、実行前後のcode/input pinsは一致した。
10ms未満の校正結果を成功扱いしない負例も含む。[生成後diff](generated-review.patch)は独立した
ソースレビューを行い、CLIの計測追加以外の数値処理変更がないことを確認した。
Rust/Cの実コンパイル・実機memory測定の成功は主張しない。

`test-evidence01/02`はWindows Python3.13の一時directory ACLによりcopy testsのsetup/cleanupが
失敗した原ログである。通常の継承ACLでowned directoryへ作る方式へ変更した後、
`test-evidence03`で27 tests、10ms下限を加えた最終版で28 testsが成功した。
最初のcapture表示にもcp932の文字化け表示エラーがあったが、元stderr/receiptは保存済み。
初回の一時directory20件はすべて空であることと所在を確認して削除した。
旧test sourceと旧validator sourceは各時点の証拠directoryへ保持し、失敗記録は書き換えていない。
