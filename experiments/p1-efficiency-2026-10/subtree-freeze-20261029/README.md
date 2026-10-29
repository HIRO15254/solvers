# freeze_upstream 部分木の実験（AWS、2026-10-29）

再現状態: **verified**（基準`.sol`はGit外のローカル保存物が必要）。

## 変更

`freeze_upstream`で固定された厳密な祖先を通る到達を一度計算し、他のprofile対象の子孫ではない対象nodeを部分木の入口とする。実験用`p1_bench --subtree-only`は両playerの各CFR passをその入口からのみ走らせる。compiled tree、evaluator、storageは全木のままである。入口を横断して並列化せず、各部分木内の既存並列処理を使う。公開config・CLI・成果物の契約は変更しない。頻度controllerは実験経路で拒否する。

## 方法

基準は`flop_srp`を0.1% potまで解いたFull `.sol`。profileはBB check後のBTN、flop check-check後の全turnのBB、同じlineのAc turnのみの3種。[configs](configs/)に入力と断片、[setup.sh](setup.sh)に`relock --freeze-upstream`によるconfig生成、[measure.sh](measure.sh)に計測、[correctness.sh](correctness.sh)に一致比較を記録する。

bitcheckの基準revisionは`00483eaf`、測定した試作は`1530db00`。基準`.sol`は試作の途中状態の通常CLI経路で生成しており、`00483eaf` binaryそのものによる生成ではない。Rust 1.97.0、`-C target-cpu=znver3`のrelease build。labはr6a.8xlarge、benchはc6a.8xlarge、いずれもEPYC 7R13のSpot。instance・AZ・binaryと基準`.sol`のSHA-256は[manifest.json](manifest.json)、基準solveの記録は[base.txt](results/base.txt)。基準`.sol`は1,111.91 MBでGitに含めず、checksumを照合して`C:/Users/PC_User/.solvers-remote/artifacts/subtree-freeze-20261029/solution.sol`にローカル保存した。別環境への配布はされていない。

部分木NashConvは、各入口で両playerの固定到達と制約付きEV・BRを使い、入口までのchance重みを掛けて基準gameのnormalizerで割ったBR利得の和である。対象外での逸脱を含まないため全木NashConvではない。停止判定はこの和の半分が元の5.5 BB potの0.1%以下、すなわち部分木NashConvが0.011 BB以下で、10反復ごとに評価する。

反復回数・停止値の判定は共有labで[convergence.sh](convergence.sh)を使用。benchでは[measure.sh](measure.sh)で16・32 thread、warmup 2・20反復・3評価を3組、全木と部分木を交互に走らせる。[fixed-time.sh](fixed-time.sh)はlabで得た停止反復回数を固定してbenchで3組交互に走らせ、停止runと同じ10反復ごとの部分木評価を含む時間を測る。labの停止判定回数をbenchへ移した固定仕事量の時間であり、benchで独立に停止反復を探索した値ではない。

## 結果

bench c6a.8xlargeでの3組の中央値。評価は全木のEV/BRと部分木のEV/BRを比較し、peakは両modeとも最後に全木評価も行ったprocessのVmHWM。全木storageを確保する試作なのでpeak削減をコンパクト実装の効果と誤認しない。

| 対象 | thread | 反復 秒 全木 → 部分木 | 比 | 評価 秒 全木 → 部分木 | 比 | peak MiB 全木 → 部分木 | 比 |
|---|---:|---|---:|---|---:|---|---:|
| lock_flop | 16 | 0.17780 → 0.11658 | 0.656 | 0.24404 → 0.15770 | 0.646 | 4113 → 4112 | 1.000 |
| lock_flop | 32 | 0.15840 → 0.10438 | 0.659 | 0.21847 → 0.14321 | 0.656 | 4259 → 4272 | 1.003 |
| lock_turn | 16 | 0.17360 → 0.05979 | 0.344 | 0.23373 → 0.07670 | 0.328 | 4134 → 4056 | 0.981 |
| lock_turn | 32 | 0.15508 → 0.05930 | 0.382 | 0.20887 → 0.07799 | 0.373 | 4303 → 4136 | 0.961 |
| lock_deep | 16 | 0.17766 → 0.001193 | 0.00672 | 0.24462 → 0.001635 | 0.00669 | 4112 → 4027 | 0.979 |
| lock_deep | 32 | 0.15887 → 0.001068 | 0.00672 | 0.21985 → 0.001677 | 0.00763 | 4264 → 4085 | 0.958 |

部分木0.1% pot停止の反復数はlabで全木・部分木とも一致。次はその反復数を固定したbenchのsolve区間（10反復ごとの評価を含み、build・終了時の全木評価を除く）3組の中央値。反復数と最終部分木NashConvがlab記録と一致したことを集計時に確認した。

