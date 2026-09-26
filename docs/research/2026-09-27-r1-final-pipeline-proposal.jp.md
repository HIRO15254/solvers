# R1 最終版の全工程・成果物比較案

2026-09-27時点の**未採用の実験案**。実施命令、正式なT1-06閾値版、Linearの状態表ではない。
新しいVM、runtime変更、測定はこの文書の作成では行っていない。
[R1実行票](../plans/r1-execution-plan.jp.md)のT1-07/08に対し、現行版の保存・読戻しと
同等品質での全工程時間・process memoryを、小さい追加campaignで確認する。

## 先に閉じる範囲

1. **優先1：SOL v4 / checkpoint v2の実I/Oと保存profile。** 同じ入力から新規生成した成果物の
   容量、保存・全読込・部分読込時間、量子化policyのEV/BRを確認する。
   [過去のcodec入力](../../experiments/hu-postflop-r1/codec/inputs.json)はsource06のSOL v3であり、
   現行readerへそのまま渡しても現行形式の性能根拠にはならない。
2. **優先1：現行版の累積効果を全工程で比較。** 過去のpipeline、compaction、action並列化、kernelの
   改善率を掛け合わせない。[exact mass source04](../../experiments/hu-postflop-r1/exact-mass/report04.jp.md)
   は数値修正のcost ceilingを満たしたが、solver＋品質チェック区間は旧版より遅く、全32本の
   native RSSが同値だった。保存込みの時間・メモリ削減を示した結果ではない。
3. **優先2：判定範囲をT1-06へ接続。** この合成3ゲームの内部比較と、条件・精度が未確定の外部参照を
   分ける。成功しても24候補、一般range、NoRivers、全storageや全thread数を認定しない。
   [既存受入監査](../../experiments/hu-postflop-r1/acceptance/audit.jp.md)の外部校正条件は別に残る。

ここで新たにreader/writerの不具合を見つけたという意味ではない。source04の原workspaceログには、
indexed部分読込・破損/旧版拒否、非対称compact F32/I16 checkpoint再開、保存u16 profileの再評価、
微小reachのFull/lazy river roundtripが成功している。残りは、その現行実装を使った実測の範囲である。

## 比較する2版

| arm | 固定source | 目的 |
|---|---|---|
| old | `88ffa5dd4583e5c8bae84e20e9b8390cb719e4f0` | bulk codec、compact hand、action grain、prepared kernel、exact massより前の保持済み対照。SOL v3 / HU共通checkpoint v1 |
| new | `11e4062ba1735e58b60d12999cb23ed10fd1a163` | 現行production。SOL v4 / HU共通checkpoint v2 |

oldには既にCLIの`target_nash_conv`による停止と`hu_saved_profile_audit`がある。
元のR1全工程baseline `9632d8b244990cb5b95ff7d0cacc84ee95a7ee0e`へ評価器を移植するより小さく、
v3からv4までの累積変更を比較できる。初期R1の全改善をこの2版の差だけに帰属させず、
9632d8bとの[歴史的比較](../../experiments/hu-postflop-r1/pipeline/current-report.md)は別の証拠として残す。
個々の最適化の寄与をこの比較から分離しない。両版のCargo.lockとCargo.tomlは現在のGit比較で同一。

sourceはGit原文から別rootへpackし、全build入力のexact set・長さ・SHA-256、archiveとmanifestを固定する。
old/newのtargetも別々にし、同一VM上でRust 1.97.0、同じrelease flags、同じCPUでfresh buildする。
以前のVMのnative binaryは流用しない。productionのコードは変更しない。
研究用exampleの追加が必要なら両armへ同一bytesのoverlayとして記録し、production差分から分ける。

## 入力と同じ有限ゲームの確認

次の既存[pipeline設定](../../experiments/hu-postflop-r1/pipeline/README.md)を元に、
**`run.threads`だけ8から1へ変更**した同一TOMLを両armに渡す。新しいrangeやbet treeは追加しない。

