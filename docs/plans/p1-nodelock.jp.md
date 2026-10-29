# P1 Nodelock・相手profile計画

作成: 2026-10-28。[製品定義](../products.jp.md)のD8に基づく、P1（NLH HU Postflop Solver）への
nodelockと相手profileの追加計画である。既存solverの調査は[調査](../research/2026-10-28-nodelock-survey.jp.md)、
担当・状態は[Linear](../status.jp.md)に置く。本書は目標・決定・設計方針・段階の完了条件を定める。
公開契約は各段階で[共通Input規範](../nlh-input-v1.jp.md)・[P1規範](../hu-postflop.jp.md)・
[CLI規範](../cli-reference.jp.md)へ移し、本書の草案は規範を上書きしない。

## 1. 目的と範囲

利用者が相手（または自分）の偏りを指定し、それに対する最適な応答と、偏った側の最善の残り戦略を
同じ厳密vector CFRで求める。次の3つを利用者に届ける。

1. **実rangeのnodelock**: 指定nodeで、指定comboの戦略を固定する。
2. **hand category別のnodelock**: 「Top pairのcheckを50%」のように、hand categoryとactionで固定する。
3. **相手profile**: hand category・actionの価値を実際より高く（低く）見積もる偏りを与える。

対象外: P2・multiway Postflop、ML近似、他variant（D4のまま）。nodeに届くrange自体の固定（上流戦略への制約）は
nodelockとして扱わず、上流nodeのlockで表す。Restricted Nash Responseのような根元で型を選ぶ全体のソフト化は扱わない。

## 2. 利用者決定（2026-10-28）

| ID | 決定 |
|---|---|
| NL1 | D4を改訂し、P1に限りNodelockと相手profileを対象に戻す（製品定義D8） |
| NL2 | incentive（action価値への加点）は利得への加点として扱い、上流の判断にも伝わる。上流を動かしたくない場合は「上流の固定」を別optionで指定する |
| NL3 | 「Top pairのcheck 50%」は、combo単位（各comboが50%）のlockと、集計頻度（category全体で50%、どのcomboかはsolverが選ぶ）の目標の両方を提供する。集計頻度は既定でrunoutごとに数える |
| NL4 | ソフトlock（確率pでlock戦略、1−pで自由に最適化）を入れる |
| NL5 | 配布v0.1.0（D7）には含めない。後の配布で含める |
| NL6 | （2026-10-29）上流固定の部分木solveは自動で使う（PF12、[資源効率計画](p1-efficiency.jp.md)）。relockのwarm startは採らない |
| NL7 | （2026-10-29）N0〜N5の後に残った制限を解除する: GUIのLock & re-solveでincentive・頻度目標、GUIのSetupからdaemon管理下のrunを基準にした上流固定、頻度目標と上流固定の部分木solve、suit非対称selectorと上流固定、no-riversとRiver上のprofile。あわせて上流固定runのmemory見積りを実測に合わせる |

## 3. 設計方針

