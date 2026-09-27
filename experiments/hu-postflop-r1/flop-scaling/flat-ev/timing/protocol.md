# EV / flat-EV のローカル比較手順

全street Flopの既存narrow/expanded入力を使い、現行EV修正を共通基準として
flat出力だけの差を比較する。これは固定反復で同一profileを得る場合の局所比較であり、
外部解との品質認定、収束目標への到達、32 vCPUでの比例高速化を認定する手順ではない。
ローカル機は8 physical cores /16 logical processors。16 workersを16 physical coresとは扱わない。

## 開始条件

両armで同じadapterを新規linkし、engine/game/holdemの由来を各native buildへ結び付ける。
adapterは既存probeからworker/反復の許容範囲とusageだけを変更する。fixture、DCFR、
状態出力、公開EV/BR、phase timerは同一。計測binaryにallocator計装を含めない。
候補の短いnative全状態照合と、F32/I16・0/可変次元・専用poolの検査が先に成功していること。

各stageは512MiB Job commit、Below Normal、wall 120秒以下、開始直前のavailable commitと
physical memoryが各1.5GiB以上、disk reserve 2GiB。rootのRSS・Job commitを別に保持する。
全stageを順番に実行し、他のbuild/solveを重ねない。上限到達・品質不一致で停止し、自動再試行しない。
ローカルの背景負荷を前後に記録する。全coreを独占した測定とは主張しない。

## Baselineだけのpilot

入力ごとに1 workerのbaselineを16反復から試す。CFR時間が4秒未満なら32/64/128へ順に倍増し、
最初に4秒以上になった反復数をその入力の全比較へ固定する。128でも未達なら128で固定し、
短時間測定として扱う。候補の時間を見て反復数を選ばない。pilotは本測定の標本に含めない。
pilot最終状態はその反復数のcanonicalとし、以後すべてのstate bytesとquality JSONを照合する。

## 固定matrixと判断

入力ごとに1/2/4/8/16 workers、baseline/flatの10条件を実行する。
round0はwarmupで集計から除外。round1〜3を測定とし、worker順を交互に正順/逆順、
各workerのarm順をbaseline→flat / flat→baselineと交互にする。入力順はnarrow→expanded。
同じ入力の全条件で反復数、全状態と公開品質bitsが同一であることを要求する。
matrix全体の実行期限は開始から20分。失敗・未実行を速い標本として数えない。

CFR、7回の品質walk、両者合計、build・state出力・process全体を分けて報告する。
strong scalingはarmごとにT(1)/T(p)、efficiencyはその値/p。中央値に加え全3標本と幅を示す。
root OS peak working setは各process全工程の観測値で、要求allocation bytesやJob commitとは別物。

局所的な採用候補とする条件は、全状態・品質一致、1 workerのCFR+quality中央値の悪化が5%以内、
各条件のroot OS peak最大値がbaseline最大値の110%以内、両入力の8 workersでCFR+quality中央値が
10%以上短縮すること。いずれかの測定条件でCFR+qualityの3標本の最大/最小が1.15を越えれば、時間判断は保留する。
16 workersを含む全結果を残し、速かった設定だけを報告しない。採用には本体への適用後の通常検証も必要。
このguardを通過してもR1全体の受入・32 vCPU性能・他入力の性能を認定しない。
