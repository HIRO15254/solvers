# 配布計画: P1 v0.2.0のGitHub Release

更新: **2026-10-30**。[製品定義](../products.jp.md)のD8（Nodelock・相手profileは後の配布で含める）と
[Nodelock計画](p1-nodelock.jp.md)のNL5を受け、v0.1.0の後の変更を2回目の配布にする実行計画である。
作業状態・担当・待ち関係は[Linear](../status.jp.md)（親SOL-58）に置き、本書へ転記しない。
配布形態・対象OS・CPU基準・署名・P2の除外は[v0.1.0の配布計画](release-v0.1.jp.md)のRD1〜RD7を引き継ぐ。

## 1. 範囲

v0.1.0（tag `v0.1.0`）から配布するmainまでの変更をすべて含める。内容は`CHANGELOG.md`の`[Unreleased]`を正本とし、主なものは次である。

- P1のNodelock・相手profile（lock・incentive・頻度目標・`freeze_upstream`・relock、GUIでの表示と編集。N0〜N6）
- tree-scriptの`jam`・`merge`と、構造的なall-in設定（`tree.allin_threshold`、`[tree.max_aggressive_actions]`、
  `tree.preflop_reraise_jam_above_stack`、`tree.include_allin`）の削除（入力の非互換）
- `no-rivers`の未保存Riverの遅延再solve（CLI・daemon・GUI）、`decision`・`--rest-of-street`
- GUIのCodeMirror editorとTree previewの刷新
- P1の速度・memoryの改善とmemory見積りの較正
- chip額面（`table.bb_chips`・`table.chip_rounding`・`_chips` key）と、P1の履歴・labelのstreet内raise-to表記（`7cbc129c`。`.sol` version 7〜9）
- 配布版の既知の欠陥の修正（手順2・3）

## 2. 利用者決定（2026-10-29、30）

| ID | 決定 |
|---|---|
| RV1 | versionは`0.2.0`。入力の削除と成果物形式の変更を含むため、0.xのminorを上げる |
| RV2 | GUIのブラッシュアップは含めない。手順4は現行のGUIで受け入れる |
| RV3 | P2は配布物に含めない（RD3）。`nlh_p2`受入試験の既知の失敗（`fixed_seed_smoke_resume_and_inspect_lifecycle`、SOL-47）はv0.1.0と同じく受入から除外し、受入記録に明記する |

最初のdraft作成後、mainに`7cbc129c`（chip額面と履歴表記の変更）が入った。これを含めてv0.2.0とし、レビューを受けて次を決めた（2026-10-30）。

| ID | 決定 |
|---|---|
| RV4 | `solvers upgrade`（旧`.sol` 4〜6の変換）は削除する。v0.1.0のsolutionは削除済みkeyを埋め込むため変換できず、profile付きの5・6は履歴とselectorを安全に書き換えられない。旧`.sol`と旧checkpointは再solveを案内する |
| RV5 | lineの額はBBと`Nc`の両表記を受け付け、混在も認める。node selectorも両表記を受け付ける。互換性hashは実効configの文字列から作るため、treeが同じでもscriptの書き方が違えば変わる。同じtreeかどうかは上流固定の基準解の照合（board・pot・stack・tree・label・range）で確かめる。実効configの表記はRV10、表示はRV11で定める |
| RV6 | 利用者がBBで書いた額（table・economicsの額、lineの額、node selectorの額、size literalのBB）と既定のSB（0.5 BB）は、どの卓でも`table.chip_rounding`で整数chipへ丸める。丸めで額が変わったときは`validate`がwarningで場所と丸めた後のchip数を示す |

続けて、額の単位の書き方を作り直すことにした（2026-10-30）。