以下の実装済みの既定・計算意味は確定し、[P1規範](../hu-postflop.jp.md#4-計算停止storage)と
[共通Input規範](../nlh-input-v1.jp.md)を正本とする。未実装の方針は引き続き草案である。
利用者決定と矛盾する変更は新たな利用者決定を要する。

### 3.1 3つの仕組み

利用者向けの機能は、エンジンの3つの仕組みと、その上の指定方法（第3.2節）の組合せで実現する。

| 仕組み | 意味 | 利用者向け機能 |
|---|---|---|
| lock | nodeの指定handについて、戦略（全action、または一部action）を固定する。ソフトlockは混合比pを持つ | 1、2のcombo単位 |
| incentive | 指定（node、hand、action）の利得に加点する | 3 |
| 頻度目標 | 指定（node、hand集合、action）の集計頻度が目標に近づくよう、incentiveの加点を反復中に調整する | 2の集計頻度 |

**lock**

- 全体lock: 指定handの戦略を固定し、regretと平均戦略の更新を省く。平均戦略は固定値を返す。
- 部分lock（action単位）: 固定actionの確率を除いた残りを、固定していないactionの中でregret matchingして配る。
  瞬時regretは固定していないaction集合の値を基準にする（regret matchingは全actionへの定数加算で結果が変わるため）。
- 固定しないhandは通常どおり最適化する（各社の部分lockと同じ意味）。
- ソフトlock: nodeの戦略を `p·σ_lock + (1−p)·σ_free` とする。`σ_free`はそのnodeの通常のregret matchingで、
  regretは同じaction価値から計算する。戦略累積は自由成分を保存し、平均戦略の読取り時に混合する（[規範](../hu-postflop.jp.md#4-計算停止storage)）。
- 入力の戦略は確率の合計が1（部分lockでは1以下）であることを要求し、黙って正規化しない。

**incentive**

- 加点は `β·pot_I`（`pot_I`はnode Iのpot、βは比率）を既定の単位とし、BB額の指定も受ける。
- vector CFRでは、対象handのaction価値の行へ `加点 × m(h)` を足す。`m(h)`はhandと両立する相手reachの質量で、
  既存の`compat_sums`で求める。加点は利得の一部として親へ返す（NL2）。
- 表示・保存するEVには加点を含めない（GTO Wizardのvirtual bountyと同じ扱い）。
- incentiveがあるゲームは一般和である。零和のexploitabilityとは呼ばず、第4節の指標で収束を確かめる。

**頻度目標**

- 頻度はexportの`actions`と同じく、node到達weightで加重した選択頻度とする（[P1規範](../hu-postflop.jp.md)第8節）。
- 集計の単位は既定でrunoutごと（NL3）。iso併合した代表nodeは、その類の各runoutを表す。
  runoutを束ねた集計は別optionとし、需要を確認してから実装する。
- 目標ごとに加点λを持ち、評価の区切りで現在戦略の達成頻度との差に応じてλを更新する（双対上昇）。
  更新規則と報告値は[規範](../hu-postflop.jp.md#4-計算停止storage)で確定。

### 3.2 指定方法

- node: 既存のhistory selector（`node_by_history`）に、runoutを省略して全runoutへ一括適用する記法を加える。
  特定のrunoutの指定も受ける。
- hand: comboのrange表記、またはhand category式（例: `toppair & !flushdraw`）。
  category分類器は`nlh`に新設し、役（made hand）とdrawの2軸を持つ。定義は規範に明記する。
- action: nodeのaction label。
- 上流の固定: 基準解（`.sol`）を指定し、対象nodeより上流の戦略をその平均戦略で固定する。
- `.sol`からの取込み: 既存解の戦略を読み、編集してlock戦略にできる。
- 無効な指定（存在しないhistory、support外のcombo、合計が1を超える確率、同じ対象へのlockと頻度目標の重複）は
  入力errorとし、黙って無視しない。

### 3.3 suit同型の併合

hand category式とrunoutの省略はsuit置換に対して対称であり、現行の併合をそのまま使える。
特定のcard・suitに依存する指定（例: A♠K♠だけのlock、特定turnだけのlock）では、併合に使う置換を
その指定を保つものに絞る（rangeを保つ置換に絞る現行の仕組みと同じ扱い）。

### 3.4 成果物と互換

- lock・incentive・頻度目標・基準解のidentityを実効config、checkpointの互換性hash、`.sol`のmetadataに記録する。
- これらが異なるcheckpointのresumeは拒否する。
- `compare`は条件の違いを明示する。
- lock・incentiveの無いrunは、`.sol` payload・checkpointが変更前とbit一致する（f32・f64とも）。

## 4. 評価と報告

| 指標 | 定義 |
|---|---|
| lock下の`g_i` | 各seatの最適応答利得。lockを持つseatの最適応答は、lockした（node、hand）では固定戦略に従い、それ以外でだけ逸脱する。ソフトlockでは `p·σ_lock·v + (1−p)·max_a v_a` |
| incentive下の`g_i` | incentiveを持つseatは加点込みの利得、相手は真の利得で測る |
| EV | 加点を含まない真の値 |
| 頻度目標 | 目標ごとの達成頻度、目標との差、最終λ |

lockだけのrakeなしchipEVは零和のままであり、`NashConv / 2`をexploitabilityとして報告できる。
incentiveを含むrunは一般和の報告規則（[製品定義](../products.jp.md)第4節「品質」）に従う。

## 5. 段階と完了条件

| 段階 | 内容 | 完了条件 |
|---|---|---|
| N0 | 決定の記録（本書、製品定義D8、調査、Linear） | 製品定義・AGENTS・文書索引・Linearが本書と一致する |
| N1 | エンジン: lock（全体・部分・ソフト）、incentive、lock下のBR、EVから加点を除く処理。CFR・EV・BR・profile評価・batchの全経路 | lock・incentiveの無いrunがbit一致。toyゲームで、片側を全nodeでlockするともう片側の値が`br_pass`と一致する。Nash解でlockすると解が変わらない。部分lockとソフトlockの不変条件。incentive付きの修正ゲームを総当たりで解いた結果と一致する |
| N2 | 指定方法と入出力: history＋runout省略、combo range、`.sol`取込み、上流の固定、suit同型、成果物のidentity・resume拒否・compare、規範・CLI規範・利用者guide・template | runout省略の指定がrunout列挙と一致する。suit非対称なlockが併合なしの結果と一致する。lock変更後のresumeを拒否する。AGENTSの同期範囲を満たす |
| N3 | hand category分類器とcategory式（lock・incentiveの両方） | 分類がsuit置換で不変で、各分類軸で全comboを分割する。式で指定した結果がcomboを列挙した結果と一致する |
| N4 | 頻度目標（runoutごと） | toyゲームと小さなRiver木で、目標との差が許容内に収まる。EV loss順にcomboを動かす素朴な方式と、lock下の`g_i`を比べた結果を`experiments/`に記録する |
| N5 | GUI/daemon: lock・incentive・頻度目標の表示、その後に編集 | 対象画面で表示と操作を確認する |
| N6 | NL7の制限解除 | 各制限の拒否・fallbackが無くなり、規範・CLI reference・利用者guide・testから制限の記述が消える。頻度目標の部分木solveが全木solveと一致する。suit非対称selectorの上流固定が同じselectorで解いた基準解の結果と収束誤差内で一致する。no-riversの再solveがlock・固定nodeを守る。GUIの2機能を実daemonで確認する |

N1→N2→N3の順に依存する。N4はN1・N3、N5はN2に依存する。配布物への組込みは別の配布計画で扱う（NL5）。

N6の結果（2026-10-29）: GUIのLock & re-solveはlock・incentive・頻度目標を積んで再solveし、Setupはdaemonが管理するrunを基準解に選べる
（clientはhost pathでなくrun IDを送る）。両方を実daemonとbrowserで確認した。頻度目標は部分木solveで更新し、強制全木との頻度・λの差は`1e-5`以内。
suit非対称selectorは基準解の手順をprofile木へ写して上流固定する。基準解のgroupingがprofile木より細かく代表が一意に決まらない場合の拒否は残す
（[P1規範](../hu-postflop.jp.md)）。`no-rivers`の`.sol`はRiverの制約（lock・incentive・頻度目標の最終λ・固定戦略）を保存し、
遅延再solveに適用する（`.sol` v6）。上流固定runの見積りと時間は[資源効率計画](p1-efficiency.jp.md)のC8に記録した。

## 6. 制約

- `crates/cfr-ref`は凍結のまま変更しない。N1の検証はtoyゲームの総当たりで行う。
- AGPL・無ライセンスの実装（postflop-solver、TexasSolver）のコードを読まない・移植しない。公開APIの意味の参照に留める。
- GTO Wizardの解・値はlocalかつgit-ignoredに置き、commitしない。
- 計算は`hu-engine`・`hu-postflop`に置き、`cli`は引数・run directory・表示だけを持つ（[architecture.md](../architecture.md)第2節）。
