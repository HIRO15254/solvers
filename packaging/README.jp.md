# solvers — NLH HU Postflop Solver

[English](README.md) | 日本語

このarchiveは**NLH HU Postflop Solver**（P1）のbuild済み配布物です。
No-Limit Hold'emで2人がflop・turn・riverを見る局面を、指定したbet treeについて厳密に解きます（vector CFR、card abstractionなし）。
戦略、ハンドごとのEV、exploitabilityを出力します。

このbuildには、Multiway Preflop Solver（P2）と`solvers derive`は**含まれません**。
P2の入力や成果物を渡すと、明示的なerrorで拒否します。

ソースコード、issue、詳しい文書は<https://github.com/HIRO15254/solvers>にあります。

## 内容

| ファイル | 用途 |
|---|---|
| `solvers` / `solvers.exe` | コマンドラインのsolver |
| `solversd` / `solversd.exe` | ローカルのジョブdaemon兼Web GUIサーバー |
| `ui/` | `solversd`が配信するWeb GUI |
| `start-gui.cmd` / `start-gui.sh` | daemonを起動し、ブラウザーでGUIを開く |
| `CHANGELOG.md`、`LICENSE-*`、`THIRD-PARTY-LICENSES.txt` | 変更履歴とライセンス |

## ダウンロードの選び方と展開

このarchiveと`SHA256SUMS`は<https://github.com/HIRO15254/solvers/releases>から入手できます。
ファイル名は`solvers-<version>-<target>[-compat]`の形です。末尾は環境によって次のとおりです。

- Windows: `x86_64-pc-windows-msvc.zip`
- Linux: `x86_64-unknown-linux-gnu.tar.gz`
- Apple Silicon: `aarch64-apple-darwin.tar.gz`
- Intel macOS: `x86_64-apple-darwin.tar.gz`

Windows・Linuxの通常版は**x86-64-v3（AVX2）**が必要です。
AVX2のないCPUでは、`-compat.zip`または`-compat.tar.gz`を選んでください（**x86-64-v2**は必要です）。
Intel macOS版はv2だけです。Apple Silicon版はapple-m1向けです。

実行する前に、archiveのSHA-256が`SHA256SUMS`の同じファイル名の行と一致するか確かめてください。

- PowerShell: `Get-FileHash <archive> -Algorithm SHA256`
- Linux: `sha256sum <archive>`
- macOS: `shasum -a 256 <archive>`

archiveは丸ごと展開してください。Windowsでは**すべて展開**、Linux・macOSでは`tar -xzf <archive>`を使います。
`ui/`はbinaryと同じ場所に置いたままにしてください。Rust、Cargo、Node.jsのinstallは不要です。

動作要件とCPUの確かめ方は<https://github.com/HIRO15254/solvers/blob/main/docs/user-guide.jp.md#1-インストールする>にあります。

## すぐに使う

GUI: 展開したディレクトリーで、Windowsは`start-gui.cmd`を、Linux・macOSは`sh start-gui.sh`を実行します。
`solversd`が起動してGUIが開きます。run directoryはbinaryの隣の`runs/`に作られます。
GUIを使っている間はウィンドウを開いたままにしてください。閉じるかCtrl-Cを押すと、daemonが止まります。

CLI:

Windows PowerShellでは、下の`./solvers`を`.\solvers.exe`に置き換えてください。

```sh
./solvers config new --out spot.toml       # 有効なP1のtemplate
./solvers validate spot.toml
./solvers solve spot.toml --out runs/first
./solvers export runs/first/solution.sol summary
```

## 未署名のbinary

binaryにはコード署名がありません。

- **Windows**: SmartScreenが「WindowsによってPCが保護されました」と表示することがあります。
  - **詳細情報**→**実行**を選びます。
  - または、初回起動の前に、展開したディレクトリーのPowerShellで`Get-ChildItem -Recurse | Unblock-File`を実行します。
- **macOS**: Gatekeeperが、インターネットから取得したファイルを止めます。
  - 展開したディレクトリーのターミナルで`xattr -dr com.apple.quarantine .`を一度実行します。
  - または、一度開こうとした後に、**システム設定→プライバシーとセキュリティ→このまま開く**を選びます。
  - Gatekeeper全体を無効にはしないでください。
- **Linux**: 追加の手順はありません。

実行する前に、Releaseページの`SHA256SUMS`と照合してください。
ブロックを解除するのは、信頼できるReleaseページから取得し、checksumが一致したものだけにしてください。

daemonは、TLSを設定しない限りloopbackで使ってください。loopback以外にbindする場合はTLSが必須です。
bearer tokenと、tokenを含むGUIのURLは秘密情報として扱ってください。

## ライセンス

MIT OR Apache-2.0のどちらかを選べます。第三者のライセンス表記は`THIRD-PARTY-LICENSES.txt`にあります。