| case | 元TOML SHA-256 | 最大反復 | check間隔 | target |
|---|---|---:|---:|---|
| River | `cbbe0b27cc7cc5d31a6b11ac9ba9a421bf6e7e755a4306254cc1071a2ab9d790` | 400 | 25 | NashConv `< 0.04` chips |
| Turn | `726f664ac4417d1d5d6cee145885805fc90cdd719d9d2490fa1f6e49df7067d8` | 400 | 25 | 同上 |
| limited Flop | `c8cb6c07ed18bbcf60f81b1e8022850b0a70f3c584d03b4307d7f5b9c504e7ad` | 100 | 10 | 同上 |

F32、DCFR、no-rake、chip-EV、pot20、stack60、iso off、Full保存。
Flopの狭いrangeと後続street checkdownは元の有限ゲームの一部で、一般Flopの代表とは呼ばない。
生成TOMLのbytes/hashと元ファイルとの差分を**候補実行前**に固定する。pilotでcapやtargetを変更しない。
過去の100/100/50反復へ上限を切り詰めず、各armが共通cap内の最初の合格checkで止まる。
旧・新ともCLI実装の不等号は`<`。別exampleの`<=`へ置き換えない。

同一性はファイルversionや配列長の一致ではなく、board、global comboごとの初期weight、pot/stack、
公開木のaction/chance/terminal構造、support上のchance maskと重み、rake/utility、schedule、停止設定を照合する。
共通APIのcensusはterminal evaluatorの内部とpublic deal card IDを列挙しないため、
同一有限ゲームを支える必要条件であって単独の完全な証明ではない。固定sourceの差分と独立oracle回帰を併用する。
旧版の1326幅と新版のseat-local幅は意図した差なので、比較用の手札行はglobal comboへ写像する。
normalizerはこのfixtureではf64 bits一致を必須とし、違えば測定不適格とする。観測後に許容幅を導入しない。
公開exportが省略するzero-own-reach行を均等戦略等で補完しない。全raw blockの読書き同値は各arm内で検査する。

v3/v1ファイルを現行readerへ渡さない。各版が同じTOMLからfresh solveし、自分のreaderで読む。
外部tree sourceの自己完結化、旧版拒否、NoRivers、I16は既存回帰証拠を引き継ぎ、今回の性能対象には加えない。

## 品質・成果物の必須判定

- 全processで有限のseat別EV/BR/gainとNashConvを保持する。上記3ゲームに限り`NC/2`を
  Exploitabilityと解釈でき、`NC < 0.04`は開始pot比0.1%未満に対応する。一般和の定義へ流用しない。
- native CLIの`solve --out ... --sol-streets full`を測る。各checkの評価・checkpoint・最終保存を含め、
  時間目標による途中終了は使わない。cap未到達ではなく**品質未到達**なら失敗として残す。
- 全24 solveのFull SOLを、同じarmの`hu_saved_profile_audit --threads 1`で再構築し、
  保存されたu16 policyの両seat EV/BR/gain/NCを再評価する。保存後も`NC < 0.04`を必須とする。
  metadataのNCを代用しない。保存前後の差はchips単位で別々に報告する。
- 同armのwarmup＋3反復は、終了iteration、品質trajectory、時間を除くmetadata、policy/stateの
  正規化比較でexact一致を要求する。SOL全ファイルには実測時間が入るので反復間のfile hash一致は要求しない。
  old/newの反復数・float bits・量子化bytesが異なること自体は数値修正の失敗とはしない。
  差分を保持し、同じ品質目標への到達で比較する。
- summary/root/full readerのmetadataと該当raw blockを同armで照合する。writerへ戻した同じpayloadは
  再読込canonical bytesが一致することを必須とし、同versionで完全file bytesが一致するかも記録する。
  他versionとのSOL bytes一致を要求しない。
- checkpointはheaderの期待version、config hash、iteration、state iterationとstorage形状を読む。
  読戻し専用の小exampleが必要なら`formats::read_checkpoint`を呼ぶ同一overlayに限る。
  早期停止したrunへ既存completed-cap resumeをそのまま呼ぶと追加反復が起こり得るため、
  「追加0反復の読込み時間」と呼ばない。実際の復元・追加反復の同値性は既存F32/I16回帰証拠に委ねる。

