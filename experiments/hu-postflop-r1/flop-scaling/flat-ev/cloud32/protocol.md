# EV / flat-EVの32 vCPU比較

ローカル計測は利用者の2026-09-27指示により開始前に取り下げ、GCP Spotで比較する。
この手順を候補の時間を見る前に固定する。目的は同じ全street Flop入力・固定反復での
CFRと公開品質評価のstrong scalingを測ること。外部参照精度や収束目標の認定ではない。

## 同一条件と品質

- 現行EV scratchをbaseline、flat chance出力を組み合わせた研究copyをflatとする。
  runtime sourceの差はengine/solver.rsだけ。凍結oracleを変更しない。
- 両armとも元のCargo workspace/lockfileへ同一holdem exampleを追加する。
  同一32 vCPU VM・boot・toolchain・release profile・native CPUで新規buildする。
  小型機では依存の取得を行い、resize前のnative binaryを計測へ流用しない。
- 先行Windows検査は8条件の全state/quality一致と8汎用境界testsが成功済み。
  Linuxではnarrow・2反復のbaseline1/flat1/baseline32/flat32を先に実行し、
  Linux baseline1の全state bytesとquality JSONへ一致させる。計測標本には数えない。
- 入力は既存narrow（34/30 hands）・expanded（63/160 hands）、全streetにbet/raiseを持つ
  367,662 nodes。DCFR、F32、chance_depth=2、min_children=12と公開品質呼出しを固定する。
  adapterは元の使用方法とworker/反復の許容上限だけを変更し、逆変換で元bytesと照合する。

## Pilotと固定順序

各入力のbaseline1 workerを16反復から実行する。CFRが4秒未満なら32/64/128へ倍増し、
最初に4秒以上となった反復数をその入力の全arm/workerへ固定する。
128でも未達なら128で固定して短時間測定と明記する。候補の時間で反復数を選ばない。
最後のpilot state/qualityをcanonicalとして保持する。

narrow→expandedの順で、各々1/2/4/8/16/32 workers × baseline/flat × round0..3の
48条件、計96 processを実行する。round0はwarmupとして集計から除外する。
roundごとにworkerを昇順/降順、各workerのarmをbaseline→flat/flat→baselineで交互にする。
すべて同じ入力のcanonicalと全state bytes・quality JSONが一致することを要求する。

## 有限資源

単一VM、初期は小型Spot、比較時だけ32 vCPUへresizeする。別bootを同一対照に混ぜない。
クラウドの絶対STOP期限を設定し、実験期限はその15分以上前、実験窓は最大40分。
systemd serviceはMemoryMax=12GiB、swap=0、KillMode=control-groupで全子processを囲む。
各buildは300秒、各solveは120秒以内、RSS監視8GiB、空きphysical2GiB・disk2GiBを要求する。
最初の失敗、品質不一致、boot/CPU affinity変化、期限到達で停止し、自動再試行しない。
buildはCargo jobs=2、計測は順番に行い、他のbuild/solveと重ねない。

全メタデータと固有source/binaryを保持する。stateはbyte一致とhashを確認した後だけ
同じcase/反復のcanonicalへ集約できる。回収archiveの累計は1GiB以下。
停止後に回収とhash照合を行い、VM・disk・予約IPの残存を確認して費用を照合する。

## 事前固定の集計と採用guard

CFR、7回の公開品質walk、その合計、構築・state書込み・process全体を分ける。
3測定標本の全値・中央値・範囲を出し、arm内のspeedup=T(1)/T(p)とefficiency=speedup/pを示す。
OS root peak RSSと要求allocation bytesを区別し、CPUの物理/論理構成を実測して記録する。

候補の局所採用guardは全state/quality一致、1 workerのCFR+quality中央値がbaselineの
105%以内、各入力・workerのroot peak最大値がbaselineの110%以内、両入力の8 workersで
CFR+quality中央値が10%以上短縮すること。いずれかの条件のCFR+qualityで3標本の
最大/最小が1.15を超えれば時間判断を保留する。16/32の遅い結果も省かない。
guard通過後も本体へ適用した通常検証が必要であり、R1全体や外部24条件の受入とは区別する。
