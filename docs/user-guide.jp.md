# Solvers 利用ガイド

`solvers`は共通Input `solvers.nlh/v1`からP1（NLH HU Postflop）またはP2（NLH Multiway Preflop）を解く。
P1は2人が残ったFlop/Turn/Riverのstreet開始、P2は2〜9人のPreflop rootを扱う。
入力は[共通Input規範](nlh-input-v1.jp.md)、全flagは[CLI reference](cli-reference.jp.md)を参照する。
P2の方式・品質は暫定であり、3人以上の結果にNash/GTO保証は無い。

## 1. buildする

Rust stable、Cargo、Gitを用意する。OS別の準備は[development.md](development.md)を参照する。

```sh
cargo build --release -p cli -p daemon
```

binaryは`target/release/solvers`と`solversd`（Windowsでは`.exe`）である。
以下はbinaryをPATHへ置いた場合の例である。

## 2. spotを書く

[examples索引](../examples/README.md)から製品・目的に近い例をコピーする。
P1の小さい例は[river_small.toml](../examples/hu-postflop/river_small.toml)、
P2は[3max_smoke.toml](../examples/mw-preflop/3max_smoke.toml)である。
[bench](../examples/bench/README.md)は性能用であり、入門例ではない。
templateから始める場合は次を使う。

```sh
solvers config new --product p1 --template minimal --out spot.toml
solvers config new --product p2 --template full --out table-full.toml
```

先頭のschemaは常に`solvers.nlh/v1`である。table、economics、spot、ranges、treeを編集する。
製品はtable・line・boardから決まる。P1のpot・stack・OOP/IPはlineを再生して導出する。
金額はBB、内部gridは0.001 BBである。P1のrange指定やline文法は規範の例に従う。
P1ではPreflop専用のtree設定は効果を持たず、既定値以外を書くと警告が出る。
`checkdown`は一致した手番をcheck、賭けに直面していればfoldにする。
外部`.tree`を使う場合はconfigからの相対pathと`tree.params`を記し、
[river_script.toml](../examples/hu-postflop/river_script.toml)を参照する。
旧configの自動変換は無いため、現行例から書き直す。

## 3. 検証して実効入力を保存する

```sh
solvers validate spot.toml
solvers validate spot.toml --format json --show-effective
solvers validate spot.toml --write-effective spot-effective.toml
solvers validate spot.toml --resources
```

製品・開始状態・暗黙fold・tree ruleを確認する。実効入力は既定値を展開し、外部scriptをinline化する。
P1は木と3種類のstorage bytes（JSON: `f32Bytes`・`i16Bytes`・`i16F32avgBytes`）を見積もる。P2の通常validateは木を歩かず、ruleHitStatusは`not-checked`となる。
P2の`--resources`はpublic treeのarena countとrule hit測定を行うため、大きい木では時間が掛かる。
`complete`なら未一致ruleの警告を確認する。`incomplete`なら未使用ruleの判定は保留である。
withinLimitがfalseならmemoryまたはtreeを見直す。validateの成功はsolve可能なmemoryや品質を保証しない。

## 4. solveする

```sh
solvers solve spot.toml --out runs/my-spot --threads 4 --memory 2GiB --max-time 5m
```

毎回新しいrun directoryを指定する。overrideはrun.tomlへ保存する。
P1の`[solver.stop] check_every`は`"auto"`が既定（PF10、2026-10-08）。targetがあると初回25 iteration、以降は評価値から到達を予測した3〜50 iterationの適応間隔で測り、`NashConv / 2 <= target`で止まる。target無しは固定25、整数の明示は従来の固定間隔である。target有りでは停止iterationが旧版と変わり得るため、旧版と同じ停止を得るには`check_every = 25`を明示する。実効configはautoか整数を必ず保存する。
P1の`[solver] storage`は`f32`（既定、両arena f32）、`i16`（両arena i16、memory最小）、`i16-f32avg`（regret i16、戦略累積f32）を選べる。0.1% pot程度を目指し旧i16が頭打ちになる場合は`i16-f32avg`を使う。regretの量子化誤差は残る。storage bytesは順に8L、4L＋8N、6L＋4N（L=要素数、N=action node数）。

