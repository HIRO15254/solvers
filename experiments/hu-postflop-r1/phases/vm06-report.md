# VM06: source03 phase 計測の独立検証

2026-09-25 UTC の source03 計測 on/off 各18件を、元の source03 比較18件と照合した。**区間・出力同等性の検証は pass**。source06 の最終性能比較は別データであり、この表の対象ではない。

全2,224 raw payload、compact の正確な file 集合と hash、360 supervisor stage（各campaign 120）、18 phase record を独立に再検証した。全 stage は child/supervisor exit 0、cleanup 完了、記録された実行前後 identity 一致、stdout/stderr/samples hash 一致。未保存の `/usr/bin/python3.12` 自体は再ハッシュできず、全記録で同じ実行前後 identity という観測のみを採用する。

同じ case/version/repetition の18組で live EV/BR・NashConv・iteration・設定、保存後 tree/strategy/EV の全ノード export bytes が original/off/on 間で一致した。ここでの BR は保存前 live profile の値である。保存後量子化 profile の BR 再評価および外部 GTO 条件認定は、この phase campaign では行っていない。

## 時間校正

単位は秒。各値は3反復の中央値。original は無計測 binary、off は instrumentation を組み込んだ binary で記録を無効化、on は記録を有効化した別campaign。

| case | version | original | off | on | on/off | off/original |
|---|---|---:|---:|---:|---:|---:|
| river | baseline | 0.196240546 | 0.198107882 | 0.258961850 | 1.307176 | 1.009516 |
| river | candidate | 0.194388856 | 0.207069451 | 0.259383090 | 1.252638 | 1.065233 |
| turn | baseline | 2.095555404 | 1.904456045 | 2.170412705 | 1.139650 | 0.908807 |
| turn | candidate | 1.974848824 | 1.716451542 | 2.193780364 | 1.278090 | 0.869156 |
| flop | baseline | 12.225236040 | 10.388432502 | 11.419379429 | 1.099240 | 0.849753 |
| flop | candidate | 10.149984351 | 9.885615642 | 9.589961151 | 0.970092 | 0.973954 |

mode は別campaignで順次実行され、cache・実行順序・OS scheduling を統制していない。candidate Flop の on/off が約0.970であることも、差を instrumentation cost 単独と解釈できないことを示す。0.05秒間隔の supervisor 観測、起動・終了、初回 phase JSON の fsync、最終 phase JSON 書込みも外側の時間に関係する。計測 on の baseline/candidate 比を主性能受入値として使用しない。

## 区間の内訳

単位は秒、on の3反復中央値（原値の整数 ns は JSON に保持）。各 invocation の10 leaf は連続・非負・非重複で、合計が total と厳密一致する。**個々の中央値の和は total の中央値とは限らない**。

| leaf | River base | River cand | Turn base | Turn cand | Flop base | Flop cand |
|---|---:|---:|---:|---:|---:|---:|
| input_preparation | 0.009691580 | 0.012696790 | 0.009997730 | 0.010514900 | 0.012469269 | 0.009341520 |
| initialization | 0.001690280 | 0.001573110 | 0.008032240 | 0.007567420 | 0.101238235 | 0.097613126 |
| cfr_updates | 0.099275135 | 0.104405296 | 1.589507107 | 1.729940332 | 6.105964019 | 6.345184140 |
| periodic_ev_br | 0.004997610 | 0.006442460 | 0.079946896 | 0.095589264 | 0.795323840 | 0.839791547 |
| checkpoint | 0.052879677 | 0.047903836 | 0.273793120 | 0.170603305 | 2.111015057 | 0.601074898 |
| final_ev_br | 0.002618449 | 0.000618540 | 0.032733018 | 0.008016910 | 0.299868548 | 0.059405328 |
| summary_publish | 0.000019260 | 0.000013230 | 0.000035570 | 0.000060100 | 0.000046180 | 0.000035449 |
| sol_preparation | 0.001798580 | 0.002010250 | 0.058851357 | 0.055695258 | 1.390266785 | 1.172931685 |
| sol_serialization_and_write | 0.007572799 | 0.007909949 | 0.022898549 | 0.024964920 | 0.292740249 | 0.295200768 |
| overhead | 0.021417311 | 0.021266650 | 0.023739258 | 0.020903038 | 0.087175546 | 0.065641026 |
| total | 0.203972942 | 0.207200962 | 2.082726939 | 2.167254905 | 11.379767810 | 9.514995914 |
| CFR inclusive（加算禁止） | 0.133144455 | 0.150588954 | 1.807507019 | 1.909290465 | 8.147171019 | 7.507427764 |

`cfr_updates` は CFR 呼出しだけの leaf。CFR inclusive は最初の update 開始から最後の update 終了までで、中間の periodic EV/BR・checkpoint・overhead を含む別区間である。最後の update 後の評価・checkpoint は含まない。inclusive は他の leaf と重なり、total に足し込まない。

