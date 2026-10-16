# solvers — NLH HU Postflop Solver

[English](README.md) | 日本語

No-Limit Hold'emのheads-up postflop（flop・turn・river）を解くsolverです。

## 起動する

### Windows

`start-gui.cmd`をダブルクリックします。ブラウザーでGUIが開きます。

Windowsの警告が出たら、**詳細情報**→**実行**を選びます。
または、ダウンロードしたzipを右クリック→**プロパティ**→**許可する**にチェックを入れてから、展開し直します。

起動しない場合や、すぐにエラーで止まる場合は、`-compat`付きのzipを使ってください。古いCPU（AVX2非対応）向けの版です。

### macOS

**ターミナル**を開いて`cd `と入力し（`cd`の後に空白）、このフォルダーをターミナルにドラッグしてEnterを押します。
続けて次の2行を実行します。ブラウザーでGUIが開きます。

```sh
xattr -dr com.apple.quarantine .
sh start-gui.sh
```

1行目で、未署名のアプリを止めるmacOSの仕組み（Gatekeeper）を、このフォルダーだけ解除します。

### Linux

このフォルダーで`sh start-gui.sh`を実行します。
起動しない場合は、`-compat`付きの版を使ってください。古いCPU（AVX2非対応）向けです。

## GUIでsolveする

1. **Setup**で局面を設定します。最初は表示されている例のままでもかまいません。右側の**Validation**に、必要なメモリが表示されます。
2. 右上の**Solve**を押します。
3. **Runs**で進み具合を確かめます。終わるまで待ちます。
4. **Results**で戦略とEVを見ます。

GUIを使っている間は、起動したときに開いた黒いウィンドウ（ターミナル）を閉じないでください。
閉じると終了します。結果はこのフォルダーの`runs/`に残ります。

## 詳しく知りたいとき

CLIの使い方などは、[利用ガイド](https://github.com/HIRO15254/solvers/blob/main/docs/user-guide.jp.md)にあります。

## ライセンス

MIT OR Apache-2.0のどちらかを選べます。第三者のライセンス表記は`THIRD-PARTY-LICENSES.txt`にあります。