P1のmemory見積りは`max(storage, storage − regret + 保存作業領域) + 圧縮予算`。regret bytesはf32で4L、i16・i16-f32avgで2L＋4N。solve/resumeは最後のcheckpointを新たに書き、並行peak `S + W + 2C`（S=選択storage、W=保存作業領域、C=圧縮予算）が上限以下ならregretを保持してcheckpointと`.sol`を並行生成する。その他は最後のcheckpoint（指定時）→ regret配列とscaleの解放 → `.sol`生成の順とする。全停止理由に適用し、solve開始の見積り式は変わらない。反復中のcheckpointでは解放しない。保存作業領域は保存用packed値block・sref slot・保存対象/street配列、上限付き1 batch分の並列戦略作業領域、圧縮作業予算を含む。戦略blockは合計8,388,608要素以下のsref連続区間ごとにrun threadsで並列生成し、sref順に出力する。上限を超えるnodeは単独batchとし、同時に保持するbatchは1個。全node分は保持しない。NoRiversもfullの保守的な見積りを使う。木・rank table・thread scratch等は別途必要でRSS上限ではない。
P1のmemory autoは物理RAMの80%、P2はarena予算6 GiBである。P2のmemoryはRSS全体の上限ではなく、
tree・cache・thread scratch・評価・checkpoint用の追加memoryが必要である。
P2のEHS² tableは初回に構築し、以後はmachine cacheを利用する。
cache場所は`--cache-dir`または`SOLVERS_CACHE_DIR`で指定する。

P1はNashConv / 2の目標、P2は測定deviationの目標を検査する。
反復・sweep・時間の上限で終わったことは品質目標の達成を意味しない。
保存物はrun.toml、manifest.json、events.jsonl、progress.jsonl、run.jsonと、
P1のcheckpoint.ckpt / solution.solまたはP2のcheckpoint.mwckpt / solution.mwsolである。
P1の`[solver] cfr_precision`は`"f32"`（既定）または`"f64"`（旧版とbit一致）。CFR終端とcurrent strategyだけに作用し、評価・平均戦略・保存EVはf64を使う。

P1の保存streetは`[output] solution_streets`で設定する。
no-riversはRiverの戦略と値を省く。

## 5. 監視・停止・再開する

P1の`[run] final_checkpoint = false`は終了時の再開state保存を省く設定で、既定はtrue（PF9、2026-10-08）。目標到達・iteration上限・時間上限・cancelの全てに適用し、定期checkpointと`.sol`は従来どおり保存する。保存した`run.toml`を編集してresumeにも適用できる。P2では指定不可（`NLH002`）。省いたrunは最終状態から再開できず、最後の定期checkpointから続ける。定期checkpointも無ければ`run.toml`から再solveする。再開時のprogressはcrash再開と同じ追記方式で、checkpointより後の既存行も残るためiterationが重複し得る。
autoの評価は時間・thread数に依存せず、progressは評価ごとに追記するためtarget有りでは不等間隔になる。中断・max_time・定期checkpointの判定はautoで25 iteration以下のsub-batchごと、整数で指定間隔ごとに行う。
autoの再開はprogress.jsonlのcheckpoint iteration以下の行を使い、同じiterationの重複は最後の行を採用して次の評価を再計算する。同じconfigで一度に解いたrunと評価iteration・停止iteration・stateが一致する。progressが無い・読めない場合は履歴無しとしてcheckpoint iteration＋25（max_iterationsで切る）から評価するため、評価iteration一致を保証しない。裸のcheckpointだけを移した場合も同様である。checkpoint形式は変わらず、旧run.tomlの`check_every = 25`は固定25で再開する。

