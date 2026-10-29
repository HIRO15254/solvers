# Solvers 利用ガイド

`solvers`は共通Input `solvers.nlh/v1`からP1（NLH HU Postflop）またはP2（NLH Multiway Preflop）を解く。
P1は2人が残ったFlop/Turn/Riverのstreet開始、P2は2〜9人のPreflop rootを扱う。
入力は[共通Input規範](nlh-input-v1.jp.md)、全flagは[CLI reference](cli-reference.jp.md)を参照する。
P2の方式・品質は暫定であり、3人以上の結果にNash/GTO保証は無い。

現行versionは`0.1.0`。`solvers --version`（`-V`）は`solvers 0.1.0 (<git-hash>)`、
P1専用buildでは`solvers 0.1.0 (<git-hash>), P1 only`と表示する。
`solversd --version`は`solversd 0.1.0 (<git-hash>)`で、Git情報が無いbuildのhashは`unknown`である。
0.xでは入力schema `solvers.nlh/v1`を互換性単位とし、`.sol`・checkpointはminor releaseで変更し得る。
旧形式を拒否された場合は現行configから再solveする。詳細は[互換方針と変更履歴](../CHANGELOG.md)を参照する。

## 1. インストールする

### 配布物を使う

開発環境は不要。[GitHub Releases](https://github.com/HIRO15254/solvers/releases)から
対象versionのarchiveと`SHA256SUMS`をdownloadする。配布binaryは**P1（HU Postflop）専用**であり、
P2とderiveは含まない。[P1専用buildのコマンド表](cli-reference.jp.md#build-featureと配布範囲)を参照する。

archive名は`solvers-<version>-<target>[-compat]`である。

| OS / CPU | target | 拡張子 / CPU基準 |
|---|---|---|
| Windows x86-64 | `x86_64-pc-windows-msvc` | `.zip`、通常v3 / `-compat`はv2 |
| Linux x86-64 | `x86_64-unknown-linux-gnu` | `.tar.gz`、通常v3 / `-compat`はv2 |
| macOS Apple Silicon | `aarch64-apple-darwin` | `.tar.gz`、apple-m1 |
| macOS Intel | `x86_64-apple-darwin` | `.tar.gz`、v2のみ |

Windows/Linuxの通常版は**x86-64-v3（AVX2必須）**。AVX2が無いCPUには拡張子直前に`-compat`が
付くarchiveを選ぶ。compatも**x86-64-v2**が必要で、すべての64-bit CPUで動くわけではない。
WindowsはMicrosoftの[Coreinfo](https://learn.microsoft.com/en-us/sysinternals/downloads/coreinfo)の
`coreinfo -f`で`AVX2`が`*`か、Linuxは`lscpu`のflagsに`avx2`があるかを確認する。
Intel macOSでは`sysctl -n machdep.cpu.leaf7_features`の`AVX2`で確認できるが、配布はv2の1種である。

展開前に次のコマンドでhashを計算し、`SHA256SUMS`の**同じfilename**のSHA-256と一致することを確認する。
不一致なら実行しない（checksum自体はcode署名ではない）。

```powershell
# Windows PowerShell（filenameは実際のdownloadに置換）
Get-FileHash .\solvers-0.1.0-x86_64-pc-windows-msvc.zip -Algorithm SHA256
```

```sh
# Linux
sha256sum solvers-0.1.0-x86_64-unknown-linux-gnu.tar.gz
# macOS
shasum -a 256 solvers-0.1.0-aarch64-apple-darwin.tar.gz
```

WindowsはExplorerの「すべて展開」、Linux/macOSは`tar -xzf <archive>`で全体を展開する。
binaryと`ui/`、launcherの相対配置を保つ。Windowsは`start-gui.cmd`、
Linux/macOSは展開先で`sh start-gui.sh`を実行する。browserが開き、Setup → validate → solve →
Runs → Resultsで最初のP1を計算・閲覧できる。利用中はlauncherのterminalを閉じない。
詳細は[Web GUI](#web-gui)を参照する。

CLIだけなら展開先で以下を実行する。PowerShellでは`./solvers`を`.\solvers.exe`に置換する。

```sh
./solvers config new --product p1 --template minimal --out spot.toml
./solvers validate spot.toml
./solvers solve spot.toml --out runs/first
./solvers export runs/first/solution.sol summary
```

PATHへの追加は任意。追加しなくても展開先の`./solvers`（Windowsは`.\solvers.exe`）で使える。
常用する場合は展開先directoryをユーザーのPATHへ追加し、新しいterminalで`solvers --version`を確認する。
以下の章ではPATHに追加した表記を使う。

### 動作要件

| 項目 | 要件・目安 |
|---|---|
| Windows | 64-bit Windows 10以降 |
| macOS | Apple Silicon: macOS 11以降、Intel: macOS 10.12以降（Rust targetの下限） |
| Linux | x86-64、glibc 2.35以降（Ubuntu 22.04 build）。musl版・Linux ARM版は配布しない |
| CPU | 上表の基準。GPUは不要 |
| memory | 木・range・storageに依存。solve前に見積りを表示する。`solvers validate spot.toml --resources`でも確認可能 |
| disk | archiveと展開先に加え、runごとの`solution.sol`・checkpoint等の空き容量が必要。木・保存street・圧縮に依存し、一律の最小値は無い |
| GUI | 現行のWeb browser。Rust・Cargo・Node.jsは配布物の実行には不要 |

OS下限の根拠は[Rust platform support](https://doc.rust-lang.org/rustc/platform-support.html)。
Linux配布物はRust自体のglibc 2.17下限ではなく、release workflowのUbuntu 22.04（glibc 2.35）を基準とする。
これはtarget/build環境からの要件であり、各最古OSでの実機動作確認を意味しない。

[CPU基準の計測](../experiments/release-2026-10/cpu-baseline-20261015/README.md)では
`flop_4bp`が469,808 node、`flop_3bp`が2,222,550 nodeである。
同梱例[flop_3bp](../examples/hu-postflop/flop_3bp.toml)のstorage見積りは
f32で約1.44 GiB、i16で約0.73 GiB。これらは**storageだけの値でRSSではない**。
solve前に表示するmemory見積りは、これに木・rank table・thread scratch・保存作業領域・checkpoint圧縮等を加えた量で、thread数とともに増える。
大きい木では数GiB以上になるため、小さいP1 templateで始め、[memory設定](#4-solveする)と見積りを確認する。
配布archiveの確定サイズ・runのdisk使用量はReleaseと実際の入力で確認する。

### 未署名binaryを起動する

v0.1.0はcode署名しない。信頼できるReleaseから取得し、上記checksumを確認したものだけに以下を行う。

- Windows SmartScreenの「WindowsによってPCが保護されました」は **詳細情報（More info）→ 実行（Run anyway）**。
  または展開先のPowerShellで`Get-ChildItem -Recurse | Unblock-File`を実行する。
- macOS Gatekeeperで拒否された場合は`xattr -dr com.apple.quarantine <展開先directory>`でquarantine属性を除く。
  または起動を試した後、**システム設定 → プライバシーとセキュリティ → このまま開く（Open Anyway）**を使う。
  OS全体のGatekeeperを無効化しない。

daemonは既定のloopbackで使う。非loopback bindには**TLSが必須**であり、bearer tokenを他人に渡さない。
tokenを含むGUI URL・ログ・browser保存値も秘密情報として扱う。
公開設定・tokenの指定方法は[daemonの章](#7-daemonから実行する)を参照する。

### ソースからbuildする

Rust stable、Cargo、Gitを用意し、repositoryを取得する。OS別の準備は[development.md](development.md)を参照する。

```sh
cargo build --release -p cli -p daemon --no-default-features
```

binaryは`target/release/solvers`と`solversd`（Windowsでは`.exe`）である。
これはP1専用配布buildで、P2とderiveは利用できない。拒否診断は`p2` feature付きbuildを案内する。
P2も使う開発用buildは`--no-default-features`を外す（`cli`の`p2` featureは既定で有効）。
以下のP2・deriveの手順は`p2` feature付きbuildを前提とする。
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

`config new`の既定は`p2`である。P1専用配布buildでは既定が`p1`になり、`--product p2`は`NLH005`（exit 2）になる。

先頭のschemaは常に`solvers.nlh/v1`である。table、economics、spot、ranges、treeを編集する。
製品はtable・line・boardから決まる。P1のpot・stack・OOP/IPはlineを再生して導出する。
金額はBB、内部gridは0.001 BBである。P1のrange指定やline文法は規範の例に従う。
P1ではPreflop ruleは効果を持たず、指定すると警告が出る。
`checkdown`は一致した手番をcheck、賭けに直面していればfoldにする。
外部`.tree`を使う場合はconfigからの相対pathと`tree.params`を記し、
[river_script.toml](../examples/hu-postflop/river_script.toml)を参照する。
旧configの自動変換は無いため、現行例から書き直す。
`tree.allin_threshold`・`[tree.max_aggressive_actions]`・`tree.preflop_reraise_jam_above_stack`は削除済みで、指定は未知keyの`NLH002`となる。
これらを埋込みconfigに含むartifact・checkpointも読み込めない。
正規化・実効configにも出力しない。旧Preflop設定は他のPreflop ruleの後へ
`preflop when aggressions >= 1 { jam raise > 33.333%stack }`を置いて移行する。
旧keyとはanteのある場合の分母、0.001 BB gridでの閾値丸め、ソース位置での適用が異なる
（[共通Input第9節](nlh-input-v1.jp.md#size-literalと解決順序)）。

### P1の木の大きさを決める

P1のmemoryと時間は、行動の候補数の積でほぼ決まる。0.1% pot程度の解を少ない資源で得るには、
GTO Wizard（GTOW）の公開情報と[雛形の計測](../experiments/p1-efficiency-2026-10/templates-20261009/README.md)から、次の順で木を絞る。

- flopのbet sizeの数は木を最も大きくするが、EVへの影響は小さい。GTOWの比較では、flopのsize 1本と3本の差は
  多くの盤面で0.01 bb以内だった。SRPのflopはまず1〜2 sizeにする。
- flop戦略への影響はturnのsizeの方がriverより大きい。riverには小さいbetとall-inのように離れた2 sizeを置く。
- donkは`when donk { remove bet }`で消す。raiseは`when aggressions >= 2 { replace raise [a] }`で2回目以降をall-inにする。
  大きい候補だけをall-inにするには、sizeを足すruleの後へ`turn, river { jam bet >= 85%stack jam raise >= 85%stack }`を置く。
  `jam`は現在menuの到達額を閾値と比べ、該当候補だけをMへ置き換える。MはW＋残stack（P1ではeffective-stackの木の到達額）である。
  閾値は到達額への変換だけで求め、最小raiseへ引き上げずMでcapしない。候補自体は追加せず、後続ruleが追加した候補には作用しない。
  低SPRでは`turn, river when spr <= 1.5 { replace raise [a] }`でraiseをall-inにできる。
- 近いsizeをまとめるには、sizeを足すruleの後へ`flop { merge bet within 10 }`を置く。大きい候補から順に見て、
  直前に残した候補とのpot 10%以内の差の候補を削除する。幅は`0.5bb`や`5%stack`でも書け、all-inに近い候補はall-inへまとまる。
- 攻撃を3回までに制限するにはscriptの最後へ`flop, turn, river when aggressions >= 3 { remove raise }`を置く。
  ruleはソース順に適用するため、最後のcapは先行ruleに取り消されない。cap ruleが無ければraise chainはstackとNLHの合法性だけで制限され、木が大きくなりうる。
- 3BP・4BPはSPRが低く木が小さいので、flopに3 sizeを置いても軽い。

同梱の[flop_srp.toml](../examples/hu-postflop/flop_srp.toml)・[flop_3bp.toml](../examples/hu-postflop/flop_3bp.toml)・
[flop_4bp.toml](../examples/hu-postflop/flop_4bp.toml)はこの方針の雛形である。8C/16T・12 threadのPCで0.1% potまでの目安は次の通り。

| 例 | node数 | f32 storage | 0.1% potまでの反復 | 時間 |
|---|---:|---:|---:|---:|
| flop_srp | 1.16M | 3.7 GiB | 389 | 3.5分 |
| flop_3bp | 2.22M | 1.4 GiB | 260 | 1分 |
| flop_4bp | 0.47M | 0.1 GiB | 99 | 2.5秒 |

flop_srpのflopに75%を足すとstorageは約1.8倍、時間は約1.7倍になり、OOPのEVは0.015 BB（0.27% pot）しか変わらなかった。
全streetに33・75%と3x raiseを置く旧来の木は28 GiBを超える。

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

lineを段階的に組み立てるときは、卓だけが有効な未完成configでも次の合法手を確認できる。

```sh
solvers line spot.toml --moves "" --format json
solvers line spot.toml --moves "r3,c" --format human
solvers line spot.toml --format json
```

`--moves`は明示prefixをcomma区切りで置換する。省略すると`spot.line`を厳密にreplayする。
cursorの`next.options`から次のtokenを選び、bet / raiseはstreet投入総額を付ける。
all-in callは`c`、all-in bet / raiseは`a`である。`complete`はpostflop開始状態を表し、
config全体の検証成功ではないため、solve前には通常のvalidateを実行する。
JSONの全fieldと単位は[CLI reference](cli-reference.jp.md#solvers-line)を参照する。

P1のsolve前にはconfigからpublic treeを構築し、nodeの行動候補と子nodeを確認できる。
`--rest-of-street`を付けると、選んだnodeからそのstreetの終わりまでのaction nodeをまとめて返す。

```sh
solvers tree spot.toml --format json
solvers tree spot.toml --node check --format human
solvers tree spot.toml --rest-of-street --format json
```

既定はrootだけを返す。JSONの`children`にある履歴をそのまま`--node`へ渡して辿れる。
行の意味は`export SOLUTION tree`と同じだが、まだ保存状態が無いので`stored`は含まない。
solveやrun directoryの作成は行わず、P2 configと未知nodeは明示的に拒否する。

## 4. solveする

```sh
solvers solve spot.toml --out runs/my-spot --threads 4 --memory 2GiB --max-time 5m
```

毎回新しいrun directoryを指定する。overrideはrun.tomlへ保存する。
P1の`[solver.stop] check_every`は`"auto"`が既定（PF10、2026-10-08）。targetがあると初回25 iteration、以降は評価値から到達を予測した3〜50 iterationの適応間隔で測り、`NashConv / 2 <= target`で止まる。target無しは固定25、整数の明示は従来の固定間隔である。target有りでは停止iterationが旧版と変わり得るため、旧版と同じ停止を得るには`check_every = 25`を明示する。実効configはautoか整数を必ず保存する。
P1の`[solver] storage`は`auto`（既定）、`f32`（両arena f32）、`i16`（両arena i16、memory最小）、`i16-f32avg`（regret i16、戦略累積f32）を選べる。0.1% pot程度を目指し旧i16が頭打ちになる場合は`i16-f32avg`を使う。regretの量子化誤差は残る。storage bytesは順に8L、4L＋8N、6L＋4N（L=要素数、N=action node数）。

autoはmemory上限（`[run] memory` / `--memory`、未指定なら物理RAMの80%）にf32の必要量が収まればf32、次にi16-f32avgを選ぶ。i16は選ばず、両方超過なら両必要bytesと上限を示しexit 75となる。`validate --resources`は選択結果を表示し、両方超過ならi16-f32avgの必要量と`withinLimit: false`を返す。正規化はautoを維持し、solve表示とrun.toml・checkpoint・`.sol`のconfig / metadataは具体storageを保存する。既存runの明示storageは維持され、resume・照会は現在のmachine memoryで再選択しない。

P1のmemory見積りは`max(storage + checkpoint encoder, storage − regret + 保存作業領域 + 圧縮予算) + 常駐量`。regret bytesはf32で4L、i16・i16-f32avgで2L＋4N。checkpoint encoderはrun threads数に比例するzstdの作業領域、常駐量はprocess・compiled木・rank table・profile slot・thread数分のengine作業領域で、`validate --format json --resources`の`checkpointWorkspaceBytes`・`residentBytes`に出る。solve/resumeは最後のcheckpointを新たに書き、並行peak `S + W + C + E + M`（S=選択storage、W=保存作業領域、C=圧縮予算、E=checkpoint encoder、M=常駐量）が上限以下ならregretを保持してcheckpointと`.sol`を並行生成する。その他は最後のcheckpoint（指定時）→ regret配列とscaleの解放 → `.sol`生成の順とする。全停止理由に適用し、solve開始の見積り式は変わらない。反復中のcheckpointでは解放しない。保存作業領域は保存用packed値block・sref slot・保存対象/street配列、上限付き1 batch分の並列戦略作業領域を含む。戦略blockは合計8,388,608要素以下のsref連続区間ごとにrun threadsで並列生成し、sref順に出力する。上限を超えるnodeは単独batchとし、同時に保持するbatchは1個。全node分は保持しない。NoRiversもfullの保守的な見積りを使う。常駐量とcheckpoint encoderは測定から較正した値で、RSSの上限保証ではない。threadsを減らすと見積りも減る。
P1のmemory autoは物理RAMの80%、P2はarena予算6 GiBである。P2のmemoryはRSS全体の上限ではなく、
tree・cache・thread scratch・評価・checkpoint用の追加memoryが必要である。
P2のEHS² tableは初回に構築し、以後はmachine cacheを利用する。
cache場所は`--cache-dir`または`SOLVERS_CACHE_DIR`で指定する。

P1はNashConv / 2の目標、P2は測定deviationの目標を検査する。
反復・sweep・時間の上限で終わったことは品質目標の達成を意味しない。
保存物はrun.toml、manifest.json、events.jsonl、progress.jsonl、run.jsonと、
P1のcheckpoint.ckpt / solution.solまたはP2のcheckpoint.mwckpt / solution.mwsolである。
`run.lock`は排他OS lock用の共通fileであり、artifactではない。process終了・crashでlockは自動解放され、fileが残っていても妨げにならない。
P1の`[solver] cfr_precision`は`"f32"`（既定）または`"f64"`（旧版とbit一致）。CFR終端とcurrent strategyだけに作用し、評価・平均戦略・保存EVはf64を使う。

P1の保存streetは`[output] solution_streets`で設定する。
no-riversはRiverの戦略と値を省く。省いたRiverはviewer・`export --resolve-rivers`・Web GUIのResultsが開いた時にその場で再solveする。

### P1のnodelockと相手profile

`solvers tree spot.toml --node root`で実際のaction labelを確認し、configに`[profile]`を追加する。
例えばboardが`2c 7d 9h Js Qs`、BB rangeに`AsAh`を含み、rootにcheckがある場合:

```toml
[[profile.lock]]
node = "root"
combos = { AsAh = { check = 1.0 } }
```

comboごとの固定のほか、`combos`の代わりに`hands = "AA,KK"`または
`category = "toppair & !flushdraw"`と`strategy = { check = 0.5 }`を使える。
部分lockでは残りの確率を未指定actionで最適化する。`confidence = 0.5`のsoft lockは
固定戦略50%と自由戦略50%を混ぜるため、全actionの確率を指定し合計1にする。
categoryの名称と式は[P1規範](hu-postflop.jp.md#hand-category)を参照する。

相手の判断を偏らせるには別の`[[profile.incentive]]`で`node`、`action`、任意の
`hands`または`category`と、pot比の`pot = 0.03`（またはBB単位の`bb = 0.5`）を指定する。
加点は上流へ伝播し、incentiveのある計算は一般和となる。集計頻度を目標にするには
`[[profile.frequency]]`に`node`、`action`、任意のhand指定と`target = 0.5`を書く。
頻度は到達weightで加重しrunoutごとに測る。solveの表示と`run.json`で達成値・差・λを確認する。
combo単位の固定と集計頻度目標は異なり、同じnodeの同じhandに重複するとerrorになる。

上流を基準解に固定する場合は`[profile]`に
`freeze_upstream = { solution = "runs/base/solution.sol" }`を指定する。基準解は同じ木と
hand supportを持つ必要があり、実効configにpathとSHA-256が保存される。
基準fileのhashはvalidate・solve時に照合し、大文字・小文字を区別しない。
freeze由来の`.sol`を読み込む、exportする、比較する際は基準fileを再読込・再hashしない。
solveはprofile対象の最大部分木を自動的に選び、外側には基準解の平均戦略をコピーする。
外側の戦略が基準解に無い場合、頻度目標がある場合、対象が空またはrootを含む場合は木全体を反復する。
上流固定に必要な戦略が無い場合はerrorとなる。停止判定・進捗・結果は木全体で評価し、
storageや保存成果物も木全体を扱うため、部分木化によるmemory削減は期待しない。
`run.json`の`profile`で`solve_scope`（`subtree` / `whole-tree`）、`subtree_roots`、
任意の`fallback_reason`を確認できる。理由は`frequency-targets`、`no-targets`、`root-target`、
`missing-base-strategy`、`outside-residual`である。後者は対象外の残差だけで停止目標以上となる場合に全木を再solveする。
再開時は計算modeと部分木identityを照合する。新modeを選んだ場合、旧whole-tree modeのcheckpointは再開できないので現行configから再solveする。

既存解の戦略を出発点にするには、lockをexportして編集し、`relock`で追加して再solveする:

```sh
solvers export runs/base/solution.sol lock --node root --output lock.toml
# lock.tomlの確率や対象を編集する
solvers relock runs/base --add lock.toml --print-config
solvers relock runs/base --add lock.toml --out runs/relocked
```

exportは選択nodeに到達するcomboだけを出す。SOURCEにはrun directoryのほか、
埋込みconfigを持つP1 `.sol`も指定できる。`--add`は繰り返せ、`--add -`はstdinから
`[[profile.lock]]`、`[[profile.incentive]]`、`[[profile.frequency]]`のTOML断片を読む。
既存profileへの追記が既存lock等と重複する場合は検証errorになる。同じ種類・同じ`node`文字列の
既存entryを置き換えるには`--replace`、元のprofile全体を消してから追加するには
`--clear-profile`を使う。後者は既存の上流固定も消す。

1件だけなら断片を作らずに指定できる（action labelは実際の木に合わせる）:

```sh
solvers relock runs/base --node root --hands "AA,KK" \
  --strategy "check=0.25,bet 1=0.75" --out runs/quick-lock
```

`--hands`の代わりに`--category EXPR`を使え、どちらも省略すると全handが対象。
soft lockには`--confidence P`を指定する。既定では上流も自由に木全体を再solveする。
上流を元のsolutionに固定するには`--freeze-upstream`を追加する。
`--print-config`はsolveせず実効configを表示し、`--out`とは同時に使えない。
入力の詳細・wildcard・制約は[共通Input規範](nlh-input-v1.jp.md)、
全flagは[CLI reference](cli-reference.jp.md#solvers-relock)を参照する。profileはP1専用である。

## 5. 監視・停止・再開する

P1の`[run] final_checkpoint = false`は終了時の再開state保存を省く設定で、既定はtrue（PF9、2026-10-08）。目標到達・iteration上限・時間上限・cancelの全てに適用し、定期checkpointと`.sol`は従来どおり保存する。保存した`run.toml`を編集してresumeにも適用できる。P2では指定不可（`NLH002`）。省いたrunは最終状態から再開できず、最後の定期checkpointから続ける。定期checkpointも無ければ`run.toml`から再solveする。再開時のprogressはcrash再開と同じ追記方式で、checkpointより後の既存行も残るためiterationが重複し得る。
autoの評価は時間・thread数に依存せず、progressは評価ごとに追記するためtarget有りでは不等間隔になる。中断・max_time・定期checkpointの判定はautoで25 iteration以下のsub-batchごと、整数で指定間隔ごとに行う。
autoの再開はprogress.jsonlのcheckpoint iteration以下の行を使い、同じiterationの重複は最後の行を採用して次の評価を再計算する。同じconfigで一度に解いたrunと評価iteration・停止iteration・stateが一致する。progressが無い・読めない場合は履歴無しとしてcheckpoint iteration＋25（max_iterationsで切る）から評価するため、評価iteration一致を保証しない。裸のcheckpointだけを移した場合も同様である。checkpoint形式は変わらず、旧run.tomlの`check_every = 25`は固定25で再開する。

P1のDCFRは`[solver.algorithm]`の`alpha = 1.25`、`beta = 0.5`、`gamma = 4`が既定である。`pow4_reset`未指定時は`[solver] storage = "i16"`ならtrue、`"auto"`・`"f32"`・`"i16-f32avg"`ならfalseとなる（利用者決定PF8、2026-10-07）。明示したtrue・falseはどのstorageでも優先する。full templateはauto用の`pow4_reset = false`を明示しているため、storageをi16に変えて既定resetを使う場合はこの行を削除するかtrueにする。
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
実行中の同じdirectoryへのresumeはexit 1で拒否され、directoryとmanifestのpidが表示される。止まるまで待つか、`--out`で別directoryへforkする。forkは実行中sourceを書き換えない。
events/progressは改行の無い不完全な末尾だけをreaderが無視し、再開writerがその末尾だけを切り捨てて追記する。statusのeventsOffsetも最後の完全recordまでである。P1の評価履歴では改行済みの不正JSONがあると履歴無しへ戻る。
P1ではrun設定とmeta以外を変更できない。P2では必要に応じて
`--max-sweeps`・`--stop-target`・`--evaluation-samples`・`--evaluation-cadence`を上書きする。
裸の自己完結checkpointからの再開には`--out`で新directoryを指定できる。

旧runのstatus・watch・runs lsも利用でき、JSONのconfigSchema / gameKindは記録値のままである。
旧runのresumable表示はcheckpointの存在を示すだけで、現行CLIでは再開できない。
`.sol` version 1/2/3とcheckpoint version 1/2/3/4は読めない。現行configから再solveして`.sol` version 4・checkpoint version 5を作る。

## 6. 結果を読む

```sh
solvers export runs/my-spot/solution.sol summary
solvers export runs/my-spot/solution.sol ev --node all --format csv --output ev.csv
solvers export runs/my-spot/solution.sol strategy --node root --node x --format csv
solvers export runs/preflop/solution.mwsol strategy --format csv --output strategy.csv
```

summary、tree、actions、strategy、range、evをJSON/CSVで読む。P1の`--node`は繰り返せ、1回の読込みで複数nodeの行を指定順に出す。
P1のtreeはaction・chance・terminalを返し、`children`のhistoryを次の`--node`にそのまま使える。
chanceの`actions`は配るcard（`*`はiso併合、cardなしは`dealN`）、terminalは空配列である。
action以外の`actor`・`pot`はnull。不明なselectorやaction以外へのstrategy等の指定は入力error（exit 2）。
P1の`.sol`はsolveした木を保存するため、読込みで木を再構築しない。`summary`はmetadataだけを読む。
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
no-riversの未保存Riverをviewerで開くと再solveする。`export`も`--resolve-rivers`を付けると未保存Riverの`actions`・`strategy`・`ev`を同じ再solveで出す
（反復予算は`--river-iterations`、既定500）。3つとも要るなら`export SOL decision --node NODE --resolve-rivers`の1回でJSONにまとめて受け取ると再solveも1回で済む。
再計算値を保存時の値と同一視しない。
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
P1のsolution同士もcompareできるが、tree形状と保存node集合の一致が必要である。
通常はboard・pot・effective stack、seat別comboリスト、各nodeのaction label（size込み）も一致させる。
rangeの重みやalgorithmだけの違いは許す。意図的に異なるspotを形状だけで対応付ける場合は`--cross-game`を付ける。
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

表示されたbearer tokenを全`/v1` API requestのAuthorizationへ付ける。
POST /v1/validateまたはPOST /v1/runsへJSONのconfigTomlを送る。
上流固定には任意の`freezeBaseRunId`を同じJSONに指定できる。daemon管理下の停止済みP1 runの
`solution.sol`をdaemonが選び、pathとSHA-256を実効configへ入れる。clientからpathは送れない。
投入したrunの`relockedFrom`に基準run IDが残る。
line-builderにはPOST /v1/lineへconfigTomlと任意のmoves配列を送り、CLIと同じcursor JSONを受け取る。
P1のsolve前の木はPOST /v1/treeへconfigTomlと任意のnodes配列を送り、CLIと同じtree行のJSON配列を受け取る。
nodes省略・空配列ならrootだけで、childrenの履歴を次のnodesへ渡せる。storedは含まない。restOfStreet: trueで各nodeのstreetの残りも返す。
外部source参照は拒否されるため、先にvalidate --write-effectiveでinline化する。
daemonは同じCLIを起動し、run directoryに状態を保存する。再起動後もrun rootから一覧を復元する。
run一覧・詳細は`lastProgress`に最後のraw進捗行を含み、P1では`nash_conv / 2`を一覧のExploitabilityに使える。進捗行がまだ無ければこのfieldは省略される。
停止済みP1 solutionからは`POST /v1/runs/{id}/relock`へ
`{"profileToml":"...", "replace":true, "clearProfile":false, "freezeUpstream":false}`を送り、
新しいrunを投入できる。`profileToml`は`[[profile.lock]]`等のTOML fragmentである。
`freezeUpstream`をtrueにすると元runのsolutionより上流を固定する。
返る`runId`で新runを追跡でき、一覧・詳細の`relockedFrom`から元runへ戻れる。
非loopbackで公開するときはTLS証明書・秘密鍵を指定する。
HTTPのsolution viewはP1・P2に対応する。P1では、例えば
`GET /v1/runs/{id}/solution/tree?node=root`や
`GET /v1/runs/{id}/solution/strategy?node=root&format=csv`で照会できる。
`node`は繰り返せ、履歴中の`[`・`]`・`/`・space・`*`はURL encodingする（`+`もspaceとして復号）。
P2では`node`を指定できない。不正selectorはHTTP 400 `invalid-request`としてCLIの診断を返す。
P1の`resolve=rivers`はno-riversで未保存のRiver節点を`export --resolve-rivers`で再solveして返す（requestごとにRiverを解く）。
同じnodeの`actions`・`strategy`・`ev`は`GET /v1/runs/{id}/solution/decision?node=...`の1 requestにまとめられる（P1のみ、JSONのみ）。
`restOfStreet=true`を付けると各nodeの後にそのstreetの残りのaction nodeも返す。Riverの入口に付ければ1回の再solveでそのRiverの全action nodeが得られる。
cancelは1回目のCtrl-Cと同じく境界で止まりcheckpointを保存する。Windowsではdaemonを
consoleのある端末から起動する（console無しではcancelが`internal` errorになる）。

### Web GUI

配布archiveを展開した場合は、Windowsでは`start-gui.cmd`、Linux・macOSでは`sh start-gui.sh`で起動する。
launcherは自身のdirectoryへ移動し、`solversd --open --runs runs`を実行する。
同梱の`ui/index.html`は`solversd`の隣から自動検出され、既定browserでGUIが開く。
runは展開先の`runs/`へ保存するため、書込み可能な場所へ展開する。

sourceから使う場合は、Node.js 22以上で`web/`をbuildしてから同じdaemonで配信する。

```sh
cd web && npm ci && npm run build && cd ..
solversd --runs runs --ui web/dist --open
```

`--open`はlisten後、起動時の`gui:` URL（tokenはfragmentに入る）を既定browserで開く。
browserの起動に失敗してもdaemonは動き続け、stderrに警告を出すので、表示されたURLを手動で開く。
`--open`を省略した場合もそのURLを手動で開ける。tokenはそのoriginに保存され、
無効なら入力欄が出る。静的assetは認証不要だが`/v1` APIは引き続きbearer tokenが必要である。
`/`と拡張子なしの未存在pathは`index.html`へ、未存在のassetは404となる。
明示`--ui DIR`は自動検出より優先する。`--ui`省略時は実行binaryの隣の`ui/index.html`を探し、
無ければAPI専用となり全requestの認証が必要である（作業directoryの`ui/`は探さない）。
UI無しで`--open`を指定すると起動errorになる。endpointとflagは[CLI reference](cli-reference.jp.md#solversd)を参照する。

GUIの表示は英語である。sidebarのGuideは、各画面のscreenshot付きの操作手順を示す。

- Setup: P1入力をTable・Start・Range・Tree・Solverの5つのstepで編集する。step tabで行き来し、各stepの下の
  「Next」で順に進む。TOMLの読込み・保存（Open TOML・Save TOML）もできる。step tabの右の「Load template」は
  `examples/hu-postflop/`の例をdialogで選び、入力を置き換える（例の`meta.name`は外す）。stepとは別に、区切り線の右端の
  「Edit TOML」で文書全体のTOMLを開き、もう一度押すと開く前のstepへ戻る。SolveはheaderとSolver stepの下にあり、
  検証が通りTOMLが読めるときだけrunを投入する。cardは4色deck（♠文字色・♥赤・♦青・♣緑）で示す。
  - 右列: 卓の図・検証・Treeを常に示す（狭い画面ではstepの下）。卓の図の上の名前は`meta.name`で、spot名・run名・TOMLの
    保存名を兼ねる。空なら自動の名前（例: `6max BTN vs BB Ks7h2d`。spotに追従する）をplaceholderに示し、検証・Solve・
    保存する文書の`meta.name`に書く。run名とTOMLのfile名は、名前の英数字・`.`・`_`・`-`以外（空白など）を`_`に置き換えたもの
    （例: `6max_BTN_vs_BB_Ks7h2d`）で、既存のrunと重なれば`-2`などを付ける。
    卓の図は`[table]`と`spot.line`をdaemonの`solvers line`でreplayした席・stack・street wager・fold・button・pot・board、
    effective stack・SPR、手に残る2人のOOP / IPの印を描き、検証の結果は書かない。検証は編集のたびにdaemonの
    `validate --resources`を呼び、node数・terminal数・storage・memory見積り・warningを示し、
    実効config（既定値をすべて明示した正規化済みの文書）を保存できる（Save effective config）。errorが名指すkeyに入力欄があれば、その入力欄の直下に
    理由を示し（サイズ表は`tree.script`のerrorの行を書いたcell、Rangeは席のtabにも印、席ごとのstack・blind・anteは該当する欄、
    `meta.name`は名前の欄）、そのstepのtabに印を付けて「Fix in（step名）」でそのstepを開く。どのstepにも入力欄の無いkeyのerrorは
    「Edit TOML」に印を付けてTOMLへ導き、TOMLの上に理由を示す。daemonは一度に1件のerrorを返す。
    Treeは検証が通るとdaemonの`POST /v1/tree`（`solvers tree --rest-of-street`と同じ行）で、1 streetの全decisionを字下げで示す。
    各decisionは手番（席とOOP / IP）とpot、その下に各actionを並べる。actionはResultsと同じ色の印、額（BB）、
    betはpotに対する%、raiseはcall後のpotに対する%と直面額の倍率（scriptのsize literalと同じ読み方）、全stackならAll-inを示す。
    次のdecisionがあるactionは押すと開閉し、最初の2段は開いている。右上のbuttonで全て開く・閉じる。
    矢印key（上下で移動、左右で閉じる・開く、Enterで実行）でも操作できる。streetを閉じるaction（「Deal turn」「Deal river」）を
    押すとcard選択を出し、選んだcardで次のstreetを示す。all-inのrunoutは続けてcardを選び、Showdownで終わる。
    歩いたstreetは上にcardで並び、押すとそのstreetへ戻る。lineやscriptを変えると同じstreetを読み直し、無くなれば最初のstreetへ戻る。
  - Table: 人数と、stack・blind・anteを「Same for all」か「Per seat」で書く。Same for allは全員のstackとante
    （None・Everyone・BB ante）で、SBは0.5 BB固定である。Per seatはstack・blind（straddleを含む）・anteを全員分カンマ区切りで、
    Preflopの行動順（6maxならUTG, HJ, CO, BTN, SB, BB）に書く。blindは最後の2席がSBとBB（BBは1）、
    先頭の席から続く正の値がstraddleである。anteは全員同額か、BBだけ（BB ante）を書ける。
    席ごとに異なるstack・0.5以外のSB・straddleがある文書はPer seatで開き、Same for allへ戻すとそれらを消して共通のstackにする。
    経済条件はcash（rake率・cap・rakeの条件）かtournamentで、rakeはcashだけ、賞金・卓外fieldはtournamentだけに付く。
    種類を切り替えると他方のkeyを外し、page内で戻すと外したkeyを戻す。tournamentの賞金は専用dialogで編集し、
    preset（勝者総取り・65/35・50/30/20・40/30/20/10・9位まで・上位N人同額のサテライト）と賞金総額から埋めて行ごとに直せる。
  - Start: 卓の図の席を押すとactive playerの選択・解除になり、2人を選ぶとその2人が手に残るpreflopの経緯
    （2bet・3bet・4bet・5bet・squeeze vs open・squeeze vs call・limp・iso・limp-iso）が並ぶ。
    squeezeやisoの3人目は、条件を満たす最も近い席である。経緯を選ぶとすぐ`spot.line`へ書き、
    開始street・開始前のpostflopの進行（check-check・OOP check→IP bet→call・OOP bet→call）を変えても書き直す。
    presetの額は固定（open 2.5・3bet 8・4bet 20・5bet 45・squeeze 12・iso 4 BB、postflopのbetはFlop 33%・Turn 75% pot）で、
    他の額はlineを直接直す。lineを手で変えるとpresetの選択は外れる。preflopの行動順・postflopの先手・potはdaemonのreplayから得る。
    lineのカンマと` / `の前後の空白はformが文法の形（`, `と` / `）に揃えて文書へ書き、lineの下にreplayの行動を
    streetごとのbadgeで示す（lineに書かずに済むfoldは薄く示す）。今のstreetの行には、行動の後に続けて、そのstreetでまだ
    手番が来る席のpositionだけを破線のbadgeで並べる（daemonのreplayで前の席がcheck、できなければfoldしたときに手番が回る席）。
    badgeを押すとpopoverにその席の行動（fold・check・call 額・bet / raise 最小–最大・all-in 額）が出て、選ぶとその前の席を
    check / foldで補ってlineへ足す。bet・raiseはそのstreetでの合計額を書いて足し、範囲外の額は足さずに理由を示す。
    replayのbadgeを押すと、そのactionの前まで戻す。
    組み立て途中のlineはfoldを含む明示の手順でreplayし続け、lineを手で書き換えるかpresetを選ぶと書いたlineのreplayへ戻る。
    boardは文字で直すか「Choose」（flop・turn・riverの順に最大5枚）で選び、
    作ったlineにboardが足りなければboard選択が開く。lineが配る枚数より多いboardは、先頭から必要な枚数だけを使い、
    残りのcardは薄く破線で示して無視する（検証・Solve・TOMLの保存は必要な枚数のboardで行い、文書には残すので、
    lineを伸ばせばそのcardを使う）。lineを再生できなければ卓の図にPreflop rootを示し、理由は検証がline欄の下に示す。
    postflopのstreetの開始に届かなければ次の手番を示す。
  - Range: lineが手に残す2人のrangeをOOP・IPのtabで編集し、「Swap OOP and IP」で中身を交換する。
    lineの変更で手に残る2人が変わると`[ranges]`のkeyを付け替える。残る席はそのrangeのまま、新しい席はこの文書で
    その席が前に使ったrange、無ければ同じ役（OOP・IP）だった席のrange、それも無ければ外れた唯一のrangeを受け取る。
    移したrangeの中身は変えないため、その席のrangeに直す。付け替えた内容をRange stepに示し、TOMLを直接編集している間は付け替えない。
  - Tree: bet sizeを「Size table」か「Script」のtabで書く。サイズ表はstreetごとにOOP・IPのbetとraiseのsize
    （[size literal](nlh-input-v1.jp.md#size-literalと解決順序)。`33`はpotの33%、`a`はall-in。カンマか空白で区切り、
    空ならそのactionを作らない）を並べ、`tree.script`を`in_position`・`donk`・`aggressions`の条件だけのscriptへ書き直す。
    「No donk bets」は前のstreetで最後にbet・raiseした相手へOOPが先にbetしない木、
    「Re-raises are all-in only」はbet・raiseに直面したraiseをall-inに限る木にする。開始streetより前のstreetの行は薄く示す。
    サイズ表で書き戻せないscript（他の条件・`add`・`force`などを含む）はscriptのtabで開き、
    サイズ表のtabはその旨と「Replace with the default size table」（Flop 33/50・Turn 75/a・River 75,a/a、OOP・IP同じ）を示す。
    サイズ表は各streetの攻撃回数を3回までにする`when aggressions >= 3 { remove raise }`を他のruleの後へ書く。
    全nodeでall-inを候補にするには、Scriptのtabでscriptの先頭へ`add bet [a]`と`add raise [a]`を書く。
    scriptの`param`宣言は入力欄として表示し、変更を`[tree.params]`の同名keyへ書く（script本文の既定値は変えない）。
    Scriptのtabとscriptの編集欄は下の「script・TOMLの編集欄」の機能を持つ。
  - Solver: Stopping（Target・Max iterations・Check every・Max time）、Resources and output（Threads・Memory limit・
    Saved streets・Checkpoint interval・Checkpoint at the end）、Algorithm（Schedule・CFR precision・Storage）を書く。
    Profileではlock・incentive・頻度目標を編集できる。上流固定のCLI用pathはTOMLの保存用であり、
    daemonでは停止済みP1のsolutionを持つrunを別に選ぶ。選択中はCLI用pathを保存文書に残しつつ検証・solveの送信から除く。
  - Edit TOML: stepではなく、文書全体を色分け付きで直接編集する。読めるTOMLになるたびに各stepへ反映し、各stepの変更はTOMLへ反映する。
    読めない間はerrorを示し、構文errorの行・列に印を付ける。他のstepと検証は直前に読めた内容のままでSolveできない。自分で書いたcommentや順序は、
    TOMLを直接編集している間は保持し、stepで変更するとformの書式で書き直す。複数行の文字列（`tree.script`）は
    `'''`の複数行literalで書き、その中はscriptとして色分け・補完する。どのstepにも入力欄の無いkeyは消さずに保持し、TOMLの下に一覧で示す。
  - script・TOMLの編集欄: 行番号・現在行の強調・括弧の対応と自動の閉じ括弧・`{ }`の折りたたみ・折り返しを持つ。
    補完はCtrl+Spaceか入力中に、文脈に合う語（topではstreet・`param`・`define`、block内ではeffect・`when`・`if`、
    `jam`・`merge`の後はaction・`>=`/`>`・`within`、条件では変数と`define`名、`position ==`の後は席名、`[`の中はsize literalと`param`名）と
    blockの雛形を出す。語にpointerを置くと、変数の型と意味・予約語の説明・size literalの解釈・`param`/`define`の宣言と直前のcommentを示す。
    daemonの検証errorは該当行に波線と行頭の印で示す（scriptは`line N`の行、TOMLはkeyの行）。検証の警告は行が分からないため印を付けない。
    主なkey: Ctrl+F 検索・置換、Ctrl+G 次を検索、Alt+G 行へ移動、Ctrl+D 次の同じ語を選択、Ctrl+クリック 複数cursor、Alt+ドラッグ 矩形選択、
    Ctrl+/ comment切替、Ctrl+[ / Ctrl+] indent、Ctrl+Shift+[ / Ctrl+Shift+] 折りたたみ・展開、Alt+↑/↓ 行の移動、Ctrl+Z / Ctrl+Y 取消・やり直し。
    Tabはindentで、編集欄から出るにはEscの後にTabを押す。
- Runs: run一覧、iteration・`nash_conv / 2`（cashは%pot、ICMは%prizes）・経過時間、収束chart、
  event、artifact download、cancel・再開。cancel後は保存が終わるまで「Stopping (saving)」と表示する。
- Results: 完了したP1 runをroot から節点ごとに辿る。action cardで手を選び、chance nodeではcardを選ぶ。
  action cardは画面幅に収まらなければ次の行へ折り返し、狭い画面（1280px以下）では詰めて表示する。
  13×13 gridは戦略・EV・Rangeをclassごとに到達weightで集計し、hand tabでcombo別の値を見る。戦略の色はセル幅いっぱいに
  actionの割合で区切り線なしに塗り分ける。actionの色はGTO Wizardの既定に揃え、foldは青、check・callは緑、bet・raiseは
  menuの小さい順に4段の赤（5つ以上は最も明るい赤から最も暗い赤までを等分）である。
  「Lock & re-solve」では現在の節点にlock・incentive・頻度目標を追加し、不要なentryを取り除いてから
  再solveできる。replaceと上流固定の選択はまとめて送るprofileに適用される。
  エクスポートは現在節点の`strategy` CSVを保存する。no-riversのrunで未保存のRiver節点を開くと、そのRiverの部分木を
  保存済みの到達rangeからその場で再solveして表示し（数秒かかり得る）、gridの上に再計算値である旨を示す。
  1回の再solveでそのRiverの全節点を得るため、同じRiverの中の移動は待たない。River cardを選び直すと（同じcardでも）
  再solveの結果を捨てて解き直す。到達しない等で再solveできない節点は理由を示す。

`npm run dev`（`web/`内）は`/v1`を`127.0.0.1:38127`のdaemonへproxyする開発用serverである。

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
| その他 / 1 | 使用中runへのresumeなら停止を待つか別directoryへfork。その他はfile path・権限・I/O診断を確認 |
| `freeze_upstream`のsuit-isomorphism restriction / 2 | suit対称のrange・categoryや`?` runoutを使うか、同じprofile selector（`freeze_upstream`を除く）で基準解をsolveする |
| `no-rivers cannot be used with river profile or frozen nodes` / 2 | Riverに解決されるlock・incentive・frequencyまたは固定祖先があるため、`solution_streets`を`no-rivers`以外にする |

未一致ruleのwarningはerrorではない。P2のnot-checked/incompleteは測定未完了を示す。
型付き入力errorの診断に`resource limit`・`memory limit`・`node limit`等が含まれてもexit 2である。実際の資源上限errorだけがexit 75となる。
P1の一般和設定やP2の暫定方式の品質制限は規範を読み、予算終了と収束を区別する。
