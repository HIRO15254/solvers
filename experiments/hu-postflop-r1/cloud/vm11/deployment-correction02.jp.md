# VM11の2回目の起動設定

proof01は旧CLIのrelease buildがexit 0で終わった直後、host条件の同一性検査で停止した。
性能測定156 processはすべてskipped。失敗時点の比較対象host値は保存されておらず、
停止後の同じboot ID・CPU model・topologyだけでは原因を断定できない。
原証拠とportable検証は[proof01](../../final-pipeline/proof01/README.jp.md)へ保持する。

初回planのCPU制限辞書にはsystem.sliceのcpu.maxだけがあり、service自身のcpu.maxは無かった。
比較中にCPU controllerが有効化・無効化されると、制限が同じでもファイル集合が変わり得る。
これは候補原因であり、実際の失敗原因の認定ではない。
[systemdの公式仕様](https://github.com/systemd/systemd/blob/main/man/systemd.resource-control.xml)
ではCPUWeightの設定によりcpu controllerが有効化され、kernel既定weightは100とされる。

19:14:50 UTCに、CPUWeight=100を明示した別unit `solvers-r1-vm11-final02`を起動した。
wrapperはprepare前にservice自身のcpu.maxが存在し、cpu.weightが100であることを検査する。
比較器・protocol・両production source・停止目標・標本数・採用条件は変更しない。
旧新targetとproof directoryは新規の02を使い、01の標本やbuild結果を継ぎ足さない。
性能結果を見て条件を選び直す変更ではなく、測定前の実行環境の明示化である。

同じVM・boot・予約$3の内側で、12GiB/swap0の制限を保つ。
測定期限21:34:51 UTC、cloud STOP 21:54:51 UTCは延長しない。
この設定変更の承認は新しい性能成功や初回原因の解決認定を意味しない。
