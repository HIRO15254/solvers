# solvers

[English](README.md) | 日本語

Rustで書いた2つのNo-Limit Hold'em solverです。どちらも入力形式`solvers.nlh/v1`を使います。

- **HU Postflop**（P1）: 厳密なvector CFRです。
- **2〜9人のMultiway Preflop**（P2）: 暫定方式のsampled MCCFRです。

P2はterminalの効用を推定するためにpostflopも辿ります。ただし、multiwayのpostflop局面を開始点としては受け付けません。
3人以上の結果は、regretを最小化した近似です。認定されたNash/GTO解ではありません。

## インストール

archiveと`SHA256SUMS`を[GitHub Releases](https://github.com/HIRO15254/solvers/releases)からダウンロードします。
配布binaryに入っているのは**HU Postflop（P1）だけ**です。P2と`derive`は入っていません。
下で説明するソースのworkspaceには、両方の製品が入っています。

archiveの名前は`solvers-<version>-<target>[-compat]`の形です。

| OS・CPU | targetとarchive |
|---|---|
| Windows x86-64 | `x86_64-pc-windows-msvc.zip` |
| Linux x86-64 | `x86_64-unknown-linux-gnu.tar.gz` |
| macOS Apple Silicon | `aarch64-apple-darwin.tar.gz` |
| macOS Intel | `x86_64-apple-darwin.tar.gz` |

Windows・Linuxの通常版は**x86-64-v3（AVX2）**向けです。
AVX2のないCPUでは、`.zip`・`.tar.gz`の直前に`-compat`が付いたものを選んでください。
`-compat`版も**x86-64-v2**は必要で、どんな古い64-bit CPUでも動くわけではありません。

AVX2の有無は次の方法で確かめられます。

- Windows: Microsoftの[Coreinfo](https://learn.microsoft.com/en-us/sysinternals/downloads/coreinfo)で`coreinfo -f`を実行し、`AVX2`に`*`が付いていれば対応しています。
- Linux: `lscpu`の出力に`avx2`があるか見ます。
- Intel macOS: `sysctl -n machdep.cpu.leaf7_features`の出力に`AVX2`があるか見ます。

Intel macOS版はv2の1種類だけです。Apple Silicon版はM1の命令セットを基準にしています。

展開する前に、archiveのhashを`SHA256SUMS`の同じファイル名の行と比べてください。

- PowerShell: `Get-FileHash .\solvers-<version>-<target>.zip -Algorithm SHA256`
- Linux: `sha256sum <archive>`
- macOS: `shasum -a 256 <archive>`

hashが違うものは実行しないでください。

archiveは丸ごと展開します。Windowsではエクスプローラーの**すべて展開**、Linux・macOSでは`tar -xzf <archive>`を使います。
`ui/`はbinaryの隣に置いたままにしてください。RustやNode.jsのinstallは不要です。

展開したディレクトリーで、Windowsは`start-gui.cmd`を、Linux・macOSは`sh start-gui.sh`を実行するとGUIが起動します。
GUIを使っている間は、そのターミナルを開いたままにしてください。

CLIで最初のsolveをするには、同じディレクトリーで次のコマンドを実行します。
PowerShellでは`./solvers`を`.\solvers.exe`に置き換えてください。

```sh
./solvers config new --product p1 --template minimal --out spot.toml
./solvers validate spot.toml
./solvers solve spot.toml --out runs/first
./solvers export runs/first/solution.sol summary
```

v0.1.0のbinaryは**未署名**です。[インストール手順](docs/user-guide.jp.md#1-インストールする)に次の内容があります。

- OS・メモリの要件
- SmartScreen・Gatekeeperへの対処
- PATHの設定（任意）
- daemonの安全な使い方

ブロックを解除するのは、信頼できるReleaseページから取得し、checksumを確かめたbinaryだけにしてください。

## 現状

このリポジトリは、1つの入力形式を共有する2つの製品として作り直しました。

- **NLH HU Postflop Solver**（P1）: 2人がflopを見る局面を、厳密なvector CFRで解きます。
- **NLH Multiway Preflop Solver**（P2）: 2〜9人のpreflopを、sampled MCCFRで解きます。

CLIが読む入力は`solvers.nlh/v1`だけです。他のschemaやschemaのない入力は`NLH001`で失敗します。
削除済みの入力を含む成果物も拒否するので、現行の設定から解き直してください。
`status`、`watch`、`runs ls`は、古いrun directoryも読めます。

P2の方式と品質は、[製品定義](docs/products.jp.md)で方式を決めるまで暫定です。
製品定義には、各製品の範囲と品質も書いてあります。
目標の構成と移行の手順は[再構築計画](docs/plans/two-product-restructure.jp.md)にあります。
再構築前の状態はgit tag `archive/pre-two-products-2026-10-04`に残っています。
作業の状態はLinearで管理しています。詳しくは[状態と作業管理](docs/status.jp.md)を見てください。

## 文書

- [docs/README.md](docs/README.md) — 文書の地図と、どの文書を正とするか
- [AGENTS.md](AGENTS.md) — AIによる開発の共通の入口
- [docs/products.jp.md](docs/products.jp.md) — 2つの製品の範囲・品質・段階
- [docs/plans/two-product-restructure.jp.md](docs/plans/two-product-restructure.jp.md) — 目標の構成と移行計画
- [docs/nlh-input-v1.jp.md](docs/nlh-input-v1.jp.md) — 共通入力形式の規範
- [docs/hu-postflop.jp.md](docs/hu-postflop.jp.md) — P1の計算と成果物
- [docs/mw-preflop.jp.md](docs/mw-preflop.jp.md) — P2の暫定方式と成果物
- [docs/user-guide.jp.md](docs/user-guide.jp.md) — CLIの使い方と結果の読み方
- [docs/cli-reference.jp.md](docs/cli-reference.jp.md) — コマンド、flag、終了コード、daemon API
- [docs/architecture.md](docs/architecture.md) — solver・workspace・CLIの構成
- [docs/app-architecture.md](docs/app-architecture.md) — CLI・daemon・run directory・viewerの境界
- [docs/development.md](docs/development.md) — 環境構築、test、benchmark、変更の進め方
- [LICENSE-POLICY.md](LICENSE-POLICY.md) — AGPLの参照物に関するclean-roomの方針

## workspaceの構成

Rustのworkspaceは11 crateです。

- `spot`は共通の入力を解析し、どちらの製品で解くかを選びます。
- 製品crateは、それぞれのゲームを構築・solve・保存・照会します。
- `solvers` CLIは製品crateを呼び分け、run directoryを管理し、結果を表示します。
- `solversd`は、ローカルまたはリモートでCLIの子processを管理します。

厳密なHU postflopの経路は、flop・turn・riverのどこからでも始められます。
sampledのMultiway Preflopは別のengineです。
`web/`のWeb GUI（製品定義のS5）はdaemonのclientです。境界は[docs/app-architecture.md](docs/app-architecture.md)にあります。

```
crates/
├── cli         # `solvers` binary: 引数、run directory、表示
│               # （config / validate / solve / resume / status / watch / runs /
│               #   inspect / evaluate / export / compare / report / derive）
├── protocol    # ジョブdaemonの版付き通信型
├── daemon      # `solversd`: run directoryを作り、CLIを起動する
├── nlh         # card・range・評価器、2〜9人共通のNLHルールと精算、sizing、tree-script、suit同型
├── economics   # 共通のrake、厳密・求積ICM、効用の設定と検証
├── spot        # 共通入力の解析、正規化、lineの再生、製品の選択
├── cfr-ref     # 差分testのための凍結したscalar CFR oracle
├── hu-engine   # 中核: public tree、storage、discount schedule、vector CFR、best response
├── mw-preflop  # P2のmenu policy、external-sampling MCCFR、EHS² bucket、solve loop、.mwsol、view
├── runfiles    # run directoryの契約、進捗の指標、設定のhash
└── hu-postflop # 厳密な複数streetのpostflop、payoff pipeline、solve loop、.sol・checkpoint、view、report、toy game
web/            # Web GUI（React + TypeScript、Vite）: solversd上のSetup / Runs / Results
```

ファイルの置き場所は次のとおりです。

- 現行の仕様と構成: `docs/`
- 実行する計画: `docs/plans/`
- 再利用する補助scriptとそのtest: `tools/`
- 採用した実験の証拠: `experiments/`
- 新しいsolverのrun: Gitで無視する`runs/`
- machineごとのcache: Gitで無視するcache directory
- Cargoの出力: `target/`

利用者向けの入力は[examples/README.md](examples/README.md)に一覧があります。
性能測定用の入力は`examples/bench/`に、回帰testのfixtureは各testの隣にあります。

## ソースからbuildする

Rust stable、Cargo、Gitをinstallします。OSごとの前提は[開発環境](docs/development.md)にあります。
ソースのcheckoutで、CLIとdaemonをbuildします。

```sh
cargo build --release -p cli -p daemon
```

binaryは`target/release/`にできます（Windowsでは`.exe`）。
既定のbuildには、暫定のP2と`derive`が入ります。
配布物と同じP1だけのbuildにするには`--no-default-features`を付けます。
下のコマンドは開発者向けの例です。

どの設定ファイルも`schema = "solvers.nlh/v1"`を宣言します。solveのたびにrun directoryが1つできます。
templateは`solvers config new --product p1|p2 --template minimal|full`で作るか、利用者向けの例をコピーします。

```sh
cargo test --workspace            # 正しさの検査（Kuhn・Leducの既知解、oracleとの差分）
cargo run -p cli --release -- solve examples/hu-postflop/river_small.toml --out runs/river-small

# --- Preflop ---------------------------------------------------------------
# Multiway Preflop v1: 検証してから、自己完結したrun directoryを1つ書く。
cargo run -p cli --release -- validate examples/mw-preflop/3max_smoke.toml
cargo run -p cli --release -- solve examples/mw-preflop/3max_smoke.toml \
    --out runs/v1-smoke

# 抽象化の表はrunごとではなくmachineごとにcacheする。
# 最初のsolveで作り、後のsolveは互換性のあるcacheを読む。
cargo run -p cli --release -- --cache-dir ~/.cache/solvers \
    solve examples/mw-preflop/3max_smoke.toml --out runs/v1-cached

# 長いrunの前に、public treeとpolicy arenaの大きさを確かめる:
cargo run -p cli --release -- validate examples/mw-preflop/6max_100bb_cash.toml \
    --resources

# run directoryがinterfaceのすべてなので、いつでも別のprocessからrunを見られる。
# solveをCtrl-Cで止めても、`resume`でcheckpointから続けられる。
cargo run -p cli --release -- status runs/v1-smoke
cargo run -p cli --release -- watch  runs/v1-smoke --from 0
cargo run -p cli --release -- runs ls runs
cargo run -p cli --release -- resume runs/v1-smoke

# 保存したpreflopの平均戦略から、heads-upのflop入力を導出する。
# この短いsmoke runでは、sweepを増やすと選んだlineを十分に通る。
cargo run -p cli --release -- resume runs/v1-smoke --max-sweeps 64 --evaluation-cadence 65
cargo run -p cli --release -- derive runs/v1-smoke --line "BTN c, BB x" \
    --board "Ks 7h 2d" --out runs/derived-flop.toml
# P1のtree・solver設定は--baseで渡す。詳しくはuser guideのderiveの節を見る。

# --- Postflop --------------------------------------------------------------
# 厳密なpostflopのsolve（treeを作る前にメモリの見積もりを表示する）:
cargo run -p cli --release -- solve examples/hu-postflop/flop_srp.toml --out runs/srp20

# 対話式の戦略ブラウザー: solveしてから、13x13のANSI gridでnodeを見て回る
# （`show`、`go <action>`、`grid <action>`、`eq`、`combos AKs`など）:
cargo run -p cli --release -- inspect examples/hu-postflop/river_small.toml

# solve済みの成果物を機械可読のviewで出力する（summary / tree / actions /
# strategy / ev / range。`--node all`で保存した全nodeを出す）:
cargo run -p cli --release -- export runs/srp20/solution.sol ev \
    --node all --format csv --output ev.csv

# 同じtreeの2つのsolveをnodeごとに比べる:
cargo run -p cli --release -- compare runs/a/solution.sol runs/b/solution.sol

# board横断の集計CSV（boardごとの頻度、EV、equity）:
cargo run -p cli --release -- report examples/hu-postflop/river_small.toml \
    --boards "2c 7d 9h Js Qs,2c 7d 9h Js Ks" --output report.csv
```

## daemonでsolveを実行する

`solversd`はHTTPで設定を受け取り、run directoryを用意して、そこで`solvers`を起動します。
ジョブの状態はrun directoryに残ります。再起動したときは、runs rootを読み直してすべてのrunを見つけます。
HTTPのsolution viewは、HU postflopの`.sol`もMultiwayの`.mwsol`も`solvers export`に任せます。

```bash
cargo build --release -p cli -p daemon
cargo run -p daemon --release -- --runs runs --max-concurrent 1
```

起動時にbearer tokenを表示します。すべてのrequestにtokenを付けます。

```bash
curl -H "Authorization: Bearer $TOKEN" localhost:38127/v1/runs
curl -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
    -d "{\"configToml\": $(jq -Rs . < config.toml)}" localhost:38127/v1/runs
curl -H "Authorization: Bearer $TOKEN" "localhost:38127/v1/runs/$ID/events?from=0"
curl -H "Authorization: Bearer $TOKEN" localhost:38127/v1/runs/$ID/artifacts
curl -H "Authorization: Bearer $TOKEN" localhost:38127/v1/runs/$ID/solution/summary
```

daemonが作るrunは普通のrun directoryです。`solvers status`や`solvers watch`でも読めます。

### Web GUI

GUIでは次のことができます。

- Setup: P1の入力を編集する。
- Runs: runとその収束を追う。
- Results: 終わったHU postflopの解をnodeごとに見る。

Node.js 22以降で一度buildし、daemonに配信させます。

```bash
(cd web && npm ci && npm run build)
cargo run -p daemon --release -- --runs runs --ui web/dist
```

daemonが表示する`gui:`のURLを開きます。URLのfragmentにtokenが入っており、GUIはそのoriginにtokenを保存します。
GUIを開発するときは、`web/`で`npm run dev`を実行します。`/v1`へのrequestは`127.0.0.1:38127`のdaemonへproxyされます（`SOLVERSD_URL`で変更できます）。

loopback以外にbindするときはTLSが必須です。tokenはheaderで送るので、暗号化しない接続では経路上の誰にでも渡ってしまいます。

```bash
openssl req -x509 -newkey rsa:2048 -nodes -days 365 \
    -keyout key.pem -out cert.pem -subj "/CN=$(hostname)" \
    -addext "subjectAltName=DNS:$(hostname)"
cargo run -p daemon --release -- --bind 0.0.0.0:38127 \
    --tls-cert cert.pem --tls-key key.pem
```

証明書がない場合は、SSH tunnelでリモートのdaemonのloopback addressにつないでください。

## ライセンス

MIT OR Apache-2.0のどちらかを選べます。外部の参照物の扱いは[LICENSE-POLICY.md](LICENSE-POLICY.md)にあります。
