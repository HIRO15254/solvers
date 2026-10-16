# solvers

[English](README.md) | 日本語

No-Limit Hold'emのheads-up postflop（flop・turn・river）を解くsolverです。ブラウザーのGUIで使えます。

## クイックスタート

[Releasesページ](https://github.com/HIRO15254/solvers/releases)から自分のOSのファイルをダウンロードし、下の手順で起動します。
RustやNode.jsなどのinstallは不要です。

### Windows

1. `solvers-<version>-x86_64-pc-windows-msvc.zip`をダウンロードします。
2. zipを右クリックし、**プロパティ**を開きます。下の方にある**許可する**にチェックを入れて、**OK**を押します。
   この操作をしないと、起動時にWindowsの警告が出ます。
3. zipを右クリックし、**すべて展開**を選びます。
4. 展開したフォルダーの`start-gui.cmd`をダブルクリックします。ブラウザーでGUIが開きます。

起動しない場合や、すぐにエラーで止まる場合は、`-compat`付きのzipで1からやり直してください。
古いCPU（AVX2非対応）向けの版です。

### macOS

1. 自分のMacに合うファイルをダウンロードします。
   - Apple Silicon（M1以降）: `solvers-<version>-aarch64-apple-darwin.tar.gz`
   - Intel: `solvers-<version>-x86_64-apple-darwin.tar.gz`
2. ダウンロードしたファイルをダブルクリックして展開します。
3. **ターミナル**を開いて`cd `と入力し（`cd`の後に空白）、展開したフォルダーをターミナルにドラッグしてEnterを押します。
4. 次の2行を実行します。ブラウザーでGUIが開きます。

   ```sh
   xattr -dr com.apple.quarantine .
   sh start-gui.sh
   ```

   1行目で、未署名のアプリを止めるmacOSの仕組み（Gatekeeper）を、このフォルダーだけ解除します。

### Linux

```sh
tar -xzf solvers-<version>-x86_64-unknown-linux-gnu.tar.gz
cd solvers-<version>-x86_64-unknown-linux-gnu
sh start-gui.sh
```

起動しない場合は、`-compat`付きの版を使ってください。古いCPU（AVX2非対応）向けです。

### GUIでsolveする

1. **Setup**で局面を設定します。最初は表示されている例のままでもかまいません。右側の**Validation**に、必要なメモリが表示されます。
2. 右上の**Solve**を押します。
3. **Runs**で進み具合を確かめます。終わるまで待ちます。
4. **Results**で戦略とEVを見ます。

GUIを使っている間は、起動したときに開いた黒いウィンドウ（ターミナル）を閉じないでください。
閉じると終了します。結果は展開したフォルダーの`runs/`に残ります。

## 詳しく知りたいとき

- [利用ガイド](docs/user-guide.jp.md): 動作要件、ダウンロードの確認（SHA256SUMS）、CLIの使い方、結果の読み方。
- 配布版に入っているのはHU Postflop（P1）だけです。2〜9人のPreflop solver（P2）は、ソースからbuildした場合だけ使えます。
- 開発者向けの情報は[docs/README.md](docs/README.md)と[開発ガイド](docs/development.md)にあります。

## ライセンス

MIT OR Apache-2.0のどちらかを選べます。外部の参照物の扱いは[LICENSE-POLICY.md](LICENSE-POLICY.md)にあります。
