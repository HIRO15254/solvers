# HU-R0-002 / VM07 診断実測

source07の8件の検証・ビルド、6件の診断stage、Full保存戦略の再評価を、3本の回収archiveと元source/input/binaryのhashへ照合した。132判断点の順序付きメニュー・actor・pot・履歴・残stackが観測記録と一致し、261終端、393公開node、未取得frontier 0を確認した。これは観測した行動木の一致であり、参照解との品質認定ではない。

`condition_match=unverified`、`quality_status=not_evaluated`、`acceptance=null`を維持する。参照の個別版・精度指標と分母、fold時のレーキ、uncalled wagerの返却、丸め規則は未確認で、参照側の全戦略・BRもない。EVが近くても同一有限ゲームや同等exploitabilityは証明できない。

| 評価対象 | BB / OOP EV (BB) | BTN / IP EV (BB) | NashConv (BB) |
|---|---:|---:|---:|
| 参照画面（0.01 BB表示） | 2.37 | 2.76 | 不明 |
| 保存前live平均戦略 | 2.346470095521 | 2.725192641378 | 0.004388569442 |
| Full保存後の量子化戦略 | 2.346470075870 | 2.725192616086 | 0.004389073205 |

保存後の両seat BRは 2.348996703348 / 2.727055061813 BB、BR gainは 0.002526627478 / 0.001862445727 BB。NashConvは両gainの和であり、その半分を開始pot 5.5 BBで割った値は 0.039900665501% になる。この式をGTO Wizardの精度表記と同一視しない。量子化前後の差はJSONに符号付きで保持し、後付けの許容差・品質閾値は選んでいない。

100 chips/BB、pot 550、stack 9750、493/479正weight combos、isoなし。レーキ5%・cap 0.6 BBを仮定し、現在のruntimeはfoldでも両者の実contributionを含むpotから徴収する。例えばbet 2 BB→foldは0.375 BBとなり、matched-potを基準にすれば0.275 BBである。この差をEV合計から補正・認定していない。

1000反復の予算を完了。内部solve wallは 19.185372992 秒、監督processは 19.341924740 秒、wait4によるchild peak RSSは 22,962,176 bytes。内部30秒制限には達しておらず、品質targetも未設定。単独River診断の1回実行であり、性能比較やphase別memoryの証拠ではない。

保存後評価はsource07の研究用helperがFull戦略をロードして行った実測である。このPython verifierはarchive・記録・数値整合を確認し、Rust評価や独立scalar oracleを再実行しない。SOLのSHA-256は保存後auditの前後とも `0bb2bf5a1e3076378a40bb08771754f14a8ad4b20010d5175336c2b4d520ead9` のまま。

source07ビルドで観測されたCPUはAMD EPYC 7B12、Rust 1.97.0、bootは `0727ebb3-36dd-4fae-b978-c629114703ed`。launcherは診断開始前に同じbootを確認するが、独立したstageごとのboot記録はない。systemd外枠はlauncherに記載されており、回収記録から直接確認できるのは内側supervisorの600秒、grace 5秒、kill wait 5秒、40 GiB/8 GiB free/10 GiB disk制限と正常終了・cleanupである。

元のgeneric retention readinessは書き換えない。cross-bundleで必要なsource/binary/input/outputを解決した一方、この3本のarchiveにはbash・Python・Cargoの実行file bytesがなく、記録された前後hashの一致までしか確認できない。rootは後のcodec bundleへ回収したと報告しているが、それは本verifierの対象外であり、この3件を再ハッシュ済みと扱わない。SOLとCKPTのconfig hash・1000反復header、saved auditのconfig hashも一致する。ただしこのstdlib verifierはBLAKE3自体や圧縮frameを再計算しない。SOL/CKPT・compact記録は [evidence-vm07-002](evidence-vm07-002/)、大きなbinaryとsource archiveはJSONに場所・hashを記したlocal bundleへ保持する。

再検証: `python experiments/hu-postflop-r1/reference/vm07-002-verification.py --check`。負例: 同script `--self-test`。詳細は [verification JSON](vm07-002-verification.json) と [実測JSON](vm07-002-report.json)。全bundleの元memberを検査し、retained codeの実行・展開・書換えは行わない。
