# 全street Flopのローカルworker比較

このローカル計測は実行しない。2026-09-27の利用者指示でローカル計算資源を抑えるため、
adapter準備後、timing用のbuild・pilot・matrixを開始する前に取り下げた。
以下は未実行の比較設計であり、GCP向け手順の根拠として残す。

[protocol.md](protocol.md)は時間比較の前に固定した手順。
現行EV scratchとflat-EV候補の両方を同じadapterでlinkし、同じ反復数・全state・
公開品質bitsを要求する。ローカルは8 physical cores /16 logical processorsであり、
32 vCPUの試験ではない。背景負荷があるため、全core独占も仮定しない。

[prepare.py](prepare.py)は以前の[native adapter](../../native-solve/solve.rs)の
usageとworker/反復上限だけを変更する。rustfmt後に差分を逆変換して元bytesとの一致を
確認し、[provenance.json](provenance.json)に入出力pinを残す。
実装やtimer、状態形式、品質式、DCFRの設定は変更しない。

準備初回はworker guardの長いエラー文字列がrustfmtで複数行になり、逆変換のanchorが
一致しなかったため、候補を出力する前に停止した。文字列を`workers`に短縮した再準備で
逆変換が成功した。solve実行の失敗や品質不一致ではない。

出力状態は各caseのbaseline pilot最終結果をcanonicalとする。warmupは測定集計から除外し、
1/2/4/8/16 workersの全条件を保持する。allocation計装の結果とは分けて解釈する。
ここで固定反復の一致が成立しても、収束目標や外部参照との品質一致を認定しない。
