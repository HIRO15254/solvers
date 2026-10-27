# アプリケーション設計: CLI とジョブ daemon

## 0. 本書の位置づけ

本書は現行の`solvers`・`solversd`・run directory・HTTPとviewerの境界を記す。
公開操作は[CLI reference](cli-reference.jp.md)、入力は[共通Input規範](nlh-input-v1.jp.md)、
計算と保存は[P1規範](hu-postflop.jp.md)・[P2暫定規範](mw-preflop.jp.md)、
crateの構造は[architecture.md](architecture.md)を参照する。
作業状態は[Linear](status.jp.md)、移行手順は[再構築計画](plans/two-product-restructure.jp.md)に置く。

## 1. 利用モデル

CLIは入力を読み、製品APIへdispatchし、計算・保存・queryの表示を行う。
daemonはCLIを子processとして起動し、job投入・queue・監視・cancel/resume・artifact配信を行う。
local/remoteは同じCLI/HTTPを使う。clientの接続や寿命にsolveを依存させない。
永続状態はrun directoryにある。daemonはそこから再接続と再起動時の状態を復元する。

## 2. 境界を分ける理由

solverの実行をCLI子processへ集約し、cancel・checkpoint・進捗のコード経路を共有する。
arenaとprocess障害をjob単位に分離する。process起動とファイル経由の進捗伝達はこの境界の費用である。
長時間のsolveと再接続にはrun directoryとappend-only eventを使う。

## 3. レイヤと 3 つの境界

```text
HTTP client
    │ ① protocol: versioned JSON over HTTP
solversd (daemon)
    │ ② run directory + CLI child process
solvers (cli)
    │ ③ 製品のRust API + caller-owned observer/diagnostics
hu-postflop / mw-preflop       runfiles
```

CLIのnormal workspace依存は`spot`・`hu-postflop`・`mw-preflop`・`runfiles`である。
`spot`が共通InputとSpot IRを、製品がgame/session・資源判定・solve/resume・成果物とtyped queryを持つ。
P1のpayoff builderは`hu_postflop::input::NlhPayoff`であり、共通のrake/ICMは`economics`にある。
P2のsession・abstraction・停止評価は`mw_preflop::{prepare,session,run,views}`にある。

製品APIはcancel flagを明示的に受け取り、typed observationとdiagnosticをcallbackへ渡す。
cacheを使うP2のAPIはcache rootも受け取る。CLIは場所を選び、進捗を保存し、eventとexit codeを写す。
checkpoint・solution・評価cacheの入出力は製品が所有する。CLIはrun directoryを作成/採用し、
`run.toml`・manifest・events・progress・`run.json`のlifecycleとJSON/CSV/人向け表示を担う。

