# P1 `.sol` v4: 木とindexの保存、読込みでの再構築の廃止

2026-10-13。再現状態は`partial`である。大型の`.sol`とbinaryはignoredの`runs/`にあり、hashだけを残す。

## 問いと結論

[前回の改善](../export-rebuild-20261012/README.md)の後も、v3の照会（summary以外）は埋込みconfigから木を再構築していた。c013の1 node exportは5.8秒・2.8 GBだった。利用者決定（2026-10-13）により、次の形式に変えた。

- 読込みでは、export・compare・`inspect --sol`（equityとRiver re-solveを含む）のどれも木を再構築しない。
- 全frameをzstd level 3にし、chunkにはu16のbyte shuffleをかける。
- v3以前は変換せず、再solveを案内するversion errorで拒否する。

| 項目 | 内容 |
|---|---|
| 形式 | header 178 bytes。metadata frameとchunk列の間にindex frameを置く。index frameは木の形（kind・子数・terminal id・deal・mask・transition）、action label、deal card、potを持つ（[規範第7節](../../../docs/hu-postflop.jp.md)） |
| 読込み | `PublicTree::from_shape`は、構築時と同じ規則で子配置・sref・storage範囲を再計算する。不整合な形はerrorにする。hand supportとroot rangeは埋込みconfigから計算する。configの木計数（preflight）も行わない |
| live solve | 構築時のnode情報を`GameIndex`に変え、solve前に捨てる。liveと`.sol`の照会は同じ`TreeView`を使う |

出力は、summaryの`wall_secs`を除き、v3とbyte一致した。

## 条件

- Machine: Windows 10.0.26300、16 CPU、31.9 GB RAM。1回ずつ順に実行した。
- Source: base `78381efd`。この実験と同じcommitで入る。

| binary | sha256 |
|---|---|
| v3 最終（`78381efd`） | `43689b3a7cb90184e40d152367d84b399ee342313b3cca318974c0e26e9d96b9` |
| v4 | `6ff39bab23de5a1d60b962d3348f9c8b9b163490b203d6295789dcd435a5df34` |

- v4の`.sol`は、v3と同じconfigをv4 binaryで再solveして作った（sha256）。
  - c001: `12266a1537086b8985996802864b791dbe77802b02b7d4133176a215d41ac703`
  - c001の`max_iterations`を60にしたもの: `b6f4576a504de1c59ceb7b2b2d05737f854480eca3017b7d12acd6253da44a72`
  - c013: `41d4b8c181bbff2cfde66e2c8e96c3ce4ed4d49574fe3751cc0a231761142377`
- 計測方法は前回と同じで、壁時計とWindows peak working setを20 msごとにpollingした。使ったscriptは次のとおりである。
  - export: [`measure.py`](../sol-index-20261012/measure.py)
  - solve: [`solve_measure.py`](../export-rebuild-20261012/solve_measure.py)
  - 複数node・`--node all`・compare: [`timed.py`](timed.py)
- 生の結果は[`results.jsonl`](results.jsonl)にある。

## 結果

1回のexport（`--node root`）。v3の値は前回の[`exports.jsonl`](../export-rebuild-20261012/exports.jsonl)のlabel `f`から引いた:

| case/view | v3 秒 | v3 peak GB | v4 秒 | v4 peak GB |
|---|---|---|---|---|
| c001 actions | 0.84 | 0.370 | 0.06 | 0.074 |
| c001 strategy | 0.88 | 0.371 | 0.07 | 0.074 |
| c001 ev | 0.84 | 0.370 | 0.06 | 0.074 |
| c013 actions | 5.83 | 2.806 | 0.41 | 0.657 |
| c013 strategy | 5.79 | 2.805 | 0.41 | 0.657 |
| c013 ev | 5.85 | 2.810 | 0.41 | 0.657 |

複数nodeとbulk export、compare:

