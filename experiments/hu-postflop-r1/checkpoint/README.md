# HUの実中断・再開対照

SOL-12のcheckpoint継続とCLIの終了コードを検証する研究用の小ケース。
`river.toml`のraked River、F32、1 worker、1,000反復を固定する。
実行時間・資源改善や外部参照品質の測定には使わない。

## 修正と検証対象

CLI規範は初回SIGINTでcheckpointを保存して130を返すが、HUは取消後に
`Ok(())`を返し、明示exit codeを設定しないため0で終了していた。
`crates/cli/src/main.rs`で、成功経路の明示codeが0かつ取消済みの場合に130を返す。
資源停止75やエラー経路のcodeを優先する。公開契約の変更はない。
既存のUnix ignored統合testも130を要求し、子processの終了待ちを有限にする。

## 固定手順

`protocol.json`を実行前に固定し、`run.py`が以下を順番に実行する。

1. 公開された非最終checkpointを観測して、既存supervisorへSIGINTを要求する。
   supervisorがWindowsの専用consoleへ実Ctrl-Breakを送る。
   child/supervisorとも130、協調停止、子process回収完了を要求する。
2. 中断点 `0 < m < 1000` から新しい出力先へ再開する。同じconfigで直行1,000反復も実行する。
3. `hu_checkpoint_audit`でconfig、manifest、event、進捗の継承、最終checkpointの全bytesと
   全decoded state、Full SOLの全fieldを比較する。SOLでは`wall_secs`だけを除外する。
4. 両方のFull SOLを既存の`hu_saved_profile_audit`で読込み、保存された量子化profileの
   EV・BR・gain・NashConvを再計算して完全一致を要求する。

全工程の時間枠は600秒。各solveは60秒、照合は30秒、協調停止猶予15秒、強制終了待ち5秒。
source、build記録、3実行binary、config、監視器を実行前にhash固定し、各段階の出力を後続段階の入力として固定する。
最初の失敗を保持して中止し、反復数の事後変更、終了済みcheckpointの0反復再開、黙示の再試行を認めない。
Windowsは既存のJob containmentを使用する。この実行器ではLinuxを明示拒否する。
Linuxで再実行する場合は有限の外側systemd/cgroupの検証を追加する必要がある。

実行には`r1.hu-checkpoint-build-plan/v1`のsource/toolchain/argv記録と、成功した
`tools/run_supervised.py`のbuild記録が必要。対応する3binaryを次でbuildする。

```text
cargo build --locked --offline -p cli --bin solvers --example hu_checkpoint_audit --example hu_saved_profile_audit
```

CLI研究exampleの2 unit testsは次で実行する。

```text
cargo test --locked --offline -p cli --example hu_checkpoint_audit
python -m unittest discover -s experiments/hu-postflop-r1/checkpoint -p test_run.py -v
```

実中断の対照は初めての出力directoryを指定して実行する。資源枠は直前host観測から決める。
Windowsの例（build記録の名前は実際のものへ置換）:

```text
python experiments/hu-postflop-r1/checkpoint/run.py --build-plan runs/BUILD/plan.json --build-record runs/BUILD/supervisor.json --out runs/NEW-CONTRAST --memory-bytes 536870912 --min-free-memory-bytes 3000000000 --disk-reserve-bytes 4294967296
```

## 判定範囲

この対照は1つのraked Riverの保存・継続の正しさを扱う。I16、chance/Turn/Flop、複数worker、
外部24caseの品質、性能改善、R1総合受入へは外挿しない。
保持した実行結果と通常検証は[Windowsの20→1000反復の証拠](evidence-20260926/README.md)に記載する。
