# 2026年9月Multiway実験の共有スクリプト

このディレクトリは[実験索引](../README.md)に載せた当時の測定・集計・検証用コードを保存する。現在の開発手順ではない。スクリプト本体とテストは、実験記録が固定したSHA-256を維持するため、移動時に内容を書き換えていない。

現行の保持証拠を検査する場合は、旧runnerの代わりに[quality-evidence/verify.py](../quality-evidence/verify.py)を使う。保持ファイルと集約結果だけを読み、solverを起動しない。旧スクリプト全体の再現性が回復したという意味ではない。

| ファイル | 当時の役割 |
|---|---|
| [run_average_sampling_measurement.ps1](run_average_sampling_measurement.ps1) | 研究runnerやcheckpoint監査のprocess時間、peak working set、入出力hashを採取 |
| [run_checkpoint_load_bench.ps1](run_checkpoint_load_bench.ps1)・[run_checkpoint_write_bench.ps1](run_checkpoint_write_bench.ps1) | checkpoint入出力のWindows計測 |
| [multiway_convergence_bench.py](multiway_convergence_bench.py) | 初期2bb fixtureの旧convergence matrixを生成・実行 |
| [summarize_*.py](summarize_checkpoint_write.py) | 各実験の条件、元データ、依存スクリプトのhashを検証して結果を集計 |
| [tests/](tests/) | 集計器の専用テスト |
| [gcp_*.py](gcp_average_sampling_pilot.py) | 当時のクラウド実験補助 |

実験固有のprepare、run、validate、renderスクリプトは該当する実験のoutput/にもある。入力・出力・元コードの対応は各実験のexperiment.jsonで確認する。
コピーした旧convergence/GCP pilotの専用テストは、移動先のスクリプトをimportし、fixtureとテスト一時出力をリポジトリ内で見つけるために経路だけ変更した。runner本体は同一バイトで保存している。旧GCP runnerは自身の配置からリポジトリrootを求めるため、移動先からの実行には元のtools/配置の復元か別途修正が必要である。

## 移動後の再実行

保存済みJSONとスクリプトは旧 runs/、tools/、docs/validation/ のパスを固定している。例えばcheckpoint書込みの集計器に移動先のoutput/を渡すと、スケジュール検査で ValueError: case job location となることを確認した。旧パスに入力を復元した隔離環境、または元記録とは別の移設対応validatorが必要である。保存された「検証成功」は実験実施時の結果を示す。