`input_preparation` は設定読込み・正規化からPostflop初期化手前、`initialization` は dry run・game構築・solver 初期化、`periodic_ev_br` は定期評価、`checkpoint` は各 checkpoint 書込み、`final_ev_br` は最終評価結果の取得、`summary_publish` は完了表示と RunSummary の構築・呼出元への復帰、`sol_preparation` は保存用profile/value準備、`sol_serialization_and_write` はシリアライズとSOL書込みを測る。`overhead` はそのほかの main_impl 内区間で、起動時の初回 phase JSON の同期書込みも含む。final JSON の同期書込みとプロセス起動/終了は内部 total の外側。main_impl 全体を wall clock で測り、CPU時間ではない。

River/Turn は CFR・定期評価各4回、Flop は各5回。baseline checkpoint は5/5/6回、candidate は4/4/5回であり、最終重複checkpointの除去が呼出順に現れている。両版とも final_ev_br は1区間だが、candidate の区間はキャッシュした最終値の取得を含み、同じ計算量の1回を意味しない。Flop の checkpoint 中央値は2.111015057→0.601074898秒、final_ev_br は0.299868548→0.059405328秒。CFR leaf 自体は6.105964019→6.345184140秒で、この計測から CFR 単体の高速化は主張しない。

## メモリの観測

単位はMiB、3反復中央値。root は Linux wait4 の high-water RSS、tree は /proc の瞬間RSS合計をサンプルした最大値。いずれも invocation 全体の値で、phase別メモリではない。

| case | version | root original | root off | root on | sampled tree on |
|---|---|---:|---:|---:|---:|
| river | baseline | 24.722656 | 24.410156 | 24.359375 | 10.496094 |
| river | candidate | 24.726562 | 24.429688 | 24.359375 | 10.382812 |
| turn | baseline | 48.835938 | 48.730469 | 49.035156 | 49.542969 |
| turn | candidate | 35.171875 | 34.839844 | 35.019531 | 35.390625 |
| flop | baseline | 697.632812 | 696.886719 | 696.792969 | 697.261719 |
| flop | candidate | 428.761719 | 423.171875 | 426.746094 | 423.800781 |

Turn/Flop の candidate では全modeで root peak が低い観測になった。どの区間がメモリ減少を生んだかは、この wall-time instrumentation からは分からない。River は root peak 約24MiBに対して実行中 sampled tree は約10MiBで、rootの fork/pre-exec 継承や観測範囲の影響を無視できない。wait4 は回収済み子孫の high-water を含む場合があり、同時tree peakとは異なる。sampling は短いpeakを取り逃がし、共有ページを重複計上する可能性がある。root と tree を置換・混合して性能比を算出しない。

## 出所と再検証

- phase raw bundle: `runs/r1-cloud/vm06-phases.tar.gz`、SHA256 `eef21251eec733ee51b2bf6ab79c0776df1fc5b40a0b837c77a00f29ac42749d`、1,461 payload。
- 元比較 raw bundle: `runs/r1-cloud/vm06-v3-results.tar.gz`、SHA256 `3c679810a39b3fac58d394f95387b0d98cec94ccc299f325e5922eefb9264e2d`、763 payload。
- candidate source03 archive: SHA256 `ac5d493a129cd97be9322598867fa3ab5795ba6d3241a93e2719beaa2dd89970`。baseline は `9632d8b244990cb5b95ff7d0cacc84ee95a7ee0e`。
- boot `159efb96-10fb-4ce4-bb0f-bc2b27ee618f`、AMD EPYC 7B12、8 logical CPUs。phase build 前後・original/off/on のboot一致を確認。過去のIntel bootの値は採用しない。
- phase/source manifests の before input hash を raw source archives と再照合。変更はCLIの3ファイルと追加instrumentation moduleに限定。freezeの設定・runner・supervisor・source manifest・binary identityとstage入力が一致。
- compact は phase 1,383ファイル、original 723ファイル（いずれもmetadataを含む）。SOL/CKPT・大きなexport・binaryはraw bundle内で再ハッシュした。元comparisonのphase_timingsはnullのままで、新規phase recordはonのsolve18件だけ。originalへの注入、off/export/resumeのphase生成はない。

汎用 retention の ready:false は保持する。この限定検証は cross-bundle のsource/config/run依存を補って上記範囲を検証するもので、retention全体の未解決参照を一律に解消したとは扱わない。選定compactは本変更のGit保持対象で、raw bundleは別途ローカル保管が必要。

```text
python experiments/hu-postflop-r1/phases/vm06-phase-verification.py --self-test
python experiments/hu-postflop-r1/phases/vm06-phase-verification.py --out experiments/hu-postflop-r1/phases/vm06-phase-report.json
```

[機械可読の全反復・中央値・hash・検証境界](vm06-phase-report.json)、[再検証script](vm06-phase-verification.py)。負例7件（欠測・未知leaf・重複区間・inclusive加算・不完了・source混同・export変更）を拒否する軽量self-testも実行済み。Cargo/GCP/solver再実行はこの検証では行っていない。
