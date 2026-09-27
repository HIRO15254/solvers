# Flop CPU software sampling の固定測定契約

本campaignは現行baselineのCPUをどのコードが使うかを調べる研究診断である。production、CFR、terminal、storage、既定並列化を変更しない。候補の採否、32 workerの速度保証、収束品質、外部参照合格は判定しない。[未実行時の設計根拠](proposal.jp.md)と[adapter](adapter/README.jp.md)を参照する。

## 入力と品質の固定

- fixtureは既存narrow/expandedの2種類、同じ全street public tree。F32/DCFR、alpha1.5/beta0/gamma3、pow4 reset、chance depth2/min children12、固定64 iterations。品質targetなし。
- 同一sourceから追加研究example `flop_cpu_profile_probe` を1回だけfresh release buildする。元sourceと追加adapterの全file set、Path.parts順のarchive、package、toolchain、実binary bytesを固定する。
- `-C target-cpu=x86-64-v3 -C force-frame-pointers=yes -C debuginfo=line-tables-only`、offline/locked、Cargo `-j2`。profile環境overrides/wrappersを除去する。元のnative buildやframe-pointer無しの時間とは比較しない。
- 2CPU build bootで既存engine/holdem/cfr-refのrelease `--tests -- --test-threads=1` を実行する。候補はなく、full workspace検証を実行したとはしない。root-action/fused-updateなどの研究testは追加しない。
- 同一instanceを32logical/16guest physicalへresizeした別bootで、全条件が同じbinaryを使う。v3 usable判定、CPU affinity/cgroup、boot ID、source/tool/controlを前後検査する。

## 順序と有限性

最初に各caseの1worker canonicalを64iterations、各120秒で計2本作る。続いてnarrow、expandedの順に、それぞれround0は16→32worker、round1は32→16worker、各90秒で計8本を実行する。warmupやpilot、Nの選び直し、自動retryはない。全10solveのstate全bytesとquality JSON全bytesを同case canonicalへ照合し、canonical2本をgzip保持する。alias stateは完全比較・耐久receiptの後だけ除去する。失敗stateは可能な場合にlosslessgzip保持し、成功と扱わない。

E2-only、disk20GiB、作成要求時刻から45分の絶対STOP（秒への切捨てのみ許可）。build期限は作成要求+20分以内。measurement dispatchは作成要求+15分以内かつ残windowが10分超、測定window15分以内、測定終端はSTOP−15分以前。開始条件を満たさなければ回収へ移り、期限やVM種別を変更しない。20GiBで不足しても追加disk/VMへ移らない。料金予約・resource作成・停止削除は別cloud controlが担当し、本runnerは操作しない。

外側build unitは6GiB/swap0、measurement unitは12GiB/swap0、CPUWeight100。子の上限はbuild4GiB、measurement8GiB、free memory/disk reserve各2GiB。既存supervisorがprocess groupを所有し、grace0.2秒/kill5秒/poll0.1秒で後始末する。build/testは各480秒。各stage開始時は固定timeout+10秒の余白を要求する。全体UTCとmonotonic deadlineの双方を検査する。任意補助perf読取りは各15〜20秒以内、rawを保持して不成功時は停止する。

## perfの開始条件

Cargo buildの前と、32CPU bootのcanonicalの前に、同じ固定preflightを各1回実行する。依存fetchはbuildより前の取得作業として許す。installed `perf --version`、`record -h`、`script -h` 原文を保持する。次のflagsとsoftware eventが利用できなければ停止し、PMU/event/rate/callgraph/sysctlを自動変更しない。

```text
perf record -e cpu-clock -F 97 --strict-freq --clockid mono \
  --call-graph fp,32 --no-buildid-cache --max-size 16M -o perf.data -- PROBE ...
```

1秒busyのPython子で、実software sampling、strict text grammar、sample census、evlistのfreq97/use_clockid1/clockid1/CALLCHAIN、子のCLOCK_MONOTONIC窓内10sample以上を確認する。preflight全体60秒。API privilege不足も測定未評価として保存する。fake perf fixtureを使って本番可用性を認定しない。

各profileの `perf.data` は16MiB以内、8本合計128MiB以内。script/dump textはそれぞれ各64MiB以内の一時上限とし、両者とも解析成功後にgzip元bytes照合を行って圧縮保持する。解析失敗時はplain原本を残す。全proof上限240MiB（回収controlは別枠32MiB）。上限到達/取得異常は診断未評価で停止し、原データを削除して続けない。

## 時計と集計

adapterの `phases.json` はCLOCK_MONOTONIC nsでCFR、state_write、quality、EV各席、BR各席、exploitabilityの `[start_ns,end_ns)` を示す。phase stdout到着時刻や相対Instantから窓を推定しない。perfの絶対mono clockを実preflightで照合し、CFRは同pidのsampleだけを窓でclipする。sample streamがCFRの前後を含むことも確認する。qualityは独立7walkの従来public API値を使い、profile窓とは別に保存する。

scriptの全sample数とdumpのSAMPLE record数が一致することを要求する。未認識のrecord出力、空CFR sample、LOST/LOST_SAMPLES/THROTTLE/UNTHROTTLE、script警告は診断未評価。raw dumpを残し、LOST record件数を失われたsample数と呼ばない。unknown symbolを除外せず、leaf別sample件数、unknown leaf/any frame、TID数、period合計・leaf別period合計、callchain長上限到達数を記録する。inclusive caller shareを加算しない。fp32上限到達はtruncation可能性であり、短いstackでも正確とは限らない。

software CPU samplingはon-CPUの位置を観測する。spin、allocator、kernel、terminal、reduction等の仮説選別に使えるが、off-CPU待ち時間、cache miss、帯域飽和の量は測らない。サンプル比は経過時間比ではなく、異なるframe-pointer外部libraryのunwind欠落も残る。2roundだけの比率やCPU/wallからSMTのみが原因と断定しない。

## 保持と読取り

`plan.json` → `build-execution.json` / immutable `build.json` → `measurement.json` → `execution.json` / `retained.json` を保持する。全raw stdout/stderr/samples、help、script/dump/evlist、perf.data、source archive、binarygzip、canonical stategzip、JSON、before/after identitiesを含む。失敗はcompletedへ変換せずsuffixをskippedにする。

`analyze.py` はtrusted checkoutからだけhelperをimportし、保持されたcodeを実行しない。archiveの元source全bytes、binary、canonical stream header+全SHA、alias比較receipt、quality全bytes、stage順序/command/host/deadlineを検査する。raw perf自体はSHAで保持照合し、サンプル集計は保持されたscriptとdumpから再計算する（portable readerでperfを再実行したとの主張はしない）。完全に一致しても `performance_screen=not_applicable` / `production_adoption=false` である。

一次資料: [Linux perf-record](https://raw.githubusercontent.com/torvalds/linux/master/tools/perf/Documentation/perf-record.txt) のfreq/strict-freq/clockid/fp/max-sizeと、[rustc codegen options](https://doc.rust-lang.org/rustc/codegen-options/index.html) のframe pointers/line tablesを参照した。実hostのhelpとsoftware preflightを優先し、documentにあるだけで利用可能とは認定しない。