correctnessが欠けた標本を除外して高速な残りだけを採用しない。中断・timeout・target missは原出力を保持し、
残りを機械可読の`skipped`にして終了する。自動retry、代替標本、結果後のcap/target変更はしない。

## 標本数と測定区間

ケース順はRiver、Turn、Flop。block 0をwarmup、block 1～3を測定とする。
各case/blockでold/newを連続実行し、`(case_index + block) % 2 == 0`ならold先、それ以外はnew先。
**solveは計24 process、測定18、warmup6**。warmupを中央値へ含めず、全個別値とmin/median/max、pairごとの勝敗を報告する。
CPU負荷のあるbuild・転送・圧縮・検証はperformance区間に重ねない。page cacheはwarm/uncontrolledとし、cold-cacheを主張しない。

| 系列 | 実行と記録 |
|---|---|
| 全工程 | 未改変CLIのprocess開始～終了。入力、構築、CFR、各品質評価、各checkpoint、SOL生成、run.json、dropを含む |
| 利用時読込 | 各solve後のsummary queryを独立processで記録。保存profile auditのload/eval時間・RSSは独立した検証費用として扱う |
| codec | 各solveで生成したimmutable SOLを、全24 solve終了後に実行する当該sampleの`decode-all` / `read-root` / `stream-write`の共通入力とする。計72 process。反復間はcanonicalのwall_secsだけを除外して同arm一致を検査し、各入力の原bytes/hashを保持。既存exampleのoperation内時間と全process時間を分ける |
| checkpoint | 全24本のbytesと各保存eventを保持。各arm/caseのwarmupで作った1本を読戻し検証。単独の書込み時間は測っていないものとして残す |
| phase | 今回は追加instrumentationを行わない。初期化・CFR・定期/最終EVBR・checkpoint・SOL準備の個別時間はnull。旧sourceの別phase証拠を現行sourceの内訳にしない |

既存のphase計測は異なるsourceの別系列である。今回の全工程時間から過去のphase時間を差し引かず、
CLIのwallSecsや全工程との差を純CFR時間と推定しない。今回、phase RSSの測定・認定も行わない。

## メモリの判定方法

既存supervisorのnative root peakとsampled RSSを両方残す。過去の同値native値を根拠に
「memory非回帰」とは言わない。直接起動したRust solverに別processの子がないことをsourceと
PID samplesで確認し、Rayonのthreadと子processを区別する。観測が不完全なら以下の判定は不適格。

この条件で、ある実行の同じOS会計におけるsolver root RSS peakを`P`、終了時`wait4.ru_maxrss`を`U`、
実行中に観測したroot RSS最大を`L`と置く。pre-exec親high-waterが混入しても`P <= U`、
samplingがpeakを逃しても`L <= P`である。3反復について

`max(U_new) / min(L_old)`

を保守的なpeak比上限として報告する。この比が十分小さければ、親high-waterの床を正確な
solver RSSと誤認せずに、観測した新3本対旧3本の削減を示せる。
native値そのものの旧新比、静的storage bytes比、cgroup memory.currentをこの比の代用にしない。
値が正・有限でない場合や、root以外のprocess、未知のOS換算、欠測、不完全cleanup、`U < L`等の矛盾があれば判定を行わない。
`/proc/statm`もOSの会計値であり、この不等式を物理resident量そのものの厳密な証明とは呼ばない。
PID sampleで1processしか見えないことだけでは短命な子の不存在を証明できないため、source確認も必須とする。
測定器自体の負荷・最終sample間隔・実際のsample数も残す。短いcaseで下限が弱ければ`inconclusive`とする。

Flopをmemoryの事前指定primary caseとする。元のdense arenaが小さいcaseより大きく、
短い新solveの真のpeakが親の床以下でも保守上限として判断しやすいためである。
対象外の入力や将来のprocessへ、この3反復のboundを外挿しない。

## 提案する内部screen