| 対象 | thread | 反復数 | 全木 → 部分木 秒 | 比 |
|---|---:|---:|---|---:|
| lock_flop | 16 | 370 | 70.384 → 47.851 | 0.680 |
| lock_flop | 32 | 370 | 63.127 → 43.160 | 0.684 |
| lock_turn | 16 | 90 | 16.233 → 6.039 | 0.372 |
| lock_turn | 32 | 90 | 14.674 → 5.918 | 0.403 |
| lock_deep | 16 | 10 | 1.728 → 0.0145 | 0.0084 |
| lock_deep | 32 | 10 | 1.555 → 0.0139 | 0.0089 |

全測定の[生JSON](results/subtree-results/)と[固定仕事量](results/subtree-fixed-time/)、[集計](results/summary.json)を保存する。


全木storageは3,975,682,320 bytes（3791.51 MiB）。選択spanのみをf32のregretとstrategyに確保した場合の理論値は次のとおりで、試作自体はこれを確保削減しない。

| 対象 | 最大入口数 | span storage bytes | 全木比 | span MiB |
|---|---:|---:|---:|---:|
| lock_flop | 1 | 2,592,216,960 | 0.6520 | 2472.13 |
| lock_turn | 49 | 1,208,751,600 | 0.3040 | 1152.76 |
| lock_deep | 1 | 24,668,400 | 0.00620 | 23.53 |

labの16 threadで部分木停止後、対象外の平均戦略を基準から読み戻して全木評価した（[生JSON](results/subtree-combined/)）。lock_flopは370反復、部分木NashConv 0.0109407 BB、全木 0.0110104 BB。lock_turnは90反復、部分木 0.0101354、全木 0.0177842 BB。lock_deepは10反復、部分木 0.0041550、全木 0.0136810 BB。全木値は現行profileの制約付きBRで、対象外の基準解の残差を含む。labは共有機なのでこのrunの時間はbench性能として扱わない。
同じ部分木停止量を使った全木走査も、16・32 threadともそれぞれ370・90・10反復で停止し、部分木NashConvも一致した（[lab記録](results/subtree-convergence/)）。
全木と部分木の差は順に約0.000070・0.007649・0.009526 BB。局所停止が達成しても全木の残差は別に残る。


## 判断

**利用者判断が必要。** この3条件では対象が深いほど走査は大きく速くなるが、この試作はstorageを削減せず、通常成果物やresumeには組み込まない。部分木停止を全木停止と同一視できないため、既定の自動適用はまだしない。有効化方式・保存値・停止量の選択は下記のとおり。

## 正しさ

実木の3条件を50・200反復ずつ全木走査と比較し、各最大部分木のregret・strategy-sumのf32 bit列のBLAKE3と、各入口の両席EV・BRが一致した（[出力](results/correctness.txt)、[生JSON](results/subtree-correctness/)）。小さい木の試験はchance transitionを含む複数入口で同じ比較を行う。通常経路はflop_3bpとflop_srp＋freezeで、f32 storage・16 thread・20反復についてf32/f64 CFRの両方のbitcheckが一致した（[results](results/)）。devで`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`が通過した。


## 製品化の提案と利用者に必要な判断

- **有効化:** `freeze_upstream`で自動実行するか、明示opt-inにするか。停止量と成果物の意味が変わるため、既定自動化には契約変更が必要。
- **部分木外の成果物:** 基準戦略をコピーし、祖先を含む全nodeの値を合成戦略で再計算するか。基準の値をそのまま残せば変更後の下流と整合しない。`.sol`の保存streetと、基準に未保存のnodeをどう扱うかも必要。
- **停止と表示:** 対象部分木内に限定した逸脱利得か、部分木外の基準解の残差を含む全木NashConvか。前者は局所的な達成を示し、全木の0.1% potを保証しない。表示と停止の対象を明確に分けるか決める。
- **checkpoint/resume:** 最大入口集合・入口到達・基準`.sol`のSHA-256・profile・storage layout・停止量の定義をidentityに含め、変更時は拒否する。コンパクトstorageを導入するなら旧checkpointからの扱いも決める。
- **メモリ:** この試作は全木storageを確保する。部分木spanのみのarena化にはstorage refとspan、checkpoint、評価・export、見積りを一緒に変更する必要がある。tree、evaluator、基準読み込みやscratchは別途残る。
- **GUI/daemon relock:** 局所か全木か、基準解の残差、停止量、再開互換性をAPIと画面で区別する。既存のrelockと外部pathの制約を維持する。
- **複数entry・同型:** 祖先対象を含む場合は最大対象のみを入口にし、重複部分木を二重に更新しない。頻度目標は更新時刻・複数入口間のcontrollerを揃える実装が別途必要。selectorでsuit同型が変わって基準木と一致しない場合は現行どおり拒否し、一般化にはreachと戦略のpermutation対応が必要。
