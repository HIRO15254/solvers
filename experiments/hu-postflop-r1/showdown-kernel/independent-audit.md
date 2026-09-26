# Measurement02 独立監査

保持済みproofをportable verifierで再検証し、別の標準ライブラリコードでもraw bytesから再計算した。
今回の記述的な採用guardに対する阻害事項は見つからなかった。詳細なhash、phase値、test集計は
[independent-audit.json](independent-audit.json)、元の検証結果は[verification.json](verification.json)に保持する。
solver、build、VM操作は行っていない。

- 347 original path／190個の一意なpayloadを全件SHA256照合。source archiveはold 361、新362 fileのexact setと全bytesを確認。
- 新旧buildと全32実行はboot `ac2eeb5d-385c-4ea1-95b6-1b545ce65a85`で一致。
  guestの報告はIntel Xeon、2 physical core／4 logical CPU、実験は1 worker。
- 各caseのwarmupを含む8本で、`canonical.bin`／`state.bin`の原bytes、全quality値とEV/BR/NashConv bits、
  global hand IDs、counts/tree metadata、normalized config、algorithm/rake/utilityが一致。
- 新sourceのworkspaceは933 pass／0 fail／31 ignored。44 test targetと12 doctestの計56 summaryをrawログから集計。
  追加のrelease oracle 3件とriver resolve 1件も成功。6個の新kernel testの成功行を確認した。
  oldは同bootのbuild-onlyであり、oldのfull workspace検証を今回行ったとは扱わない。

## 時間の再計算

warmupを除く3組の中央値。全caseで新実装が3/3組とも短かった。

| case | 反復数 | old run 秒 | new run 秒 | new/old | old build ms | new build ms |
|---|---:|---:|---:|---:|---:|---:|
| River | 1000 | 8.763098226 | 6.285461408 | 0.717265 | 0.758365 | 0.764304 |
| Turn | 1000 | 1.731361753 | 1.292089334 | 0.746285 | 2.219723 | 2.329269 |
| Flop | 50 | 0.454137315 | 0.406425719 | 0.894940 | 26.504802 | 26.473606 |
| narrow River | 10000 | 0.092674434 | 0.065148919 | 0.702987 | 0.865544 | 0.852991 |

run比の幾何平均は **0.7617831605803252**。事前の`<=0.95`と各case `<=1.05`を満たす。
build、prepare、solver初期化はrun timerの外で、JSONに各中央値を分けた。
例えばTurnのbuild中央値は新実装の方が約0.110 ms長い。solveだけの改善を全phaseの改善へ広げない。
Flopとnarrow Riverは1秒未満であり、3組は信頼区間を与えない。全4caseを事前どおり集計した限定的な結果である。

## メモリ値の解釈

raw samplesを再計算しても、native `wait4.ru_maxrss`は**全32本で52,080,640 bytes**だった。
一方、sampled full-process tree RSSの測定中央値は約5–15 MBで変化する。両者は異なる指標であり、
native値の一致からsolverのメモリ同等・削減・非悪化を主張できない。sampled値もpeakの取り逃しがあり、solve専用ではない。

起動元Pythonのメモリがnative値の床になる可能性は一次実装と整合する。
このsupervisorは`Popen(start_new_session=True, close_fds=True)`から起動し、`wait4`の値を保持する。
upstream [CPython 3.12.3 subprocess](https://raw.githubusercontent.com/python/cpython/v3.12.3/Lib/subprocess.py)と
[_posixsubprocess](https://raw.githubusercontent.com/python/cpython/v3.12.3/Modules/_posixsubprocess.c)では、
この呼出条件は`posix_spawn`経路を使わず、条件が許せば`vfork`、失敗時は`fork`へ進む。
[Linux fork実装](https://raw.githubusercontent.com/torvalds/linux/v7.0/kernel/fork.c)では`vfork`が親のmmを共有し、
通常のforkは複製したmmのhigh-waterを現在RSSで初期化する。
[exec実装](https://raw.githubusercontent.com/torvalds/linux/v7.0/fs/exec.c)は旧mmのhigh-waterを保存するため、
exec前の大きなPython imageの影響がexec後のnative peakに残り得る。

実VMの記録はPython 3.12.3、`Linux-7.0.0-1011-gcp`だが、distroの全patch、実際のsyscall trace、
起動時の親RSSは保持されていない。したがって今回の値の原因を特定したものではなく、**メモリ改善は認定しない**。
source上の10,608Bの一時配列除去と観測RSSは区別する。

F32の4fixture・1bootの結果であり、I16、tiny-weight相殺の修正、外部参照との一致、R1全体の受入は認定しない。
source-afterも記録されたlive再hashであり、独立した実行後filesystem snapshotではない。
