# 呼出し内のworker scratch再利用

現行productionをbaseline、workerごとの有限scratch bankを加えた研究copyをworkerとする。
flat-EV出力案は混ぜない。候補の時間を得る前に以下の判定を固定する。
凍結oracle・ゲームの入力・CFRの演算順・child順の加算・storage形式は変更しない。

目的は並列タスク間のscratch再利用でFlopの時間を減らせるかの判定である。
計算終了時にbankを破棄し、異なるゲーム・solve呼出し間には持ち越さない。
入口rangeのcompact化とriver showdownの線形走査は既存productionのまま使う。

## 固定条件

- 両armを同じ32論理CPUのSpot VM、同じboot、Rust 1.97.0、同じCargo.lock、release/nativeで新規buildする。
  小型VMではbootstrap・依存取得のみ。測定中にはbuildや別solveを重ねない。
- candidateのengine・holdem・cfr-refのunit/integration testsをreleaseで実行し、失敗したら計測を開始しない。
  全workspace通常検証は、候補採用が妥当と判断できた後、本体反映前に別途必要である。
- narrow（34/30 hands）とexpanded（63/160 hands）、367,662 nodes、全street、DCFR/F32、
  chance_depth=2/min_children=12。入力adapterは以前のCloud32と完全同一bytesを使う。
- 最初にnarrow2反復のbaseline1/worker1/baseline32/worker32を行い、全state bytesとquality JSONの一致を要求する。
- 各入力のbaseline1のCFR時間が4秒以上になる最初の16/32/64/128反復を選ぶ。
  128でも4秒未満なら短時間として明示する。candidateの時間で選び直さない。
- 各入力1/4/16/32 workers × 2 arms × rounds 0..3 = 32条件、計64条件。
  round0はwarmup除外、残り3回を測定。roundごとworker昇順/降順、arm順も交互。
  入力順はnarrow→expanded。各solveでpilotの全state bytes・quality JSONへの一致を要求する。

## 保存・有限資源

Cloud32のsupervisorによりbuild/test各300秒、solve各120秒、RSS8GiB、空きphysical/disk各2GiB。
外側systemdはmemory12GiB/swap0、全子process停止、実験窓40分以内。
別途クラウド絶対STOP期限と、少なくとも15分の回収余白を設定する。失敗・回収時に自動再試行しない。
ローカルでは編集・小さな純粋testとmetadata照合のみ行う。

測定後、参照ファイルをfsyncし、各stageの完了receiptをimmutable publishする。
全byte照合・fsync済みgzipと永続化receiptを保存してからrawを移動・削除する。
各入力32条件完了時に元boot/plan/buildとimmutable参照を持つ独立case manifestを保存する。
後続caseの中断を先行caseへ補完しない。部分結果は参考集計のみで、採用guardを通過できない。
本体の演算時間にはこの保存処理を含めず、whole-process時間と保持処理を区別する。

## 集計・採用guard

CFR、公開品質7 walks、合計、構築、state書込み、whole-process、OS peak RSSを保存する。
全標本・中央値・範囲、各armの1 worker比speedupと効率を出す。
32論理CPUの物理core数・SMT対応を記録し、plateauの原因を時間だけから断定しない。

採用の必要条件は全state/quality一致、両入力16 workersのCFR+quality中央値がbaselineの90%以下、
1 workerでは105%以下、全条件のpeak RSS最大値がbaselineの110%以下。
いずれかのCFR+qualityの3標本max/minが1.15を超えた場合は時間の判断を保留する。
16/32の結果が悪くても省かない。これは局所候補のguardで、R1全体・外部24条件の認定ではない。
