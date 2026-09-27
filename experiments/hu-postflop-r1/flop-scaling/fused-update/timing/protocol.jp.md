# Fused CFR update の有限比較

未実行の研究用計測器。production、凍結oracle、過去の証拠は編集しない。baselineに対して候補の `solver.rs` / `storage.rs` / `lib.rs` の3ファイルだけを変更する。候補だけに `engine/tests/fused_update.rs` を追加し、両方へ同一のCPU計測adapter `cpu-occupancy/adapter/solve.rs` を `holdem` の example `flop_cpu_occupancy_probe` として追加する。v1 provenanceが実装を固定し、v2はbyte同一のrustfmt確認証拠。CPU adapterはVM16で使用したものと同一で、新しい計測摂動の認定ではない。

## 入力と実行順

packageの `manifest.json` schemaは `r1-fused-update-package/v1`。`source/` と `source_pins` が元の全workspace snapshotを固定する。`installation.json` はmanifestのbytes/SHA256、archive SHA256、source revision/files、destination、builds_or_solves_started=0を持つ。配備後の `source-baseline` / `source-candidate` は元workspace、共通adapter、指定された候補3差分とfixture以外の追加・欠落・変更を認めない。ソースarchiveは各1MiB未満、展開16MiB未満。root側のpackage準備で起動前にサイズも確認する。

```text
python run.py prepare --baseline-source SOURCE_BASELINE --candidate-source SOURCE_CANDIDATE --workspace NEW_WORK --out NEW_PROOF --cargo ABS_CARGO --rustc ABS_RUSTC --deadline-utc UTC
python run.py execute --out NEW_PROOF
python analyze.py --out EXTRACTED_PROOF --report NEW_REPORT.json
```

同一boot・同一native toolchain Rust1.97.0、guest16core/32logical、親32logical affinityを要求する。cgroup memory.max=12GiB / swap.max=0 / CPUWeight100、CPU quotaは32以上。fresh targetをarmごとに作り、`--locked --offline --release -j2`、`-C target-cpu=native`、incremental=0、Rayon default=1を固定する。

固定順序は全58 stages。

1. toolchain確認1回。
2. baseline/candidateを各1回fresh release build（各240秒上限）。
3. candidateの `cargo test --locked --offline --release -j2 -p engine -p holdem -p cfr-ref --tests -- --test-threads=1`（240秒上限）。ignored testsは走らない。終了0に加えて、固定fixtureの固有5件の `test NAME ... ok` を各1回要求する。
4. narrow、2 iterations、baseline/candidate×1/32 workersの4 smoke。最初のbaseline/1workerを専用canonicalとし、全state・quality bytesを比較する。
5. baseline/1worker、16 iterationsのnarrow/expanded各1 canonical。CFR wallが各4秒以上でなければ停止。反復数の調整はしない。
6. case外側、round0..3、workers1/16/32、両armで48プロセス。偶数roundはworker昇順かつbaseline→candidate、奇数roundはworker降順かつcandidate→baseline。round0の12本はwarmup、round1..3の36本のみ集計する。

全solveはF32/DCFR固定16iterations（smokeのみ2）、固定2fixture、CFV capture=false、quality targetなし。同一case/Nのstate全bytesとquality JSON全bytesをcanonicalと比較する。CFV、他storage、他fixtureの品質認定に広げない。新fixtureと既存core testsが通る前にsmoke・性能標本を始めない。

## 有限性と証拠

solveは各90秒、RSS監視8GiB、free memory最低2GiB、disk最低2GiB。全体はprepare時点から最大16分の絶対UTC/monotonic期限。呼出し側はVM作成要求時刻から35分後STOP、少なくとも15分回収余裕を確保し、実験期限を短くする。各stage開始前にはそのstageの上限+10秒以上を要求する。途中の失敗・Spot停止・期限切れで終了し、再試行・標本差替え・別boot継続はしない。

fresh workにはCargo中間物と比較用raw canonicalを置き、proofにはsource/binary gzip、pins、raw stdout/stderr/samples、JSON出力、immutable完了receiptを置く。各stageでsource/control/tool/host/bootを再照合する。完全bytes一致とfsyncした保持receiptを確認してから重複rawだけを除去する。N16 canonicalは2本、N2 smoke専用は1本のgzipを全て保持し、readerが展開全bytesのSHA/length/headerを確認する。通常proofは240MiB以内。回収archiveはroot側で256MiB以内、転送枠512MiB。未計測の任意failed rawが必ず枠に入るとは保証しない。

失敗時に30秒以上残れば、失敗したstateを別gzipへlosslessで1回だけ圧縮し、canonical一致とは別のreceiptを保持してからrawを除去する。圧縮・receipt失敗ならrawを残す。途中killでも外側のquiescent回収が可視全filesを保持する。retained manifestの作成失敗は元失敗理由を消さずsecondary errorとし、回収にmanifestの存在を必須としない。strict readerは不足・未完了・pin不一致を `not_evaluable` とし、性能採否を出さない。回収追加分は `r1.fused-update-vm17-recovery/v1` と全original membership/bytesを照合する。

## データを見る前に固定する判定

各case×arm×workerについて測定3本すべて、median、min/maxを出す。CFR、quality7walk、両者の和、construction、state write、whole process、process CPU、各EV/BR/NC walk、whole-process `wait4.ru_maxrss` を分離する。speedup/efficiencyは同じcase/armの1worker median基準。root RSSはLinuxの単一プロセスhigh-water counterであり、工程memoryやchild aggregateではない。

以下を**全て**要求する。ratioはcandidate/baseline。

- 両caseの16worker CFR median ratio <=0.95。
- 両caseの1worker・32worker CFR median ratio <=1.03。
- 両case・全workersのquality7walk median ratio <=1.05。
- 両case・全workersの測定3本のroot RSS **max同士**のratio <=1.10。
- 各case×arm×workerのCFRとquality7walkを別々に、測定3本のmax/min <=1.15。construction/state-write等は記述のみで、このnoise gateに入れない。

guardの一つでも不合格ならperformance screenはrejected。標本欠落ではnot_evaluable。passでも本体採用ではなく、全workspace fmt/clippy/tests、必要なレビュー・契約文書同期は別途採用前に完了させる。guest32logicalは32physical専用coreの証拠ではない。

このディレクトリのPython testsはschedule、source binding、2/N16 header条件、CPU schema、fixture名とguard算術の小さな純テストのみ。ローカルのRust build/solveは行わない。
