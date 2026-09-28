# Sparse rank-group scratch の固定比較

本手順は研究候補のscreenであり、本体採用・全R1受入・追加支出の許可ではない。
候補kernelと原本のpinは `provenance.json` に固定する。旧compact F64算術の順序を維持し、
groupごとの52-card zero配列生成を一度の初期化と使用cardのresetに置き換える。
fold、開始時のall sums、整数fallback、engine、frozen `cfr-ref` は同一とする。

旧VM19の `r1-cpu-profile-package/v1` deploymentを変更せず利用する。
新 `overlay-manifest.json` と `overlay-installation.json` は元manifestのpin、source revision、
追加file全部のpinに結び付ける。元packageとoverlayのpath集合の重複は禁止する。
VM18 chance-grain adapterの3fileもoverlayへ同一bytesで加え、そのsolve.rsを両armへ追加する。
CFR/qualityともdepth2を固定する。
CPU profile版adapterやperfは実行しない。

## 2 CPUのbuild

`run.py prepare --source PACKAGE/source --workspace NEW_WORK --out NEW_PROOF --cargo CARGO --rustc RUSTC
--launch-attempted-at UTC --stop-deadline-utc UTC --deadline-utc UTC` の後、`build --out NEW_PROOF`。
pristine source全inventoryからWORK内にbaseline/candidateを作り、candidateのkernelだけに
`prepare.py --apply-to` を適用する。適用receipt、全source inventoryと各armのgzip source archiveをproofへ保存する。

VM18のhashで固定したsupervisor・host/usable ISA・environment・全state reader・durable gzipを再利用する。
各armに空の専用Cargo targetを作り、offline/locked/release/jobs2、portable x86-64-v3でbuildする。
thin LTO/codegen-units1/opt3は同一のworkspace設定に従う。Cargo依存取得は外側のbootstrapのみ。
toolchain1本、baseline build/test、candidate build/testの5stageを順番に完了する。
core testはengine/holdem/cfr-refの非ignored tests全体で、候補の4test、既存quadratic/compact kernel4test、
VM18で指定したmapped chance/parallel/storage6testの成功出力も明示的に要求する。
候補の旧算術referenceは独立oracleではなく、既存oracleを置換しない。

build/test個別上限480秒、toolchain10秒。build期限は作成要求から20分以内。
process memory4GiB、outer memory6GiB、swap0、CPUWeight100、空きmemory/disk各2GiB。
成功buildだけにimmutable `build.json` を作り、tool/source/control/binary/各原本receiptのpinを結ぶ。
buildの打切りやtest不一致があれば32 CPU測定へ進まない。

## 固定38 solve

同instanceを32 logical CPUへresize/rebootし、同bootで両armを測る。guestは16core/32logical、
affinity32とquota>=32を要求する。この観測は専用32物理coreでの線形scalingの証明ではない。
`measure-prepare --out PROOF --highcpu-armed-at UTC --highcpu-stop-deadline-utc UTC --deadline-utc UTC`
の後、`measure --out PROOF`。元のbuild全fileと2binaryを再hashする。再build・再試行・再開はしない。

1. narrow/N2、workers1/32 × baseline/candidateの4smoke。最初のbaselineをN2 canonicalとする。
2. narrow/expanded各N16、baseline/1workerの2canonical。各CFR時間>=4秒を要求し、反復を増やして救済しない。
3. case2 × round0..3 × workers16/32 × arm2の32matrix。round0の8本はwarmup、各条件round1/2/3が3標本。

caseを外側で固定し、偶数roundはworkers16→32かつbaseline→candidate、奇数roundは両方を逆にする。
depth2、F32/DCFR16、CFV captureなし、入力・range・iteration・quality7walkは同一。
warmup/smoke/canonicalは速度比の標本へ含めない。全38本で全state byteとquality JSON全byteを
case/iterationのbaseline canonicalと比較する。same-stateの判定をroot EVだけで代替しない。

## 時間・保存・打切り

VM全体のGCP STOPは作成要求+45分（秒floor）。32 CPUのSTOPはstart要求より前の
`phase32-plan.json` の `armed_at_utc` +480秒（秒floor）で固定し、bootstrap/接続時間も含める。
runnerの `highcpu_armed_at` と `highcpu_stop_deadline_utc` はその2値である。
実測の新規windowは240秒超・360秒以下、deadlineは32 CPU STOPの20秒以上前かつ元STOPより前。
controlが遅れたらwindowを延長せず開始を拒否する。canonical個別上限60秒、その他20秒。
各stage開始時にその上限+10秒の余白が必要であり、残りstage全部が必ず完了するとは主張しない。
最初の失敗/期限不足で残りをskippedにし、prefixを速度成功として使わない。

process memory8GiB、outer12GiB、swap0、CPUWeight100、空きmemory/disk各2GiB。
success stateはN2/narrowN16/expandedN16の3canonical gzipだけ保持する。全byte照合→fsync receipt→
重複raw除去の既存手順を維持する。失敗rawは時間がある場合だけlossless保存を試みる。
proofは240MiB、外側archive256MiB、総転送320MiB以内。上限を超えた原本を削って成功とはしない。
GCPの回収・archive・readerは元45分STOP内の2CPUで行い、rootのcloud resource controlsが管理する。

## 完了証拠を検証してから行うscreen

trusted `analyze.py` がsource/control/両binary、buildの別boot、measurement同boot、
全38solve、supervisorの終了/cleanup、全state gzip展開bytes、qualityと順序/期限を検証する。
欠落・未完了・不一致は `not_evaluable` とし、groupsを出さず、speedup/adoptionを宣言しない。

candidate/baselineのCFR中央値比は両caseの32workersで<=0.95、16workersで<=1.03。
全case/workerでquality中央値比<=1.05、process RSS最大値比<=1.10。
全arm/case/workerのCFR/quality各3標本のmax/min<=1.15を同時に要求する。
全標本、中央値/min/max、CPU秒、CPU/wall、構築、CFR+quality、state書込み、全process、RSSを保存する。
同armの16→32速度比と効率を出すが、1worker標本との強スケール比は算出しない。

この短いN16 screenはkernel候補選別に限る。同品質到達までの長期時間、I/Oを含む全工程、
他range/board/precisionへの適用と全workspace fmt/clippy/testは本体採用前の別条件として残る。
process CPUはspin/allocator/schedulerも含み、差だけから原因を認定しない。
ローカルでは小さいsource/contract testのみで、Cargo・solver・archive展開を実行しない。
