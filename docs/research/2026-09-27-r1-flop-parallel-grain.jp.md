# Flop CFR の chance 並列粒度を比較する未採用提案

これはソースと既存実験を根拠にした研究提案であり、実行計画の承認、本体変更、VM作成や追加支出の許可ではない。作業状態・担当・次のアクションはLinearを正本とする。ここでは計測やengineの編集を行っていない。

提案は、同じbaselineで **CFR中だけ `ParConfig.chance_depth` を2から1へ変える**比較である。CFR終了後、quality評価前に2へ戻す。最初のchance階層だけを並列化し、その下のriver処理を各task内で逐次実行することで、nested fan-outに伴う仕事が減るかを調べる。fused-update、flat出力、worker scratch再利用や別の演算変更は混ぜない。

## 固定するソースと確認できる構造

以下の行番号はこのSHA256の原本に対するもの。後続実装では全workspace、Cargo.lock、compiler、依存、adapterも別途固定する。

| 原本 | SHA256 |
|---|---|
| [engine solver.rs](../../crates/engine/src/solver.rs) | `69063b31c3433b2eb4798b2f41015311200301a7ba1887b05bbefdb936d4e81a` |
| [engine scratch.rs](../../crates/engine/src/scratch.rs) | `d5a5a5379b365896d94554c6b5e16ec1808e51dd605c6c0ae6dfd024fe6c5d49` |
| [既存CPU adapter](../../experiments/hu-postflop-r1/flop-scaling/cpu-occupancy/adapter/solve.rs) | `a6746b4316216231f3a4bf02120968d7b78ddb876d6d04cd611bf1efed7ba2a5` |

`solver.rs:101–107`はroot配下にchanceがあればActionPlanを作らない。Flopでrootや途中のaction兄弟が並列になるわけではなく、823–840行と916–938行の逐次ループからchanceへ到達した際に並列処理のまとまりが生じる。chanceの子への再帰もaction planを`None`にする（699–708行）。chanceを含まない木のRiver action-grain方式は[architecture.md](../architecture.md)の115–126行に記されているが、その方式による改善をFlopに適用済みとは扱えない。

chanceは全通過点でbudgetを減らし、`budget > 0 && num_children >= min_children`だけで分割する（649–650行）。hand次元、subtree storage量やterminal仕事量によるgrain判定はない。CFRの並列chanceではchild IDs、spans、storage views、出力Vec群を作り（657–714行）、task分割単位の`map_init(Scratch::new, ...)`を使う。全子の結果を集めた後、親が子順に合成する（715–718行）。これはallocation・同期・親側逐次処理の具体的な候補箇所であり、その時間割合を測定した結果ではない。

ActionViewsも後続に分割可能なchanceがあると、親actionごとにspansと分割viewを作る（577–592行）。depth1なら最初のchance以降のbudgetが0になり、この先ではAmbient経路を選べる。逐次経路のscratchは同じ再帰内で再利用される。一方、`scratch.rs:3–9`のsteady-state無allocationという説明を、並列task全体の無allocation保証へ広げることはできない。`solver.rs:624–626`は並列taskの局所scratch寿命を区別している。

player passは265–289行、iterationは300–303行で順次実行する。更新順を維持するこの依存関係を今回の変更対象にしない。value walkも独自にchance fan-outとordered foldを持つ（1038–1080行）が、今回のquality設定はdepth2のまま固定する。

## 観測から分かる範囲

[VM16診断](../../experiments/hu-postflop-r1/cloud/vm16/flop-cpu-occupancy-analysis01.json)では、guest16core/32logicalで16から32workersへ増やしたCFRのprocess CPU秒はおよそ2倍となり、wallはnarrowで約8.4%、expandedで約4.8%長かった。expandedのquality7walkは約10.7%短縮した。この差は処理を分けて評価すべき理由になる。

32logicalは32physical coreではない。process CPUには有用な演算、buffer zeroing、allocator、schedulerや待機中の仕事が混在するため、この観測だけでSMT、memory bandwidth、Rayonのspin、負荷偏りのいずれかを原因と断定できない。depth1比較もnested task・allocation・storage view分割が一緒に減るため、それらを個別に分離するprofilerにはならない。物理32coreでのほぼ線形な速度向上も、このguest上の比較から認定しない。

fixtureは既存adapterのnarrow（root support34/30）とexpanded（63/160）を使う。両方とも147,104 action、1,034 chance、219,524 terminalという固定木である（adapter245–263行）。全range・任意のbet treeを代表すると仮定しない。最初のchance階層で使える子数やsubtreeサイズの分布も計時外で保持する。子数が少ない・偏っている場合、depth1ではworkerに十分な仕事を配れない可能性がある。

## 次の有限screenの案

同一VM・測定boot・CPU affinityの一つのbaseline binaryで、F32/DCFR/N16、CFV capture=false、同じ2入力を比較する。小型2CPUでfresh release buildとcore testsを先に済ませ、`RUSTFLAGS=-C target-cpu=x86-64-v3`を固定する。ビルド前と32CPU測定前にCPU・OSのv3対応を検査し、非対応なら中止する。build bootとmeasurement bootは別々の証拠として保持し、全測定条件は同じ32CPU boot・binaryで実行する。過去のnative buildとの絶対時間比較は行わない。新しい研究adapterはCFR用depthを引数として1または2に固定し、invocationに`cfr_chance_depth`と`quality_chance_depth=2`を別々に記録する。CFR終了後の`solver.set_par`でqualityをdepth2へ戻し、state、EV/BR、public exploitabilityの定義と演算を変えない。engineや公開defaultは変更しない。