以下は未採用の運用screenで、R1全体の合格閾値ではない。採用する場合はprotocol/validator hashとともに
**新campaignのcandidate実行前**に固定し、失敗後に緩めない。外部参照の許容差は含めない。

| 項目 | 事前条件 / screen |
|---|---|
| correctness | 全標本で前記の品質・入力・保存後BR・reader/writer・identity/cleanup条件に合格 |
| 全工程time | TurnとFlopのnew/old中央値比がそれぞれ`<= 0.95`。Riverを含む3caseの幾何平均も`<= 1.00`。Riverが旧中央値1秒未満なら単独の速度向上を主張しない |
| process peak | Flopの保守上限`max(U_new)/min(L_old) <= 0.90`。満たせなければmemory削減の判定は未達/不確定とし、改善がないことの証明や静的領域の削減で代用しない |
| I/O | 各case/operationの比・絶対差・散らばりを全件報告。旧operation中央値が10ms以上ならnew/old`<= 1.10`を回帰screenとする。10ms未満は分解能上の記述的結果。容量はSOL/CKPT別に増減を示し、全caseでの容量減少を必須にしない |

3反復は母集団の有意差やSLOを証明しない。個別screenとscopeを分けて報告し、
例えばtimeが改善してもI/Oに回帰があれば、無条件の総合passに集約しない。
以前のexact-mass数値修正用1.10/1.25 cost ceilingや、旧codecの126-process案を今回の認定条件へ流用しない。

## 再利用する実装と、更新が必要な接続部分

- [pipeline runner](../../experiments/hu-postflop-r1/pipeline/run_campaign.py)のprocess監視、header identity、
  保存profile/exportの手順を参照する。ただし旧runnerはbaseline v1、candidate v2/v3、8threads、
  pilot反復固定、旧新export完全一致を前提とするため**そのまま実行しない**。
  新campaignの薄いrunner/validatorでv3対v4・1thread・品質停止・global mappingを明示する。
- [codec example](../../crates/formats/examples/sol_codec_bench.rs)は同一bytesを両armでbuildする。
  canonicalにはwall_secsやprivate dimensionが入るため、旧新canonicalを等値gateにせず、
  各armのimmutable input・read/write roundtripに用いる。
- [range scaling](../../experiments/hu-postflop-r1/range-scaling/scaling-run.py)や
  [exact mass](../../experiments/hu-postflop-r1/exact-mass/run.py)のschedule、source/binary pin、失敗suffix、
  CAS保持・portable照合を再利用する。過去schemaのpass条件を偽装して新結果を入れない。
- [既存phase patch](../../experiments/hu-postflop-r1/phases/README.md)は9632d8b/source03専用で、
  今回の2sourceへ適用しない。個別phaseが後で必要になった場合は別提案にする。

事前に合成ファイルでparser・version差・missing raw・changed identity・target miss・zero-stage・
failed/skipped suffix・sample count/peak再計算を試験する。保持されたPythonコードは実行せず、
現在の信頼したcheckoutのvalidatorから原bytesを読む。

## 既存検証の引継ぎと追加build

11e4062の全204 crates fileのexact set・SHAと、architecture / family仕様 / CLI reference / user guideの
4文書は、[source04 manifest](../../experiments/hu-postflop-r1/exact-mass/source-new04/source-candidate-manifest.json)
（SHA-256 `1a84947f6daa9ca1c57d5f48e4914e176643dbe9c787262ded95dcf6aba042d6`）と再照合し、208件すべて同一だった。
原証拠archive `exact-proof04.tar.gz` のSHA-256は
`b7361b4adc579dc87c36e8c9cd37b1c307465db90fa6432eb1bc24d67163da37`。
原workspace stdoutの56 summaryを再集計し、958 passed / 0 failed / 31 ignored、
別release oracle3件、river resolve1件を確認した。
[追加ignored4件](../../experiments/hu-postflop-r1/exact-mass/extra-validation/report04.jp.md)も同じsource04。
これらは別々の試験数で、unique tests数として足し合わせない。