P1のDCFRは`[solver.algorithm]`の`alpha = 1.25`、`beta = 0.5`、`gamma = 4`が既定である。`pow4_reset`未指定時は`[solver] storage = "i16"`ならtrue、`"f32"`・`"i16-f32avg"`ならfalseとなる（利用者決定PF8、2026-10-07）。明示したtrue・falseはどのstorageでも優先する。full templateはf32用の`pow4_reset = false`を明示しているため、storageをi16に変えて既定resetを使う場合はこの行を削除するかtrueにする。
保存された実効configは係数とresetを明示するため、旧係数1.5・0・3や旧既定reset=trueのrun・checkpoint・solutionは保存値で再開・照会できる。
旧runの`run.toml`を係数や`pow4_reset`未指定の元configに戻すと、新既定との互換性hash不一致で再開を拒否する。
P1の互換性hashは`[run]`・`[meta]`・`solver.cfr_precision`を除外する。精度keyの無い旧runは新既定f32で再開する。旧版と同じ計算には`cfr_precision = "f64"`を明示する。
旧係数で再solveして比較する場合は`alpha = 1.5`、`beta = 0.0`、`gamma = 3.0`を明示する。旧i16のreset無し条件を再現するには`pow4_reset = false`を明示する。reset有り条件は`pow4_reset = true`を明示する。`compare`はalgorithmの差を拒否しない。

```sh
solvers status runs/my-spot --format json
solvers watch runs/my-spot --from 0
solvers runs ls runs
solvers resume runs/my-spot --max-time 10m
solvers resume runs/my-spot --out runs/my-spot-fork --threads 2
```

statusのeventsOffsetからwatchを再開できる。watchのCtrl-Cは監視だけを止める。
solveのCtrl-Cは境界で停止し、checkpointを保存する（P1で`final_checkpoint = false`なら終了時の保存を省く）。P1は同じiterationの終了時再保存とcheckpoint eventを省く。2回目は即時終了する。
同じdirectoryへのresumeは累積進捗を引き継ぐ。max_timeも再開前を含む累積時間である。
P1ではrun設定とmeta以外を変更できない。P2では必要に応じて
`--max-sweeps`・`--stop-target`・`--evaluation-samples`・`--evaluation-cadence`を上書きする。
裸の自己完結checkpointからの再開には`--out`で新directoryを指定できる。

旧runのstatus・watch・runs lsも利用でき、JSONのconfigSchema / gameKindは記録値のままである。
旧runのresumable表示はcheckpointの存在を示すだけで、現行CLIでは再開できない。
`.sol` version 1とcheckpoint version 1/2/3/4は読めない。現行configから再solveして`.sol` version 2・checkpoint version 5を作る。

## 6. 結果を読む

```sh
solvers export runs/my-spot/solution.sol summary
solvers export runs/my-spot/solution.sol ev --node all --format csv --output ev.csv
solvers export runs/preflop/solution.mwsol strategy --format csv --output strategy.csv
```

summary、tree、actions、strategy、range、evをJSON/CSVで読む。
P1は開始rangeでweightが正、かつ開始boardと矛盾しないhandだけを計算・保存する。weight 0のhandは出力しない。
P1のcash EVはspot開始potを基準としたBB、ICM EVは賞金単位である。
P2のcash utilityはhand開始stackからのBB増減であり、P1の基準とは異なる。
field・単位・未保存値は[P1規範](hu-postflop.jp.md)・[P2暫定規範](mw-preflop.jp.md)を参照する。

### P1 viewerとboard比較

```sh
solvers inspect --sol runs/my-spot/solution.sol
solvers inspect examples/hu-postflop/river_small.toml --iterations 100
solvers report examples/hu-postflop/river_small.toml --boards "2c7d9hJsQs,2c7d9hJsKs" --output report.csv
```

REPLのshowでactionを確認し、goで子へ進み、grid・combosで戦略、eqで到達rangeのequityを見る。
up/rootで戻り、quitで終える。evはroot summaryで、hand別EVはexport evを使う。
no-riversの未保存Riverをviewerで開くと再solveする。再計算値を保存時の値と同一視しない。
reportはlineに合うboardを指定し、board別root集計をCSVにする。

### P2の照会・再評価・比較

```sh
solvers inspect runs/preflop/solution.mwsol --node root --view strategy
solvers evaluate runs/preflop/solution.mwsol --samples 4096 --seed 1 --br-traversals 20000
solvers compare runs/a/solution.mwsol runs/b/solution.mwsol
```

