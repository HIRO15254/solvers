# 019: 保存SOL3の独立policy評価

旧source03の診断で保存されたFull SOL3をPythonで独立に読み、保存policyのEV・BR・NashConvを初めて計算した。
全12 decisionの**1,326 hand列すべて**を復元し、初期レンジ130/115 handsの互換13,132 pairを積分した。
役評価・カード除去・精算・EV/BRにはproduction Rust、保存i16値、CFR oracle、sorted-rank kernelを使用していない。
これは保存済み診断仮定の独立評価であり、現行productionの新規solve、外部解との同一ゲーム・品質認定ではない。

## 結果

単位はchips（100 chips = 1 bb）、EV・BRは元subgame開始基準。
BRは各hero handについて相手handの期待値を合計してからactionを最大化する。
相手handごとに最大化する、相手の私的情報を知った評価はしていない。

| 評価するpolicy | OOP EV | IP EV | OOP BR | IP BR | NashConv |
|---|---:|---:|---:|---:|---:|
| SOL規範どおり復元したf32値をbinary64で評価 | 1658.488667380247 | 2331.511324762995 | 1658.525762934619 | 2331.557965118803 | 0.083735910180849 |
| 復元値の各列をbinary64で再正規化した別policy | 1658.488667696141 | 2331.511332303859 | 1658.525768046931 | 2331.557970163331 | 0.083738210263164 |

最初の行のseat別gainは0.03709555437239942 / 0.046640355808449385。
復元f32列和の最大誤差は3.725290298461914e-8であり、厳密な実数の確率和1ではない。
その値を変更せず独立binary64算術へ入れた行と、明示的に確率和を正規化した行を分けた。
EV和と診断上の3990 chipsとの差は、それぞれ−7.856758202251513e-6 / −4.547473508864641e-13。
後者は丸めの影響を区別するための別評価であり、元artifactを変更していない。

SOLに入っていた**量子化前live**のEVは1658.4886560610382 / 2331.5113388898817、
NashConvは0.08363443661716019。最初の行との差はEVが+0.000011319208624627208 /
−0.00001412688698110287、NashConvが+0.00010147356368861438 chipsだった。
保存u16量子化・復元と算術経路が異なるため、一致や差の大小を合格条件にはしていない。
既存rawには保存後BRの比較対象値がなく、この結果は独立初取得である。

## 復元と独立算術の境界

- 入力は[既存Full SOL](../../evidence-vm06-river/records/river/diagnostic019/run/solution.sol)、
  10,895 bytes、SHA256 `986af722a0ef15c5d7a5b58715a1c0026ba8dbac99edff6a2986b02e98b24a8b`。
  header/version/config BLAKE3、metadata、directory、各zstd frameの長さとBLAKE3、
  全srefの順序・範囲・重複・末尾bytesを検査した。限定readerは4 MiBを超える入力/sectionを拒否する。
- format定義は[final-source-pins.json](final-source-pins.json)に固定した過去のSOL3 sourceを読むためだけに参照した。
  policyはglobal combo順、`hi*(hi-1)/2+lo`、cardはrank 2..Aとsuit c/d/h/sの`4*rank+suit`。
  保存u16はaction-majorで、hand列ごとに`q / sum(q)`をf32へ丸める。全0列だけ一様fallback。
  metadataの`storage=f32`と、SOL policyのu16量子化は別概念である。
- 公開tree exportのnode順はsref順ではない。全menuを接続し、歴史的storage allocationの
  action preorderでsrefを対応付けた。全12nodeの形と観測menu・保存blockが一致することを検査した。
  [decoded-policy.json](decoded-policy.json)に42,432個のu16と復元f32値を保存した。
  own reachゼロ列を省略する通常のstrategy exportや6桁CSVを入力にしていない。
- 独立役評価は7枚から全21通りの5枚を列挙し、役categoryとkickerのtupleで比較する。
  初期重みは保存configの明示comboをf32へ変換し、その積をカード非衝突pairで条件付ける。
  全nodeで相手reachを伝播し、heroのBRではhero自身のpolicyで枝を消さない。
  root正weightが微小なhandや、そのnodeへのown reachが0のhandも除去しない。