したがって、**commit名が変わっただけでfmt/全workspace Clippy/958 tests/同じignored群を再実行しない**。
新packでも全production/build入力と上記manifestを照合し、既存validation・source-afterへ結び付ける。
[共通Python39件](../../experiments/hu-postflop-r1/exact-mass/python-checks/README.jp.md)も、
記録された9 source pinが現行bytesと同一なので引き継げる。
既存のdoc checkは、新しい研究文書と導線を対象に軽量再実行する。

新VMではtoolchain/target/CPU/bootを記録してold/new release CLI・audit・codecをbuildし、
新overlayだけのcompile/targeted Clippyと新runnerの軽量testsを実行する。
前VMで検証したproduction sourceと、新VMで作った測定binaryの結び付けは別のbuild recordに残す。
old全体の過去validationを引き継ぐと記す場合も、そのmanifestのbuild入力一致を確認する。
不一致を「同じrevisionらしい」で済ませない。production修正が必要になった場合はここで停止し、
変更範囲に応じた通常検証と、新しい比較source版を発行する。

## 有限資源と原証拠

残る保守予算は8 USDを上限とし、今回の予約はその内側に置く。これは価格見積りや起動承認ではない。
実施時にrootが既予約・未精算分とその時点の料金、disk/IP/転送予備を照合する。
第一候補はx86_64の4 vCPU / 16 GiB小型Spot、最大3時間の絶対停止、同一boot・1 Rayon worker。
buildはCargo2 jobs、incrementalなし、debug情報なし。別targetのbuildは並列実行せず、build後の測定も逐次。
32CPU再試験はこの案へ含めず、既存の32CPU証拠を過去sourceの結果として保持する。

外側cgroupはMemoryMax12GiB、swap0、全process終了を保証するKillModeと絶対deadlineを持つ。
supervisorはsampled RSS10GiB、free memory1GiB、disk reserve4GiB、grace/kill各5秒、sample間隔20ms。
build stage最大1200秒、各solve/audit最大300秒、codec/query最大120秒。
各stage直前にsource/input hash確認後も残時間を再確認し、timeout＋cleanup＋回収余裕がなければ新規起動しない。
測定終了期限をVM削除期限より最低20分前に置く。終了に失敗したprocessがあれば次を起動しない。
全予定stageが時間内に終わらなければ未完了として残し、成功caseだけで総合認定しない。

planにはsource/archive/manifest/overlay、全binary、Rust/Cargo/flags、runner/validator/protocol、生成config、
CPU型・logical/physical topology・affinity・quota・cgroup・bootをpinする。各stage前後のsource/binary/input
再hash、supervisor原JSON、stdout/stderr/samples、run.toml/run.json/progress、SOL/CKPT、audit、
canonical・exportの元bytesをCAS化する。全標本を保持し、同一bytesのみdedupする。
compiler等はidentity-onlyと原bytes保持を区別する。失敗記録もraw outputsを先に回収する。

portable verifierはVMの絶対pathを必要とせず、原source/archive、期待schedule、全成功/失敗状態、
原出力SHA、品質・同arm再現性、memory count/peak、各median・ratio・screenを再計算する。
保持量は事前上限1GiBの圧縮転送を目安に見積もり、重複canonicalを無制限に増やさない。
固有rawを削除してhashだけ残すことはしない。VM削除前にexport、download、local再検算、
残存instance/disk/IP確認まで行い、保持・費用の証拠を別に残す。

本実行用の草案では、6 census →全24 solve→各保存物の summary／保存後 BR／codec／warmup checkpoint reader の順とする。solve の old/new 交互順は維持する。大きい canonical の保持を全 solve 後に置くが、census・SOL・CKPT 等の保持も親 Python の high-water を上げ得るため、メモリ上界が基準を満たさない場合は inconclusive とする。保存後品質が不適合なら既実行 solve を含め比較全体を不適格とし、残件は中止する。live／保存後の各利得および NashConv が −1e−6 chips 未満なら拒否し、観測値は clamp せず保持する。この丸め許容は実測前に固定する。