測定matrixは次の32プロセスに固定する。

| 軸 | 条件 |
|---|---|
| 入力 | narrow / expanded |
| CFR chance depth | 2 / 1 |
| workers | 16 / 32 |
| round | 0がwarmup、1–3が測定 |
| 合計 | 2 × 2 × 2 × 4 = 32（warmup8、測定24） |

case外側とし、偶数roundはworker昇順・depth2→1、奇数roundはworker降順・depth1→2の交互順を事前固定する。途中結果に応じて反復数、worker、case、順序、標本数を変えない。1workerは入力ごとの独立canonical取得用に限り、このmatrixを1→32のstrong-scaling認定には使わない。

matrixとは別に、元workspaceの関連core tests、narrow/N2のdepth1/2×1/32workersの4 smoke、baseline depth2/1worker/N16の2 canonicalを先に要求する。既存の[parallel.rs](../../crates/engine/tests/parallel.rs)642–655行はmapped chanceをdepth0/1/2で、[value_scratch.rs](../../crates/engine/tests/value_scratch.rs)117–123行はzero/可変次元をdepth0/1で扱う。これらの既存試験はFlop性能の代用にはしない。先行条件が満たせなければmatrixを始めない。

全予定プロセスのterminal状態、同一source/binary/boot、完全state bytesとquality JSON bytes、canonical gzipの復元hash/length/header、重複raw除去前の保持receiptを照合した**後**に集計する。CFR wall/CPU、quality7walk wall/CPU、whole-process RSSと各3標本を分ける。未完了prefixの正しさ照合は可能でも、採否用の標本選択や平均・中央値計算に転用しない。

screenを採否判断へ使う場合の候補基準は、depth1/depth2のCFR median ratioが両入力の32workersで<=0.95、16workersで<=1.03、quality median ratioが全条件で<=1.05、root RSSの3標本max同士のratioが<=1.10、各depth/入力/workerのCFRとqualityを別々にmax/min<=1.15、とする。これは未確定の提案値であり、採用する基準は新しい測定結果を見る前にprotocolとして確定する。CPU消費比は原因分離の参考値で、wall改善の代わりに合格させない。本体やdefaultへの採用前には全workspace fmt/clippy/testsと必要なレビュー・文書同期が別途必要になる。

## 時間と費用の制約

[VM17の厳格reader](../../experiments/hu-postflop-r1/cloud/vm17/flop-fused-update-analysis01.json)は未完了をnot_evaluableとした。[別のprefix監査](../../experiments/hu-postflop-r1/cloud/vm17/flop-fused-update-partial01.json)は48完了stage（44 solves）のstate/quality整合を確認したが、予定54 solvesの完了を認定していない。native core testsの通過や、揃った部分だけの品質一致を、性能screenの完了と取り違えない。

次の時間見積もりは32本のCFR時間の合計だけで作らない。依存fetch、resize/start、source準備・pin、fresh native build、core testsの追加compileと実行、smoke、canonical、各solve全体、全state比較、gzip、fsync、毎stageのidentity検査、最終manifest、quiescence後のreader、回収・削除までを含む。従来runnerは各solve開始に上限90秒+10秒の余裕を要求するため、短いsolveの期待時間だけを最後まで詰め込むこともできない。work deadlineと回収用reserveは分け、作成要求からの絶対STOPを延ばさない。

新protocolの上限は作成要求から60分後STOP、2CPU build完了を作成要求+20分まで、32CPU測定を20分以内、回収余裕を15分とする。buildとcore testsは各480秒、solveは各90秒。測定開始前に20分+回収15分が残らなければdispatchしない。buildはRSS監視4GiB・外側6GiB、測定はRSS監視8GiB・外側12GiB、swap0、free memory・disk各2GiB、通常proof240MiB。各stageの最大時間の合計がwork枠に収まるとの保証ではなく、途中で上限に達した場合は未完了として終える。実行時の固定契約は[新protocol](../../experiments/hu-postflop-r1/flop-scaling/chance-grain/protocol.jp.md)と[VM18制御](../../experiments/hu-postflop-r1/cloud/vm18/README.md)に置く。archiveと転送片は再起動後も残る`/opt/r1`へ保存する。

費用節約のため、同一engine・一つのadapter binaryで設定値だけを比較し、matrixから1workerの反復測定を外す。ただし先行検証・raw保持・回収は省略しない。全工程が時間・転送・費用枠へ収まらない場合は起動前に設計を縮小して固定し直すか、実行しない。データ取得後のprotocol変更、失敗標本の差替え、別boot混合はしない。

実行には使用可能予算の確認と有限な予約が必要である。ユーザーが許可した累計$40の範囲内で、確認済みの使用可能額を使う作業は追加確認を要しない。追加予算など許可範囲を広げる場合だけ別途ユーザーの判断を求める。この提案は未確定請求や将来の返還を使用可能資金と見なさず、新しいVM・支出・ローカルの重い計算を許可しない。