- 精算は独立したFraction算術で`share*(4050+c0+c1-min(0.05*(4050+c0+c1),60))-c_i`。
  全fold/showdownでrakeを取る**診断仮定**であり、外部の精算規則は未確認。
  全21終端・43outcomeを既存[厳密精算表](../payoff-audit-20260927/terminal-table.json)と照合した。
  精算表とSOLのi16値はEV/BR evaluatorへの入力にしていない。
- 期待値はbinary64の乗算と`math.fsum`。solver内部のf32 reach/CFV丸めや、CLIのsaved-profile
  auditでF32Storageへ再投入した場合の二段目f32正規化を再現したとは主張しない。
  外部rake・個別solver version・参照精度は未認定で、NashConv/2による外部Exploitability認定も行わない。

## 実行と保持

[final-audit-command.json](final-audit-command.json)の実行はexit0、評価本体3.783秒、process全体4.135秒。
内部30秒・外部45秒の上限を設けた。新しいsolve、Cargo/build、Cloud実行はない。
最初のdecode試行ではzstd window上限を小さく指定して失敗したため、frame要求量を確認し、
既存SOL3の上限に対応する64 MiBの検査へ修正してから集計した。元入力の変更はない。

[23件の小tests](test_audit.py)は、全9役・wheel・board tie・二組のtrips、カード除去、
相手handを知る不正なBRとの反例、profile確率0のactionを選べるBR、量子化復元、
飽和/非飽和rake、破損拒否、全12nodeのsref対応を確認した。
最終実行はexit0、0.015秒、skipなし。
[最終receipt](final-tests-command.json)と[raw log](final-tests.stderr.log)は測定と同じauditor hashを指す。

数値・各root handの条件付きEV/BRは[final-result.json](final-result.json)、入力と実装のhashは
[final-source-pins.json](final-source-pins.json)、専用環境のwheel URL/hash/versionは[final-dependencies.json](final-dependencies.json)。
zstandard 0.25.0とblake3 1.0.9はrootが準備した専用venvを使用した。
過去のretention manifestにある元pathやpending表記は書き換えていない。

再実行時はGitに保持した[最初のdependency receipt](dependencies.json)を優先し、
installed package versionを独立に検査する。一時`.cache/r1-policy-audit-deps.json`は不要。
[cache読み取り禁止のreplay](replay-proof.json)はexit0、禁止fileへの読取り試行0、
初回・修正後・replayの全数値結果と全policy bytesが一致した（process全体4.419秒）。
[実行receipt](replay-command.json)と[確認script](verify_replay.py)を保持する。

初回source-pinsの`audit.py`は**実行時のpath**であり、現行fileへのpinではない。
同hashの元実装を[audit-initial.py](audit-initial.py)へ保持した。
最初のtest receiptに対応する版も[audit-tests-initial.py](audit-tests-initial.py)へ元hash一致で復元した。
test sourceは全実行で同じ。対応と元/最終/replayの同一policyの保存先は
[content-aliases.json](content-aliases.json)で明示する。
大型policyはbytes一致を確認して`decoded-policy.json`一個へ統合し、元receiptは変更していない。
alias pathが必要な場合は、この保存先のbytesを指定名へ復元できる。

再実行は同じdirectory内の**新しい**出力名を指定する。既存結果を上書きしない。

```powershell
.cache/r1-policy-audit-venv/Scripts/python.exe -B experiments/hu-postflop-r1/reference/HU-R0-019/saved-policy-independent-20260927/audit.py --out experiments/hu-postflop-r1/reference/HU-R0-019/saved-policy-independent-20260927/recheck-result.json --policy-out experiments/hu-postflop-r1/reference/HU-R0-019/saved-policy-independent-20260927/recheck-policy.json --pins-out experiments/hu-postflop-r1/reference/HU-R0-019/saved-policy-independent-20260927/recheck-pins.json --dependencies-out experiments/hu-postflop-r1/reference/HU-R0-019/saved-policy-independent-20260927/recheck-dependencies.json --seconds 30
```