evaluateは保存averageを復元し、deviation gainとCI、coverageを再測定する。
未訪問columnと量子化前の値は復元できない。小さいgainをexploitability上界やNash保証として読まない。
coverageは訪問内訳であり、全branchの学習率ではない。
compareは通常同一gameを要求する。`--cross-game`でもseat mappingとutility単位は一致が必要である。
P1のsolution同士もcompareできるが、treeと保存node集合の一致が必要である。
P2のPreflop-only出力は未達で、現行artifactにPostflop blockが含まれうる。

### P2からP1の入力を作る（derive）

P2でPreflopの平均戦略を保存し、2人が残るlineとFlopの3枚を指定する。
小さいsmoke runでは未訪問のclassが残りうるため、以下では追加sweepを行う。

```sh
solvers solve examples/mw-preflop/3max_smoke.toml --out runs/derive-preflop
solvers resume runs/derive-preflop --max-sweeps 64 --evaluation-cadence 65
```

P1用のbaseは単独で有効なspotでなくてよい。次を`p1-base.toml`へ保存する。
これはFlopの最初の手番をall-inに限定する小さい動作確認用の木である。
用途に合わせてtreeと停止条件を変更する。

```toml
schema = "solvers.nlh/v1"
[tree]
script = 'flop when unopened { force bet [a] } turn, river { checkdown }'
[solver.stop]
max_iterations = 2
[run]
threads = 1
memory = "64MiB"
```

```sh
solvers derive runs/derive-preflop --line "BTN c, BB x" --board "Ks 7h 2d" \
  --base p1-base.toml --out derived-flop.toml
solvers validate derived-flop.toml
solvers solve derived-flop.toml --out runs/derived-flop
```

このlineではSBが暗黙にfoldする。指定できるのはPreflopだけのlineとFlop開始であり、
暗黙foldを含むaction・sizeはP2の木と完全一致が必要である。foldした席のcard removalは使わない。
未訪問のnode×classはweight 0として除き、席ごとに警告する。全weightが0の席があれば生成しない。
baseなしではP2のtreeを引き継ぎ、Flop以降の`checkdown`を警告する。
baseのtable・economicsがP2と違えばerrorとなり、spot・rangesは生成値で置き換える。
生成物は出所を`meta.derived_from`へ記録した自己完結の実効Inputであり、通常のP1操作で扱う。

## 7. daemonから実行する

```sh
solversd --runs runs --max-concurrent 1
```

表示されたbearer tokenを全HTTP requestのAuthorizationへ付ける。
POST /v1/validateまたはPOST /v1/runsへJSONのconfigTomlを送る。
外部source参照は拒否されるため、先にvalidate --write-effectiveでinline化する。
daemonは同じCLIを起動し、run directoryに状態を保存する。再起動後もrun rootから一覧を復元する。
非loopbackで公開するときはTLS証明書・秘密鍵を指定する。
HTTPのsolution viewはP2のみ。P1はartifactを取得してCLI viewer/exportを使う。
endpointとflagは[CLI reference](cli-reference.jp.md#solversd)を参照する。

## 8. errorから対処する

| 診断 / exit | 対処 |
|---|---|
| `NLH001` / 2 | schemaを`solvers.nlh/v1`にした現行例から入力を書き直す |
| `NLH002` / 2 | 未知key・型・製品に適用されないkeyを確認 |
| `NLH003` / 2 | 値、range、tree条件・size・最終menuを確認。script位置も読む |
| `NLH004` / 2 | lineの綴り、action順、合法min-raise・callを確認 |
| `NLH005` / 2 | 開始street・board・残player数が製品の対象か確認 |
| `removed config family ...` / 3 | 現行configから再solve。旧solution/checkpoint/runは照会・再開できない |
| 非互換・破損 / 3 | embedded config、fingerprint、artifact version、fileの完全性を確認 |
| 資源上限 / 75 | validate --resourcesで規模を確認し、treeやmemory予算を見直す |
| 協調停止 / 130 | 保存checkpointからresume |
| その他 / 1 | file path・権限・I/O診断を確認 |

未一致ruleのwarningはerrorではない。P2のnot-checked/incompleteは測定未完了を示す。
P1の一般和設定やP2の暫定方式の品質制限は規範を読み、予算終了と収束を区別する。
