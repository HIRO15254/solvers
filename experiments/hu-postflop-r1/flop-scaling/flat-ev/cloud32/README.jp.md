# Flopの32 vCPU比較手順

現行EV scratch実装とflat-EV研究候補を、同じ32 vCPU Spot VM・boot・native build条件で比較する。
公開仕様や本体実装はこの実験によって自動採用されない。

- [事前固定した測定手順](protocol.md): 2種類の突入レンジ、1/2/4/8/16/32 workers、24 warmupと72測定。
- [配布物の記録](pack-receipt.json)と[source manifest](source-manifest.json): 全Cargo source、候補、adapter、実行制御のhash。
- [adapter生成の逆変換照合](provenance.json): 引数上限・usage以外の演算は元adapterと同一。
- [小型VMからの実行・回収](../../../cloud/vm14/README.md): 依存取得後に同じVMを32 vCPUへ変更。新規buildとsolveはGCPのみ。

`pack.py`は小さな転送archiveを作り、`install.py`はhash・相対path・通常file・展開容量・source membershipを検証する。
`runner.py`はpackageと両sourceの一致、同じboot、32 CPU affinity、cgroup制限、絶対期限を検査する。
Linuxの4 smokeが全stateとquality一致してから、baselineだけのpilotで反復数を固定して比較する。
採否は完全な証拠を検証した後にprotocolの時間・RSS・変動幅条件で判断する。

ローカルでは配布準備と短時間の純テストのみを行う。未実行のローカル測定案は[記録](../timing/README.jp.md)を参照。

VM14は測定中にSpot回収された。[実測と原本の保持記録](../../../cloud/vm14/report.jp.md)に
狭いレンジ48条件の照合済み参考値と、全96条件が未完了であることを記録した。
`partial.py`は独立した回収manifestで原本を検証する参考集計器であり、採用判定を出さない。
