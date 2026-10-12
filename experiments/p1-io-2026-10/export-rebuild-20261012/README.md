# P1 artifact読込みの木再構築・bulk export・compareの短縮

2026-10-12。[SOL-14](../sol-index-20261012/README.md)の後続改善の証拠。再現状態は`partial`である。大型の`.sol`とbinaryはignoredの`runs/`にあり、hashだけを残す。

## 問いと結論

SOL-14後のv3 exportでは、c013の1 node照会に38.6秒かかった。その時間の大半は`.sol`ではなく、木の再構築にかかっていた。内訳は一時的なprobe（commitしていない）で測った。

| 区間（c013、`strategy --node root`） | 累積秒 |
|---|---|
| `.sol`を開く（metadataとdirectory） | 0.001 |
| `prepare`（parse・lower・**memory preflightの木計数**） | 13.46 |
| 木の構築（直列） | 32.71 |
| 表示用node情報 | 37.0 |
| 行の作成 | 37.09 |
| 解放まで | 37.8 |

変更:

| 項目 | 変更 | 判断 |
|---|---|---|
| 木の構築 | 最初のchance nodeのdealをRayonで並列に構築する。forkのmask・transition・rank table・terminal・node tagを、直列walkの初出順で再採番する。memory preflightの計数も同じ単位で並列化した | 採用。sref・node idは直列構築と一致する |
| artifactの読込み | 具体storageを持つ埋込みconfigはmemory preflightを省く（`prepare_artifact`）。`auto`だけは従来の`prepare`で解決する | 採用 |
| 表示用node情報 | nodeごとに並列化 | 採用 |
| node索引（`.sol` v4） | history→sref・label・子の索引を保存すれば、再構築を避けられる | 不採用。上の3項目でc013は5.8秒になった。形式変更と全artifactの再solveに見合わない |
| bulk export | reachを読込み時のparent配列から辿る（従来は照会ごとにO(nodes)のparent配列を作っていた）。`SolStore`のchunk展開をlockの外へ出し、nodeごとの行を並列に作る | 採用 |
| compare | 埋込みconfigのhashが一致すれば木を共有する。nodeごとの差を並列に計算し、node順に集計する | 採用 |
| `.sol`のsize | u16のbyte shuffleやzstd levelを変える | 不採用（下記）。形式変更に見合う縮小が無い |

出力はすべて変更前とbyte一致した。

## 条件

- Machine: Windows 10.0.26300、16 CPU、31.9 GB RAM。1回ずつ順に実行した。
- Source: base `73124e78`（SOL-14）。最終binaryはこの実験と同じcommitで入る。

| binary | sha256 |
|---|---|
| SOL-14 v3（`73124e78`） | `5745bc3c0956a9a263cd80abf1922dcc2a8907d5e2c037134dd84c05c20eb71f` |
| 最終 | `43689b3a7cb90184e40d152367d84b399ee342313b3cca318974c0e26e9d96b9` |

- 対象: SOL-14のv3 `.sol`（c001 `1fdb7541…`、c013 `64dab04e…`）。
  - compareの対には次の2つを加えた。どちらも最終binaryと同じ変更を持つbuildで再solveしたものである。
    - c001を同じconfigで再solveした`.sol`（`07cc6e9f20ca5cefb0bed608f46a036e3a9a4f7465461550696ccca4497a621b`）。
    - c001の`max_iterations`を60にしたconfigの`.sol`（`5df7fe70c22f19f17544e39b7d635791e50fe3eeaa2ab7147b3efefcd4d77439`）。
- 計測: SOL-14の[`measure.py`](../sol-index-20261012/measure.py)を使った。他は同じ方式（壁時計とWindows peak working set、20 ms polling）で1回ずつ測った。生の結果は[`exports.jsonl`](exports.jsonl)（label `f`）にある。

## 結果

1回のexport（`--node root`）。SOL-14の値は[`exports.jsonl`](../sol-index-20261012/exports.jsonl)のlabel `b`・`b13`から引いた:

| case/view | SOL-14 秒 | SOL-14 peak GB | 最終 秒 | 最終 peak GB |
|---|---|---|---|---|
| c001 actions | 3.23 | 0.326 | 0.84 | 0.370 |
| c001 strategy | 3.26 | 0.327 | 0.88 | 0.371 |
| c001 ev | 3.19 | 0.326 | 0.84 | 0.370 |
| c013 actions | 38.80 | 2.761 | 5.83 | 2.806 |
| c013 strategy | 38.42 | 2.761 | 5.79 | 2.805 |
| c013 ev | 38.58 | 2.761 | 5.85 | 2.810 |

summaryは0.02〜0.03秒のままである。`solvers validate`（c013）も13.5秒から1.8秒になった。

複数nodeとbulk export:

| 呼出し | SOL-14 秒 | 最終 秒 | 最終 peak GB | 出力 |
|---|---|---|---|---|
| c013 actions、4 node | 38.61 | 5.84 | 2.808 | 一致 `c03f4a4887d057e4` |
| c013 strategy、4 node | 38.67 | 5.93 | 2.808 | 一致 `3800e8ed125478d2` |
| c013 ev、4 node | 38.54 | 5.82 | 2.806 | 一致 `3c0f4b4dfd4229a1` |
| c001 actions `--node all` | 837.6※ | 3.33 | 0.461 | 一致 `52656fd1f807eab1` |

※並列構築だけを入れた途中buildの値（木の構築は0.73秒）。SOL-14 binaryでは測っていない。そこからparent配列の再利用だけで16.1秒になり、行の並列化で3.3秒になった。

compare:

| 対 | SOL-14 秒 | SOL-14 peak GB | 最終 秒 | 最終 peak GB | 出力 |
|---|---|---|---|---|---|
| c001 同じconfigの別solve（木を共有） | 19.03 | 0.53 | 3.90 | 0.37 | 一致 `202097bc6a8c90fe` |
| c001 対 `max_iterations = 60`（木を2回構築） | 18.97 | 0.53 | 4.51 | 0.61 | 一致 `3ab9c52cadb8b8ba` |
| c013 同じfile（木を共有） | 123.88 | 4.64 | 21.73 | 2.81 | 一致 `d9aa4118b8e2b770` |

2つ目の対は差が0でない（平均L1 0.111、最大EV差200.5）。この対で、並列集計の和と最大nodeが直列と一致することを確かめた。

## 保存側（v3）

[`solve_measure.py`](solve_measure.py)は、solve全体のpeakと、`stop`から`completed`までの区間を測る。この区間には最終checkpointと`.sol`の書込みが入る。

| case | 区間 秒 | 区間の最大working set GB | solve中の最大 GB | peak GB | `.sol` |
|---|---|---|---|---|---|
| c001（最終binary、再solve） | 4.69 | 5.085 | 5.072 | 5.089 | 1,314.81 MB |
| c001（SOL-14のrunのevents） | 17.71 | — | — | — | 1,314.81 MB |
| c013（SOL-14のrunのevents） | 30.49 | — | — | — | 3,342.56 MB |

保存はsolve中のpeakを上げない。writerはこの変更で変えていない。c001の17.7秒と4.7秒の差は、SOL-14のrun時の実行条件によるものとみる[INFERENCE]。再solveした`.sol`は、metadataのwall時間を除いて元と同じ内容である（compareで全node差0、EVのbitも一致）。

## `.sol` sizeの試作

[`size_proto.py`](size_proto.py)は、c001の`.sol`のchunkを8個に1個抜き出し（2,001個中、展開263 MB）、別の設定で圧縮し直す。結果は[`size-proto.json`](size-proto.json)にある。shuffleは、u16の下位byteと上位byteを別の面に分ける。速度はPythonの単一threadの値で、shuffleの時間を含む。

| 設定 | 現行比size | 圧縮 MB/s | 展開 MB/s |
|---|---|---|---|
| zstd 1（現行） | 1.000 | 300 | 766 |
| zstd 3 | 0.986 | 151 | 698 |
| zstd 1 + shuffle | 0.941 | 147 | 759 |
| zstd 3 + shuffle | 0.918 | 100 | 691 |
| zstd 9 + shuffle | 0.897 | 44 | 729 |
| zstd 19 + shuffle | 0.840 | 5.6 | 548 |

展開後のsizeは格納sizeの1.52倍しかない。u16の戦略と値は、ほぼ乱数に近い。採用できそうな設定（level 3以下）では5〜8%しか縮まず、形式v4と全artifactの再solveに見合わない。大きく縮めるには量子化の精度（u16）を下げる必要があり、これは製品の精度判断になる。

## 残る制約

`export strategy|ev --node all`は全行をメモリに集めてから符号化する。c001の`strategy --node all`は18.5 GBの確保に失敗した。この制約は変更前から同じで、行の総量によって決まる。解消するには、行を流しながら符号化する出力にする必要がある。

## 一致の検査

- `--node root`のstdout hashは、SOL-14の表（actions・strategy・ev）とc001/c013の全6件で一致した。
- unit test `parallel_build_matches_serial_build`は、3 board（Flop・Turn、mergeを含む）で次を比べる。
  - 直列構築と並列構築の木、tag、mask、transition、node情報、rule hit、terminal、rank table。
  - 直列計数と並列計数のmemory見積り。

## 再現

```sh
python ../sol-index-20261012/measure.py SOLVERS c001,c013 LABEL SOLDIR
python solve_measure.py SOLVERS CONFIG OUTDIR
python size_proto.py SOL 8
```

scriptは`psutil`、`numpy`、`zstandard`を使う。