| ID | 決定 |
|---|---|
| RV7 | 単位の作り直しを済ませてからv0.2.0を配布する。`_chips` keyと`Nc`は0.2.0で入るため、配布後に消すと利用者の移行が2回になる |
| RV8 | 1つの量に1つのkeyとし、単位は値に書く。数値と接尾辞なしはBB、`"Nbb"`はBB、`"Nc"`はchip。対象は`stack`・`[table.stacks]`・`sb`・`ante`・`bb_ante`・`straddles`・`outside_field`・`cap`・`rounding_unit`。旧`*_bb`・`*_chips` keyは`NLH002`とし、新しいkey名を示す |
| RV9 | tree scriptのsize literalも接尾辞なしはBBとする。pot比は`N%`と書く。`Nbb`・`Nc`・`Nx`・`Ne`・`N%stack`・`N%effective`・`a`・`min`は維持する。v0.1.0の`bet [50]`は意味が変わるため、移行表と削除keyの`NLH002`の案内で`50`→`50%`を示す |
| RV10 | 実効config（`run.toml`・`--print-config`・`.sol`とcheckpointに埋め込むconfig）の額は、どの卓でも`"Nc"`と`Nc`のlineで書く。scriptは書いたまま保存する |
| RV11 | 人が見る表記（node履歴、action label、GUIとCLIの表示）は卓ごとの規則を維持する。`bb_chips`の素因数が2と5だけの卓はBBの10進数、それ以外は`Nc`。lineの額と同じく、selectorとlock keyはどちらの表記でも書け、RV6で丸めて照合する |

## 3. 手順と完了条件

| 手順 | Linear | 内容 | 完了条件 |
|---|---|---|---|
| 1 方針の記録 | SOL-59 | 本計画、製品定義の到達段階、status対応表 | `tools/check_docs.py`が通り、3文書の記述が一致する |
| 2 配布版のhelp | SOL-49 | P1専用buildの`--help`と各commandのhelpから、使えないcommand・flag（`derive`・`evaluate`、Preflop専用flag）と開発用の呼び名を除く。`resume`の説明にP1の`.ckpt`を含める | P1専用buildのhelpに使えないcommand・flagと開発用語が出ないことを確かめる試験がある。既定buildはすべてのcommand・flagを維持する |
| 3 待機中runのcancel | SOL-48 | daemonで待機中のrunをcancelしたとき、run directoryへcanceledを保存する | 待機中runをcancelしてdaemonを再起動しても再投入されない試験がある |
| 4 GUI受入 | SOL-60 | P1専用buildのdaemonとGUIで、validate → solve → 進捗 → 結果閲覧、profile（lock・incentive・頻度目標）の編集、Lock & re-solve、Setupでの基準runによる上流固定、`no-rivers`のRiver再solveを確かめる | 配布するGUIでの各操作をブラウザで確認した証拠がある |
| 5 version・互換 | SOL-61 | workspace versionを上げる。CHANGELOGの互換節に、v0.2.0が読む成果物（P1 `.sol` 7〜9、checkpoint 5・6・8。v0.1.0の成果物は再solve）と、`solvers.nlh/v1`の入力から削除したkeyの扱いを書く。`[Unreleased]`を版と日付にする | `--version`とtagが一致し、CHANGELOGからリリースノート案ができる |
| 6 利用者文書 | SOL-62 | README（英語・日本語）と利用者wiki（日英ページ、相互link）に、v0.2.0のリリースノート、相手profile・relockの使い方、入力の移行方法を書く。wikiには開発用の呼び名を出さない（AGENTS.md） | 開発環境の無いマシンで文書だけを使ってprofile付きのsolveとGUI閲覧ができる。wikiに開発用IDが無い |
| 7 受入と公開 | SOL-63 | リリースcommitで通常CIと`Long-running acceptance`（`cargo test --workspace --release -- --include-ignored`）を実行し、`v0.2.0` tagをpushしてdraft Releaseを作る。公開は利用者が行う | Releaseの成果物（3 OS・checksum・第三者表記・リリースノート）がそろい、受入の証拠が`experiments/`に記録されている |

依存関係: 2・3・5は1の後に互いに独立に進められる。4は2・3の後（配布するbinaryで確かめる）、
6は4と5の後、7はすべての後。
