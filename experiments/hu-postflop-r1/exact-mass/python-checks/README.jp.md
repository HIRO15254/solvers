# Python検証の実行記録

一般Python testsは次のコマンドで**39件成功**した。unittestの報告時間は12.406秒、
外側監視の経過時間は12.750秒。Cargo・solver・cloud操作は実行していない。

```text
C:\Python313\python.exe -B -m unittest discover -s tools/tests -v
```

[general-plan.json](general-plan.json)に作業directory、Python identity、環境、Git HEAD、
sourceと51個の実入力fixtureのbytes/SHA-256、外側supervisorの全引数を保持する。
[general-result.json](general-result.json)は終了結果と実行後hash、原ログ等のhashを記録する。
source・入力の実行前後一致、子processの終了0、監視終了0、cleanup完了を確認した。

原bytesは[stdout](general.stdout.log)、[stderr](general.stderr.log)、
[resource samples](general.samples.jsonl)、[supervisor record](general-supervisor.json)で確認できる。
unittestの一覧と成功要約はstderrに出力され、stdoutは空である。
外側の上限は240秒・sampled RSS 768 MiB、空きRAM 3 GiB・disk 1 GiB、grace/kill待ち各5秒。
test自身も小型subprocessのtimeout・強制終了・Windows Job cleanupを検査する。
TEMP/TMPには固有のrepo `.cache` directoryを用い、test fixtureも既存の`.cache/tool-tests`内で生成する。

writerが実行した実験専用26 testsは[writer-transcript.json](writer-transcript.json)に
**transcript-only**として保持する。報告されたコマンドは次の通りで、終了0、26件、49.579秒、OKと記録されている。

```text
C:\Python313\python.exe -B experiments/hu-postflop-r1/exact-mass/test_run.py
```

run.py/test_run.pyの報告SHAは手元の実fileと一致した。原stdout/stderrはこの記録には存在せず、
ログ生成のための再実行もしていない。先行するnegative testの期待regex不一致は、
実際にはより早い検査で拒否されていたため期待値を修正し、その後の全26件成功として報告された。
一般39 testsの原ログ付き証拠と、この実行報告の根拠強度を区別する。
