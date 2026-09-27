# combined flat+EV native診断の実行入口

[run.py](run.py) はSHA固定の [EV-scratch driver](../../ev-scratch/run.py) をimportし、
prepare/build/stage/runの実装をそのまま使う。共有driverのファイルやglobalsは変更しない。
production EV sourceをoriginal identityとしてコピーした後、コピー側solver.rsだけを
候補 `ec9e5e6b5230684fce6f2315370ceda0e05fb4346eb7e4ff44ea05e6028aabcd` へ置換する。
最終snapshot archive、source pins、候補provenance、driver、EV→candidate patchをplanへ固定する。

engine/game/holdemのmetadata tagと通常版/計測版adapterのcrate名のみを新規名にする。
再利用release依存・compile flags・5buildの順序・2入力×2adapter×1/2workerの8solve、
2反復・512MiB Job・各60秒の上限は共有driverと同じ。各solveの全stateとqualityは
保存済みbaseline canonicalへ直接byte照合する。性能・収束の認定は行わない。

```text
python -B experiments/hu-postflop-r1/flop-scaling/flat-ev/native/run.py prepare
python -B experiments/hu-postflop-r1/flop-scaling/flat-ev/native/run.py build
python -B experiments/hu-postflop-r1/flop-scaling/flat-ev/native/run.py run
```

各phaseは明示実行で、prepareからnativeプロセスを起動しない。固定出力は
`runs/flop-flat-ev01`、新規build先は`target/flop-flat-ev01`。既存出力への再試行は拒否する。
共有driverを指すsupervisor identityと、追加wrapperを含むplan controlsを合わせて
呼出し経路を結び付ける。source archiveは実際のcombined候補であり、
production EV sourceのpinを候補pinへ書き換えない。

今回の固定実行は5 buildと8 solveがすべて終了コード0で完了した。
両入力について通常版・計測版の1/2 worker、2反復の全F32 stateと公開API quality JSONを
既存baseline canonicalへ直接byte照合し、8実行すべて一致した。
この確認は未収束の小反復診断であり、品質目標の達成、速度、32 workerでの正しさを示さない。

[counts01.json](counts01.json) の要求byte数は、既存EV-scratch実行と今回の別実行を比較する。
reallocは新しい要求サイズを数え、解放済み領域も累積するため、RSS・生存メモリ・時間の指標ではない。

|入力 / worker|CFR: EV → combined|7 quality walks: EV → combined|
|---|---:|---:|
|narrow / 1|24,120 → 24,120|92,448 → 92,448|
|narrow / 2|177,865,320 → 65,393,584|226,981,116 → 61,997,292|
|expanded / 1|105,768 → 99,232|331,804 → 331,804|
|expanded / 2|641,597,236 → 149,991,380|928,363,776 → 163,397,200|

expanded / 1のCFRは6,536 bytes異なり、EV側の52 alloc / 1 zeroedに対して
今回49 alloc / 0 zeroedだった（reallocは両方53）。原因は特定していない。
全processのphase計数にはRayonの背景作業が入り得るため、将来の計数一致や単独要因の効果を保証しない。
4条件のquality計数はいずれもzeroed 0、全phaseの失敗allocationは0だった。

[proof01/manifest.json](proof01/manifest.json) は110 payload / 1,675,445 bytesを保持する。
107個の元ログ・plan・実行結果・source archive、2個の計測対象実行ファイルgzip、計数比較を含む。
8個のstateは既存2 canonicalのhash参照で重複排除し、元rawとtargetは削除していない。
compiler・再利用外部ライブラリ・生成rlibはidentityのみで、完全な再build環境の複製ではない。

[checks02/receipt.json](checks02/receipt.json) は小さな保持処理とPython構文検査の記録である。
利用者のローカル計算抑制指示に従い、保持時には大きなstateを再hashせず、実行時の直接比較・hashと
現存ファイルの長さを使った。canonical gzipのhashも固定planから再利用したことをmanifestに明記した。
[verify.py](verify.py) は独立の完全保持検査用に用意したが、この保持処理では実行していない。
したがって、完全な保持検査の成功と、すでに完了したnative実行時の照合を混同しない。