`protocol`はHTTP DTOと`runfiles`の型を使う。`daemon`のworkspace依存は`runfiles`と`protocol`だけである。
domain crateはHTTP・job・画面状態を知らない。依存図は
[architecture.md §2](architecture.md#2-レイヤ構成と-workspace)を参照する。

## 4. 決定事項

以下の R 番号はアプリケーション境界の識別子であり、product roadmap の R0〜R7 とは別である。

### R1. ソルバーを実行するのは CLI プロセスだけ

daemon は `solvers solve --out <run-dir>` や `resume` を起動し、自身では解かない。
プロセス管理と solver の cancel/checkpoint 処理を分け、local/remote で同じ solver を実行する。

### R2. 永続状態は run directory。daemon は job DB を持たない

job id は run directory 名であり、再起動時は runs root から job を読み直す。
manifest が `running` のまま owning process が消えた run は `interrupted` として報告する。
pid の判定は実行 host で行う。中断した計算の再開は checkpoint と明示的な resume 要求を使う。
Windowsはprocess終了とcreation timeも検査する。creationがrun開始時刻より1秒超遅い同じpidは再利用とみなす。access deniedはalive扱いである。

### R3. 監視は追記専用 event の offset で再開する

client は既読の byte offset を保持し、切断後はその位置から読み直す。
`seq` の連続性で欠落を検出し、書込み途中の最終行は完成後に読む。
数値の時系列と離散 event は別ファイルにする。

### R4. config の検証・正規化は Rust 実装へ集約する

daemonはCLIの`validate` / `line` / `tree`子processへ委譲する。CLIは`spot`と製品APIを呼び、
既定値・line replay・合法手とエラーの意味をdaemonへ複製しない。
規範・実装・test・文書の同期規則は [AGENTS.md](../AGENTS.md) に従う。

### R5. GUI 専用の solver 経路を作らない

計算・監視・成果物の意味は CLI でも確認できることを保つ。
`status` / `watch` / `runs ls` は run directory を読み、daemon の solution view は
`solvers export` へ委譲する。HTTP transport 固有の認証や queue と solver の機能を区別する。

### R6. production config は schema 必須、solve の出力は run directory

公開入力は`solvers.nlh/v1`だけであり、table・line・boardが製品を決める。
schemaの省略・未知・削除済みfamilyは`NLH001`で拒否する。
旧入力を埋め込んだ成果物の照会・再開はexit 3で拒否し、現行入力からの再solveを案内する。
status・watch・runs lsは旧runの記録も読み、configSchema / gameKindを保持する。

`solve --out` と `resume` が run directory の lifecycle を共有する。
P1の保存範囲は`[output] solution_streets`で選ぶ。
postflop は戦略と値を `solution.sol` に保存して `export` で読み、第二の JSON 出力や `--history` は持たない。
詳細・override の可否は CLI reference と規範仕様へ置く。

lowered config は内部 IR として残るが、schema なしの手書き lowered TOML を公開入力として
受理しない。既存構造体を使うことと、retired input を再び受け付けることを区別する。

### R7. remote は同じ daemon を別 host で動かす

loopback でも bearer token を要求する。非 loopback bind は TLS なしでは拒否し、
TLS を使わない remote 接続では SSH tunnel 等を介して loopback へ到達する。
現行のHTTP clientは同じURL/tokenのprotocolを使う。

### R8. production と研究経路を区別する

production の公開 schema は規範仕様で定義する。研究用 example や feature が存在することは、
同じ機能を production config から利用できることを意味しない。
`experiment` CLI namespace は持たず、研究の入出力・制約は対応する example の説明と実験証拠へ置く。
研究結果の採用には通常の契約・identity・検証の更新を伴わせる。

### R9. キャッシュは machine スコープ、config は run スコープ

cache root の解決順は `--cache-dir`、`SOLVERS_CACHE_DIR`、OS の user cache directory。
[cli/src/cache.rs](../crates/cli/src/cache.rs) が cache root を決め、root 内の EHS² のファイル名は
[card_abstraction/cache.rs](../crates/mw-preflop/src/card_abstraction/cache.rs) が決める。
cache は再生成可能な計算資産であり、run directory ごとに複製しない。

EHS² の名前は format version と bucket 数を含み、異なる設定を併存させる。
[Ehs2Abstraction::save](../crates/mw-preflop/src/card_abstraction/buckets.rs) は writer ごとに一意の
pid/nonce を含む一時ファイルを書いて rename する。同じ内容を並列に構築することの抑止と、
ファイルを壊さず保存することは別問題であり、前者の lock/coalescing は実装済みとは扱わない。

現行はbucket idを保存する単層cacheを使う。P1はmachine cacheを使わない。

### R10. remote へ送る config は self-contained

受信 host の filesystem で `tree.source` を解決すると、同じ config が別ゲームを指し得る。
local `validate --write-effective` は config file の directory を基準に script を解決し、
本文を inline にして `params` を保持する。run の `run.toml` もこの effective config を使う。

daemon は config text を受け取り、path を含む入力を解決せず拒否する。
client は source を解決した effective config を送る。`POST /v1/validate` は受信 host の
任意ファイルを読み込む API ではない。cache path も config に埋め込まず、machine 設定として渡す。

### R11. queued run もディスク上に存在する

job 受理時に `run.toml` と `queued` manifest を書く。起動時は queued run を再投入するが、
interrupted run を自動 resume しない。計算の再実行は明示要求を受けて行う。

CLI の `create_or_adopt` は absent/empty directory と、引き取り可能な queued directory を受け付ける。
queued の場合は manifest の状態と、`manifest.json` / `run.toml` / `stdout.log` に限定された内容を確認し、
任意の既存 run を上書きしない。同時実行は固定 slot 数の FIFO で管理し、operator が memory 予算に
合わせて指定する。設定値の既定は CLI reference を参照する。

## 5. run directory 契約

### 5.1 レイアウト

```text
<runs-root>/<run-id>/
├── run.toml             # 実行した effective config
├── run.lock             # process寿命の排他OS lock。artifactではない
├── manifest.json        # identity / state。atomic replacement
├── progress.jsonl       # 製品別の定期数値サンプル
├── events.jsonl         # state / notice / checkpoint / stop / failure。追記専用
├── run.json             # 結果サマリ
├── checkpoint.mwckpt    # Multiway の再開用 state
├── solution.mwsol       # Multiway の閲覧用 artifact
├── checkpoint.ckpt      # HU系の再開用 state (Multiwayとは別形式)
├── solution.sol         # HU postflop の戦略・値
└── stdout.log           # daemon が起動した child の stdout/stderr
```

これは製品別のファイルをまとめた図であり、すべての run が全ファイルを生成するわけではない。
定数と DTO は [runfiles/src/run.rs](../crates/runfiles/src/run.rs)、lifecycle は
[cli/src/run_dir.rs](../crates/cli/src/run_dir.rs) が所有する。
solve/resumeはrunfileの更新前に専用`run.lock`の排他OS lockを取得し、process終了まで保持する。busyはdirectoryとmanifest pidを含む型付きerror（exit 1）で拒否する。`--out`のforkは出力先だけをlockしてsourceを書き換えない。残存file自体はlockの証拠ではなく、crash時もOSが自動解放する。`create_or_adopt`の非空判定から`run.lock`を除くため、queued adoptと空directoryの判定を変えない。
進捗と event を分けることで、時系列 schema を保ち、読取り側に event の除外処理を要求しない。
P1の`solver.stop.check_every`は`"auto"`が既定。target有りのprogressは初回25 iteration、以降3〜50 iterationの適応評価ごとに追記するため、GUIは等間隔を仮定しない。target無しのautoは固定25、整数は固定指定間隔である。
autoでは中断・max_time・定期checkpointを25 iteration以下のsub-batch境界で判定する。整数は指定間隔で判定する。
auto再開は`progress.jsonl`のcheckpoint iteration以下の評価行を使い、同じiterationの重複は最後の行を採用する。progressが無い・読めない場合は履歴無しとしてcheckpoint iteration＋25（max_iterationsで切る）から評価し、一度に解いたrunとの評価iteration一致は保証しない。checkpoint形式は変えない。

### 5.2 state 遷移

```text
queued ──spawn──▶ running ──正常終了──▶ completed
                     ├── 失敗 ──────▶ failed
                     ├── cancel ────▶ canceled
                     └── pid 消滅 ──▶ interrupted
```

`interrupted` は owning process が正常な終了状態を書けなかったことを reader が判断した状態。
cancel は solver の協調停止を使い、checkpoint の存在と状態を確認して resumable を判断する。
recording開始後の失敗は`run.json`の書込み失敗を含めてfailedとfailure textをmanifestへ記録する。

### 5.3 manifest.json

`RunManifest` は schema version、run id、game/config identity、CLI version、command、pid、
開始/終了時刻、failure/completion を持つ。状態遷移時に一時ファイルへ書き、rename で置き換える。
wire name と optional field を変更するときは `runfiles` / `protocol` と reader の互換性を検証する。

### 5.4 events.jsonl

```json
{"seq":0,"unixMs":0,"level":"info","kind":"state","state":"running"}
{"seq":1,"unixMs":0,"level":"info","kind":"notice","message":"cache loaded"}
{"seq":2,"unixMs":0,"level":"info","kind":"checkpoint","sweeps":12000}
{"seq":3,"unixMs":0,"level":"info","kind":"stop","reason":"target-reached"}
```

1 行 1 JSON、追記のみ、`seq` は 0 から単調増加する。resume は既存 event の続きへ書く。
reader は byte offset を保持し、未完了の最終行を次回へ残す。
statusの`eventsOffset`も最後の改行までの完全record境界を返す。events/progressの再開writerは最後の改行以降の不完全なsuffixだけをtruncateして追記し、完全行を保持する。readerは不完全な最終行だけを無視する。P1の評価履歴では改行済みの不正JSONは履歴全体を従来どおりfail-closedとする。

## 6. CLI 表面

`config new` / `validate` / `line` / `tree` が入力、`solve` / `resume` が計算、`status` / `watch` / `runs ls` が監視、
`inspect` / `evaluate` / `export` / `compare` / `report` が閲覧・評価を担う。
P1 compareは通常spot・seat別comboリスト・size込みaction labelのidentityも検証し、
`--cross-game`で形状だけの対応付けを明示する。木/index共有は保存index checksumと復元support・root range・開始streetの一致時だけ行い、右index frameも検証する。
`derive`はP2 runとPreflop line・Flop boardからP1の実効Inputを生成する。
range計算は`mw_preflop::derive`、共通document組立ては`spot::derive`、
P1検証は`hu_postflop::prepare`、入出力と表示はCLIにある。daemonにderive endpointは無い。
製品によって対応する操作が異なるため、ここで共通対応を仮定しない。
全コマンド・引数は [cli-reference.jp.md](cli-reference.jp.md) を参照する。

## 7. daemon protocol

型定義は [crates/protocol](../crates/protocol/src/lib.rs)、routing は
[daemon/src/http.rs](../crates/daemon/src/http.rs) にある。

```text
GET  /v1                              protocol/CLI version、同時実行数
POST /v1/validate                      self-contained config の検証・正規化
POST /v1/line                          table-only line prefix replay と次の合法手
POST /v1/tree                          P1 config の solve 前 public tree 照会
POST /v1/runs                          run の作成・投入
GET  /v1/runs                          run 一覧
GET  /v1/runs/{id}                     state と進捗の要約
GET  /v1/runs/{id}/events?from=OFFSET   event page
POST /v1/runs/{id}/cancel              協調停止
POST /v1/runs/{id}/resume              明示的な再開
GET  /v1/runs/{id}/artifacts            契約上の artifact 一覧
GET  /v1/runs/{id}/artifacts/{name}     artifact download
GET  /v1/runs/{id}/solution/{view}      P1 .sol / P2 .mwsol の CLI export への委譲
```

`POST /v1/line`は`configToml`と任意の`moves: string[]`を受け、
CLI `line --format json`のcursor JSONをそのまま返す。moves省略時は`spot.line`の厳密replay、
空配列は空prefixである。卓以外の設定は検証せず、CLIの拒否はHTTP 400
`invalid-config`でstderrを保持する。認証は他の`/v1` APIと同じ。
field・単位・合法tokenは[CLI reference](cli-reference.jp.md#solvers-line)を正本とする。

`POST /v1/tree`は`configToml`と任意の`nodes: string[]`を受け、
CLI `tree --format json`の行配列をそのまま返す。省略・空配列はrootだけを選択する。
P1の製品APIがsolveと共通の木/indexを構築し、exportと同じselectorと行生成を使うが`stored`は返さない。
不正config・P2・未知nodeはHTTP 400 `invalid-config`でCLIのstderrを保持し、runは作らない。
認証は他の`/v1` APIと同じである。

artifact の名前は run directory 契約の allow-list に限る。任意の directory listing を
file share として公開しない。solution view は `solution.sol` があればP1、それ以外では
`solution.mwsol` があればP2をCLI `export`へ渡し、どちらも無ければ`unavailable`を返す。
`format=csv`でCSVを選べる（既定JSON）。P1のみ`node=SELECTOR`を繰り返せ、percent encodingと
`+`をspaceへ復号して指定順で`--node`へ渡す。P2への`node`指定と不正selectorは
HTTP 400 `invalid-request`、その他のexport失敗は`internal`である。追加契約のためprotocol versionは1を保つ。
event は SSE ではなく `nextOffset` と `terminal` を持つ page として返す。
run一覧と詳細の`RunSummary.lastProgress`は最後の`progress.jsonl`行を製品固有のkeyのまま返す
（行が無ければ省略）。既存の`progress`・`elapsedSecs`も保つ。P1のraw行は
`iteration`・`elapsed_secs`・`expl_p0`・`expl_p1`・`nash_conv`を含み、
clientは`nash_conv / 2`でExploitabilityを表示できる。

認証 token は明示 `--token`、`SOLVERSD_TOKEN`、新規乱数の順で選び、起動時に表示する。
自動的に client の設定 directory へ保存する動作は持たない。TLS と bind の条件は R7 に従う。

`solversd --ui DIR`はbuild済みSPAを静的配信する。`--ui`省略時は`current_exe`の隣の
`ui/index.html`がfileとして存在すればその`ui/`を使う。明示`--ui DIR`が常に優先し、
current working directoryは探索しない。自動検出できなければAPI専用となる。
UI配信時も`/v1`と`/v1/`配下の認証は必須で、それ以外のGET/HEADのみtokenなしで
UI directory内のfileを配信する。`/`は`index.html`、
存在しない拡張子なしpathはSPA fallback、拡張子ありpathは404となる。復号後の`..`・absolute・
backslash・drive指定を拒否し、canonical pathがDIR外へ出るfileも配信しない。
`index.html`には`Cache-Control: no-cache`を付ける。起動時は従来の3行の後に
`gui: {scheme}://{host}:{port}/#token={token}`を表示する。UIが無い場合は全requestにtokenが必要である。
`--open`はlistenと`gui:`表示後、そのtoken付きURLを既定browserへshellを介さず渡す。
Windowsは`rundll32 url.dll,FileProtocolHandler`、macOSは`open`、その他は`xdg-open`を使い、
launcherの終了待ちは別threadに置く。browser起動失敗はstderr警告のみでdaemonを止めない。
UI無しの`--open`はlisten前の引数error（exit 2）である。

配布archiveは`solvers-<version>-<target>/`直下に`solvers`・`solversd`（Windowsは`.exe`）と
`ui/index.html`を置く。`start-gui.cmd`（Windows）または`start-gui.sh`（POSIX sh）は自身のdirectoryへ移り、
`solversd --open --runs runs`（Unixは`./solversd`）を実行する。run rootもその配布directory内の`runs/`となる。

## 8. viewer 境界

viewerはP1の対話`inspect`、両製品のartifact query、`web/`のWeb GUIである。
`cli::inspect`はstdin/stdoutのREPL loop、navigation state、command errorとgridの描画を持つ。
live solve、reach・equity・combo・action gridは`hu_postflop::queries`からtyped dataを受け取る。
保存済み戦略は`artifact::SolProvider`を使う。未保存Riverは同じproviderがlazy re-solveする。
保存時のEVと再solveした戦略を混同しない。

P1の`views`とP2の`views`は表示用dataを返す。CLIのadapterがJSON/CSVを符号化する。
daemonのHTTP solution viewは両製品のCLI `export`へ委譲する。
clientはevent pageの`nextOffset`と`terminal`を使って監視を継続する。

Web GUI（React + TypeScript、Vite）はdaemon HTTPだけを使うclientであり、`solversd --ui web/dist`が
同一originで配信する。tokenは`#token=`fragmentまたは入力欄から受け取り、そのoriginのlocalStorageへ置く。

|画面|使うAPI|GUIが持つもの|
|---|---|---|
|Setup|`POST /v1/line`、`POST /v1/validate`（`resources`付き）、`POST /v1/tree`、`GET /v1/runs`、`POST /v1/runs`|P1入力の1つの文書を卓・開始状況・Range・Tree・Solverのstepで編集し、文書全体のTOML直接編集はstepの列から区切った右端のbuttonで開く。spot名・run名・TOMLの保存名は右列の卓の図の上の1つの名前（`meta.name`）で、空なら卓の行動順・line APIが手に残す2人・boardから綴った自動の名前をplaceholderに示し、検証・run投入・保存する文書の`meta.name`へ書く。run名とTOMLのfile名は名前の英数字・`.`・`_`・`-`以外を`_`に置き換え（daemonはURL pathのrun idをpercent-decodeしない）、`GET /v1/runs`の既存runと重なれば`-n`を付ける。右列の卓の図は文書の`[table]`と`spot.line`をline APIでreplayした席・wager・pot・boardと、`seats`の2人のstackの小さい方をeffective、それをpotで割ったSPR、手に残る2人のOOP / IPの印を描き、席の押下で2人のactive playerを選ぶ。卓の図に検証の結果は書かない。右列の検証はvalidate結果のtreeと資源の見積り・warningを示し、error文（`NLHnnn: key: 理由`）が名指すkeyの入力欄の直下に理由を示し、その入力欄を持つstep（無ければTOML、`meta.name`は名前の欄）を開く。右列のTreeは検証が通った文書をtree APIへ渡し、返る行（節点のstreet・手番・pot・terminal・子のaction、chance nodeのcard）を描くだけで、選んだ節点のpathを次の`nodes`として送る。`tree.script`のerrorの行番号はサイズ表が書いた行からcellへ戻すだけで、理由は書き換えない。卓の席ごと入力はstack・blind・anteの全員分の列を`[table]`のkeyへ写すだけで、列の順はline APIのroot（straddle無しの卓）の行動順から得る。lineは文字（カンマと` / `の空白だけを文法の形へ揃える）、その2人が残る経緯（2bet〜5bet・squeeze・limp・iso、固定額、postflopの進行）、またはbadgeから書く。badgeは今のstreetの行動の後に、そのstreetで手番が来る席のpositionだけを並べる。その席は、line APIのreplayで次の手番から順に`next.options`のcheck（無ければfold）を足していき、streetが閉じるまでに`next`になった席であり、GUIは手番の順を持たない。badgeのpopoverはその席の手番での`next.options`を示し、選ぶと補ったcheck / foldに続けて手順（`moves`）に足す。組み立て途中のlineはstrictなline文法では別の意味（残りの暗黙fold）になるため、GUIは押して作った`moves`を、それが綴った`line`が文書に残る間だけ保持してreplayする。bet・raiseの額は`min`〜`max`に入るかだけを見てline APIへ渡す。presetはpreflopの行動順・postflopの先手・potをline APIから受けてline文字列を綴るだけで、GUIでbetting規則を再実装しない。`[ranges]`のkeyはline APIのreplayが手に残す2人（次の手番がOOP）へ付け替え、rangeの中身は変えない。Treeのサイズ表はstreet×OOP/IP×bet/raiseのsize literalを`in_position`・`donk`・`aggressions`の条件だけの`tree.script`へ書き、rule単位で書き戻せるscriptだけを表で開く。sizeの解釈・合法性・未一致ruleはvalidate結果に任せる。line APIのreplayがpostflopのstreetを始めるとき、`boardCards`より多いboardは先頭の`boardCards`枚だけを使い、残りは文書に残したまま薄く示して、検証・run投入・TOMLの保存へは除いたboardを渡す。賞金はpresetから埋める専用dialogで編集する。TOML直接編集は読めた文書をstepと共有し、読めない文書は渡さない。複数行の文字列は`'''`の複数行literalで書く。stepに無いkeyは文書のまま保持し、既定値・検証・正規化はvalidate結果を表示する。templateは`examples/hu-postflop/`をbuild時に取り込み、`meta.name`を外して読む|
|Runs|`GET /v1/runs`、`/events`、`/artifacts`、`cancel`、`resume`|一覧の`lastProgress`と`progress.jsonl`から`nash_conv / 2`を描く。%pot換算はvalidate結果の開始pot、%prizes換算はconfigのpayout合計を使う。stop eventから終了stateまでを保存中として示す|
|Results|`/solution/{summary,tree,actions,strategy,ev,range}?node=`|`tree`の`children`で節点を辿り、chance nodeはlabelからcardを選ぶ。13×13 gridはcombo行を到達weightで重み付けしたclass集計であり、戦略・EVの計算はしない|
|Guide|なし|各画面の操作手順と、このGUIから撮ったscreenshot（`web/public/guide/*.png`、buildで`dist/guide/`へ写す）を並べる静的な頁|

SetupのTree stepはscriptの`param`宣言を入力欄として表示し、変更を`[tree.params]`へ書く。
script本文の宣言既定値は変更せず、validateのparam診断と同じ宣言・overrideを扱う。
サイズ表は各streetの攻撃回数を3回までにする`when aggressions >= 3 { remove raise }`を
他のruleより後へ書く。構造上の攻撃回数上限やsizeのthreshold併合は持たない。

Results画面はEquity・node比較・hand分類を持たない（対応するexport viewが無い）。未保存のRiver節点は
`stored: false`を示し、戦略は`solvers inspect --sol`のlazy re-solveで見る。TypeScript型は手書きであり、
protocol crateからの型生成とdesktop shellは持たない。

画面はCryst Design Systemで組む。tokens.css・bundle（`window.Cryst`のReact component）・font fileは
`web/vendor/cryst/`に正本から複製して置き、`web/src/cryst/`が型付きmoduleとして公開する。
`web/src/styles.css`は画面枠とCrystに無い表示（range grid、action履歴、収束図）だけを持ち、
色・寸法はCryst tokenだけを使う。iconはTabler webfont（`Icon`）だけを使う。Crystに無い次の表示は
この製品の例外として`styles.css`に置き、ほかの画面の前例にしない。

|例外|理由|
|---|---|
|range grid・action履歴card・Setupのlineのreplayと次のaction（streetごとのaction chip。replayのchipを押すとその前へ戻し、手番が来る席は破線枠の`primary`文字のchipで、押すとCrystのPopoverにその席のactionを出す）・収束図・script editor・TOML editor（CodeMirror 6。下記）|solverの表示であり、Crystに対応する部品が無い。色は`primary`（table見出し・script keyword・次のaction）・`success`（key）・`info`（文字列）・`warning`（数値）・`muted-foreground`（comment、lineに書かずに済むfold）|
|4色deck（♠`foreground`・♥`destructive`・♦`info`・♣`success`）。lineが配らないboardのcardは破線枠・打ち消し線・不透明度0.4|suitを色で区別し、cardの読み違いを減らす。無視するcardは消さずに使わないことを示す|
|Resultsのactionの色（fold `#3d7cb8`、check・call `#5ab966`、bet・raiseはmenuの小さい順に`#f03c3c`・`#ca3232`・`#a32929`・`#7d1f1f`、5つ以上はその間を等分）。gridのセルとmix barはactionごとの割合を幅の百分率で区切り線なしに並べる|GTO Wizardの既定の配色に揃え、他のsolverと見比べやすくする。色はthemeの意味（成功・警告）ではなくactionの種類を表すため、Cryst tokenではなく固定の値を`web/src/lib/actions.ts`に置く|
|Resultsのaction履歴cardの折返しと、1280px以下での詰めた表示（cardの最小幅を外し、action行を20px・`text-xs`にする）|節点の列が横に溢れて見えなくならないよう、横scrollではなく折り返す。CrystにResponsiveな履歴部品が無い|
|board選択dialogのdeck（13×4のcard button）|52枚から順序付きで選ぶ部品がCrystに無い。選択は`primary`枠と`selection`塗り|
|Setupの右列の卓の図（楕円のfeltと席、street wager、button、pot・effective・SPR・board、席名の横のOOP / IPの印）とtemplate選択dialogの行|卓の状態を席の配置で示し、席の押下でactive playerを選ぶ部品、例を中身付きで並べる部品がCrystに無い。選んだ席は`primary`枠と`selection`の外光。OOP / IPの印は`selection`塗りに`primary`文字。席の値はすべて`solvers line`の`seats`から受け取る。template・賞金のDialogは560px幅|
|Setupの右列のTree（節点のpathをaction chipのbuttonで並べ、子のactionをchip、chance nodeの子をboard選択と同じcard buttonで並べる）|事前のpublic treeを1節点ずつ辿る部品がCrystに無い。chipとcard buttonは上のlineのreplay・board選択の表示を使い回す|
|Guideの目次（左の縦の項目button）とscreenshot（`border`枠・`radius-md`の`img`）|頁内の節へscrollする目次と画像をCrystが持たない。hash routeを壊さないよう目次は`scrollIntoView`のbuttonで、色は`muted-foreground`、hoverは`accent`塗り|
|SetupのTreeのサイズ表（streetの行に、OOP・IPそれぞれのbet・raiseの入力欄を並べるgrid）|CrystのTableはrecordの行を表示する部品で、2段の列見出しの下に入力欄を揃える部品が無い。入力欄・checkbox・tabはCrystの部品で、開始streetより前の行の見出しは`muted-foreground`|
|`Notice`（Alertを状態色のBadge tint 15%で塗る）|AlertはCardと同じ`card`塗りで、Card上では背景と区別できない。titleを必須にし、iconと文字の行を揃える|

script editorとTOML editorは`web/src/components/CodeEditor.tsx`がCodeMirror 6（MIT）の部品を直に組み立てる。
monacoはworkerと数十MBの配布物が要り、React wrapperが既定でCDNから読むため使わない。言語は`web/src/lib/editor/`の
StreamLanguageの字句規則で、TOMLは`tree.script`の複数行文字列の中だけscriptとして色分けする。
構文の正本はdaemonのparserのままで、editorは構文を解釈しない。補完・hover文書の語彙
（`vocabulary.ts`）はRustの`crates/spot/src/dialect.rs`の変数と`crates/nlh/src/script/cond.rs`の予約語の写しで、
`vocabulary.test.ts`が両者の一致を検査する。errorの印はdaemonの検証結果を行へ置く。scriptは`line N:`の行、
TOMLはissueのkey pathを持つheader・keyの行（`tree.script`は文字列内の行）、TOMLの構文errorはsmol-tomlの行・列である。
配色は`lib/editor/theme.ts`がCryst tokenで定め、fold・検索panelのiconはTabler webfontを使う。

## 9. 研究経路と test 基盤の扱い

P2の本番solverとtoy testはcurrent-street recall/dense arenaを使う。
full recallの構築・復元は明示errorで拒否する。旧identityのcodec読込みと本番受入を区別する。
P1は独立の凍結`cfr-ref` oracleと差分試験を持つ。unit testは計算の所有crateに置き、
CLI integration testは公開入力・run lifecycle・表示を、daemon testはHTTP/process境界を検証する。
regression pinとtest層は[architecture.md §7](architecture.md#7-正当性検証)を参照する。

## 10. 変更時の参照先

公開契約は共通Input・製品規範・CLI referenceが正本である。
必須checkとexpensive ignored testは[development.md](development.md)、品質基準は
[products.jp.md](products.jp.md)、移行の受入条件は[再構築計画](plans/two-product-restructure.jp.md)に置く。
本書に作業状態や仕様の別表を作らない。