| 呼出し | v3 秒 | v4 秒 | v4 peak GB | 出力 |
|---|---|---|---|---|
| c013 actions、4 node | 5.84 | 0.41 | 0.657 | 一致 `c03f4a4887d057e4` |
| c013 strategy、4 node | 5.93 | 0.41 | 0.658 | 一致 `3800e8ed125478d2` |
| c013 ev、4 node | 5.82 | 0.43 | 0.657 | 一致 `3c0f4b4dfd4229a1` |
| c001 actions `--node all` | 3.33 | 2.64 | 0.289 | 一致 `52656fd1f807eab1` |
| compare c001 同じconfig | 3.90 | 4.27 | 0.173 | 一致 `202097bc6a8c90fe` |
| compare c001 対 `max_iterations = 60` | 4.51 | 4.21 | 0.233 | 一致 `3ab9c52cadb8b8ba` |
| compare c013 同じfile | 21.73 | 17.83 | 0.786 | 一致 `d9aa4118b8e2b770` |

v3の「c001 同じconfig」は別々にsolveした2 fileの比較だった。v4では同じfileどうしを比べた。どちらも決定的なsolveなので、出力hashは一致する。summaryは0.02秒のままで、`wall_secs`以外は一致した。

c013の1 nodeで残る0.66 GBは、保存した木を展開したものである[INFERENCE: 型sizeから見積もった]。

| 内訳 | 約MB |
|---|---|
| `storage_spans`（24 B/node、solverだけが使う） | 233 |
| `nodes` | 116 |
| `storage_refs` | 85 |
| parent・tag・edge | 各39 |

計画時の目安「0.5 GB未満」には届いていない。viewer用の木から`storage_spans`を外せば届くが、hu-engineの`PublicTree`の契約を変えることになるため、今回は入れていない。

## 保存側とsize

| case | `.sol` v3 MiB | `.sol` v4 MiB | 比 | index frame | 保存区間 秒（v3→v4） | 保存区間の最大working set GB | solve中の最大 GB |
|---|---|---|---|---|---|---|---|
| c001 | 1,314.81 | 1,205.97 | 0.917 | 13,805 B | 4.69 → 7.51 | 4.958 | 4.954 |
| c013 | 3,342.56 | 3,212.73 | 0.961 | 45,232 B | 30.49※ → 20.56 | 12.433 | 12.433 |

※v3のc013は、SOL-14のrunのeventsから取った値である。

- index frameは、c013の970万nodeでも45 KBに収まる。木は同じ形の部分木を繰り返すため、zstdでほぼ消える[INFERENCE]。
- level 3とshuffleで、chunk列はc001で8.3%、c013で3.9%縮んだ。
- 保存区間は、solve中のpeakを上げない（両caseとも差は5 MB以内）。
- c001の保存区間は2.8秒延びた。原因はlevel 3の圧縮速度である（前回の試作ではlevel 1の約1/3）[INFERENCE]。
- solve中のpeakは、c001で5.072から4.954 GBに下がった。live solveが表示用node情報（文字列）を持たなくなったためである。

## 一致と検査

- `--node root`のstdout hashは、c001/c013の全6件でv3の表と一致した。4 node・`--node all`・compareの全hashも一致した。
- v3の`.sol`は`unsupported .sol format version 3 (expected 4); re-solve …`、exit 3で拒否される。
- `inspect --sol`（c001）では`show`・`go`・`combos`・`eq`・`ev`が動く。
- unit testは次を確かめる。
  - `index::tests::stored_index_reproduces_the_builder_tree_and_metadata`: iso統合のFlopとTurnで、index frameを往復させる。木のnode・deal・mask・transition、hand、root range、全action nodeのhistory・label・pot・streetとhistoryからのnode解決、全chance childのlabel（統合deal`X*`とrunout`dealN`を含む）を構築時の値と比べる。切り詰めとnode数の不一致はerrorになる。
  - `from_shape_rebuilds_the_compiled_tree` / `from_shape_rejects_inconsistent_shapes`（hu-engine）。
  - `sol::tests`: v1/2/3/99の拒否、index frameの破損検出、shuffleの往復、level 3のcontextが圧縮予算に収まること。
  - artifactのRiver re-solve（`river_resolve_accuracy`をreleaseで実行、no-riversの往復）。

## 再現

```sh
python ../sol-index-20261012/measure.py SOLVERS c001,c013 LABEL SOLDIR
python ../export-rebuild-20261012/solve_measure.py SOLVERS CONFIG OUTDIR
python timed.py SOLVERS OUTFILE export SOL strategy --format csv --node root --node x
python timed.py SOLVERS OUTFILE compare LEFT RIGHT
```

scriptは`psutil`を使う。
