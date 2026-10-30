# P1 v0.2.0の受入（SOL-58）

計画は[`docs/plans/release-v0.2.jp.md`](../../../docs/plans/release-v0.2.jp.md)。
自動の検査はGitHub Actionsで実行した。ログは各runのページにある（GitHubの保持期間内）。

## 自動の検査

| 検査 | run | commit | 結果 |
|---|---|---|---|
| 受入（先行） `cargo test --workspace --release --no-fail-fast -- --include-ignored` | [40895272030](https://github.com/HIRO15254/solvers/actions/runs/40895272030) | `2e941b43` | 1155 passed、0 failed。下の1件だけ除外した。help・cancelの修正前のcommitで、受入試験の失敗を早く見つけるために実行した |
| 通常CI（PR #9） | [40898424189](https://github.com/HIRO15254/solvers/actions/runs/40898424189) | `abd5629c` | success（test・web・documentation・Windows/macOS smoke） |
| release workflow（手動実行、Releaseは作らない） | [40898430276](https://github.com/HIRO15254/solvers/actions/runs/40898430276) | `abd5629c` | notices jobがfailure。依存に加わった`sha2`系の第三者表記が未生成だった。`packaging/third-party.sh`で再生成した（`ff5206e7`） |
| release workflow（手動実行、Releaseは作らない） | [40900567571](https://github.com/HIRO15254/solvers/actions/runs/40900567571) | `ff5206e7` | version、notices、web、6 buildがすべてsuccess |
| 受入 `cargo test --workspace --release --no-fail-fast -- --include-ignored` | [40904820045](https://github.com/HIRO15254/solvers/actions/runs/40904820045) | `2ba039e5` | 1156 passed、0 failed、除外1件。リリースcommitの製品コードで実行した |
| 通常CI（PR #9） | [40904815789](https://github.com/HIRO15254/solvers/actions/runs/40904815789) | `2ba039e5` | success（test・web・documentation・Windows/macOS smoke） |
| tag `v0.2.0`のrelease workflow（最初のtag） | [40912966213](https://github.com/HIRO15254/solvers/actions/runs/40912966213) | `4fbc5578` | 1回目はApple Siliconのbuild jobが、stepを1つも実行しないまま15分後にcancelledになった。失敗jobだけを再実行した2回目で全jobがsuccessになり、draft Releaseができた。draftの6 archiveを`SHA256SUMS`と照合し、すべて一致した。6 buildのsmokeで`--version`は`solvers 0.2.0 (4fbc5578), heads-up postflop only`だった。下のGUI修正のため、このdraftとtagは公開前に削除した |
| release workflow（手動実行、Releaseは作らない） | [40988597745](https://github.com/HIRO15254/solvers/actions/runs/40988597745) | `a2c9e7e2` | version、notices、web、6 buildがすべてsuccess |
| tag `v0.2.0`のrelease workflow（付け直したtag） | [40990237326](https://github.com/HIRO15254/solvers/actions/runs/40990237326) | `0ac236c4` | 全jobがsuccessで、draft Releaseができた。draftの6 archiveを`SHA256SUMS`と照合し、すべて一致した。6 buildのsmokeで`--version`は`solvers 0.2.0 (0ac236c4), heads-up postflop only`だった。6 archiveの同梱`ui/`はGUI修正を含む |

`ff5206e7`より後のRustのコードは`0ac236c4`まで変わっていない。`a2c9e7e2`はweb（GUI）だけを変えた。
受入40904820045はRustの製品コードを、下のGUI再確認は`a2c9e7e2`のbuildを対象にしている。
その後mainに入った`7cbc129c`はRustとGUIを大きく変えたため、末尾の節で受入をやり直した。

## 除外した試験

`cli`の`nlh_p2`にある`fixed_seed_smoke_resume_and_inspect_lifecycle`（P2）を除外した。
v0.1.0と同じ既知の失敗で、追跡はLinear SOL-47で行う。P2は配布物に含まれない（RD3）。
2026-10-29に利用者が除外を決めた（RV3）。

## GUI受入（手順4、SOL-60）

release workflow `40900567571`のLinux x86-64-v3 archive（`solvers-0.2.0-x86_64-unknown-linux-gnu.tar.gz`）を
AWSのm6i.4xlarge（Ubuntu 24.04、Spot）で展開した。
`solvers --version`は`solvers 0.2.0 (ff5206e7), heads-up postflop only`を表示した。
同梱の`solversd`を`--ui`なしで起動して同梱`ui/`を配信させ、SSH tunnel越しにheadless Chromiumで操作した。

| 操作 | 確認した結果 |
|---|---|
| validate → solve → 結果閲覧 | Open TOMLで、River開始の小さなspot（lock・incentive・頻度目標を各1件）を読み込んだ。検証はvalid（9 node、48.51 MiB）だった。Runsでrunの完了を確かめ、Resultsへ進んだ |
| profileの表示と編集 | Setupのprofileパネルに3件が表示された。頻度目標を0.5から0.4へ変えると、TOMLが`target = 0.4`になった。Resultsでは「target 40.0%・achieved 40.2%」と、lock・incentiveの対象hand数が表示された |
| Lock & re-solve | 3件を待機列に入れた。rootの頻度目標0.3（同種置換あり）、`x`へのJJのlock（check 1）、`x`へのincentive（`toppair`、bet 1、0.1 pot）である。tree全体を再solveした。新runには「Relocked from」の元runが表示され、頻度は目標30.0%・達成30.0%、lockは置換後のJJ 6 handだった |
| Setupでの上流固定 | `x`だけにprofileを置いたconfigで、基準runに最初のrunを選んだ。memory見積りは48.51 MiBから52.90 MiBに変わった。Resultsは「Frozen nodes: 1 · root」を示し、rootの戦略は基準と同じ（check 40.2%）だった |
| `no-rivers`のRiver再solve | Turn開始のspotを`output.solution_streets = "no-rivers"`でsolveした。BB bet・BTN callの後のRiver（Qs）を開くと、「River re-solved on demand」と再計算した戦略が表示された。到達確率が0の線（BTNのcheck 0%）では、再solveできない理由がerrorとして表示された |
| 進捗とcancel | 1,164,212 nodeのflop spot（4.06 GiB、8 thread）の実行中に、iterationとNashConvが更新された。2本目のrunは待機列に入った。待機中のrunをcancelすると、APIとmanifestが`canceled`・`resumable: false`になった。実行中のrunをcancelすると、checkpointを書いて`canceled`・`resumable: true`になった |
| daemonの再起動 | 同じruns rootで`solversd`を再起動した。待機中にcancelしたrunは`canceled`のままで、再投入されなかった（SOL-48） |

受入中に気づいた4点は、公開前に次のように扱った。

| 気づいた点 | 扱い |
|---|---|
| 待機中のrunをcancelしても、Runsの一覧がRefreshまで`queued`のまま見えた | 再現しなかった。draft build（`4fbc5578`）で、API直後の一覧は`canceled`だった（cancel応答11 ms）。GUIでは詳細から開いた場合も、一覧のpollingで現れたrunを選んだ場合も、cancelの約0.5〜1秒後に一覧が`canceled`になった。daemonは一覧を毎回run directoryから読む。修正はしていない |
| 上流固定したrunのResultsで、手番がBB/BTNではなくOOP/IPになった | 原因: GUIは`run.toml`をvalidateして開始状態を得るが、daemonは`profile.freeze_upstream`（host path）を含むconfigを`config-not-self-contained`で拒否する。開始状態が無いため席名がOOP/IPになり、RunsのNashConvも%potでなくBBになった。修正（`a2c9e7e2`）: `freeze_upstream`を除き、記録pathが示す基準run IDを`freezeBaseRunId`として検証する。基準runが無ければ`freeze_upstream`を除いて検証する |
| Freeze upstream欄が生のJSONとhost pathを表示した | 修正（`a2c9e7e2`）: 「Upstream frozen to run <基準run>（link）· SHA-256 先頭12桁」と表示し、host pathは出さない |
| TOML editorの補完が「P1 only」を表示した | 修正（`a2c9e7e2`）: 「heads-up postflop only」にした。`preflop`の説明文の「P1 (HU Postflop)」も直した |

## GUI修正の再確認

release workflow `40988597745`のLinux x86-64-v3 archiveを、同じ種類のAWS Spot（m6i.4xlarge）で展開した。
`solvers --version`は`solvers 0.2.0 (a2c9e7e2), heads-up postflop only`を表示した。同梱`solversd`と同梱`ui/`を使った。
修正前の表示は、同じruns rootをdraft build（`4fbc5578`）で開いて確かめた。

| run | 修正前 | 修正後 |
|---|---|---|
| `frozen`（Setupで基準`base`に上流固定） | 「OOP（OOP） to act」。Runsで`0.00000222 BB`。「Freeze upstream: {"sha256":…,"solution":"/work/rruns/base/solution.sol"}」 | node cardが「BB」、「BB（OOP） to act」、range欄が「BB（OOP）」「BTN（IP）」。Runsで`0.000111% pot`。「Upstream frozen to run base · SHA-256 654e23c7aea5…」。host pathは出ない |
| `base`からfreeze付きでLock & re-solveしたrun | — | 「Relocked from base」「Upstream frozen to run base」「BB（OOP） to act」 |
| `frozen`からfreeze無しでLock & re-solveしたrun（上流固定は`base`のまま） | — | 「Relocked from frozen」「Upstream frozen to run base」。固定の基準はrelock元ではなく記録pathのrunで示す |

TOML editor（Setup → Tree → Script）で`pair`を補完すると、`paired`の候補に「bool · board, heads-up postflop only」が表示された。
webの単体試験は63件passed。追加した3件は、修正前のコードで失敗することを確かめた。

## `7cbc129c`の取り込みと再受入

draft作成後、mainに`7cbc129c`（chip額面`table.bb_chips`・`table.chip_rounding`、`_chips` key、
P1履歴のstreet内raise-to表記、`.sol` 7〜9、`solvers upgrade`）が直接入った。v0.2.0はこれを含めて作り直した。
2つのreviewで見つけた点と扱いは次のとおり。修正は`6d95d59b`（PR #11）。

| 見つけた点 | 扱い |
|---|---|
| CHANGELOGが旧形式（`.sol` 4〜6、0.001 BB grid）のままで、chip・履歴・形式の変更が無い | 互換節、Added（chip額面）、Changed（履歴表記・`.sol`形式・checkpoint・引分けのchip分配・Preflopの`.mwsol` 5）を書いた |
| `solvers upgrade`はv0.1.0の`.sol`を変換できない（削除済みkeyを埋め込むため`NLH002`）。profile付きの5・6は古い履歴を残す | 利用者決定RV4で`upgrade`を削除した。`.sol` 1〜6は再solveを案内するerrorになる |
| 同じ行動を`r2.5`と`r2500c`の2通りに書け、実効configとhashが変わる | 利用者決定RV5: 両表記と混在を認め、実効configで卓の正規表記へ書き換える。node selectorも両表記で同じnodeを選ぶ |
| 整数でない`bb_chips`の大会設定で`solvers validate`（人向け表示）がpanicする | 修正し、`bb_chips = 3`の大会の試験を加えた |
| `cap_chips`で書いたrake capが`cap=none`と表示される。header `amounts=BB` | chip表記を表示するよう直した |
| 旧checkpointのresumeがhash不一致の理由を示さない | 旧buildのcheckpointは再solveが必要と案内する |
| 空の`stacks_bb`と`stacks_chips`の同時指定が排他にならない、GUIのRuns表示が`stack_chips`を読まない、help・文書・コメントの旧表記 | すべて直した |

検査は次のとおり。

| 検査 | run | commit | 結果 |
|---|---|---|---|
| 通常CI（push） | [41017662632](https://github.com/HIRO15254/solvers/actions/runs/41017662632) | `7cbc129c` | success |
| 受入 | [41018237853](https://github.com/HIRO15254/solvers/actions/runs/41018237853) | `7cbc129c` | success。除外1件 |
| release workflow（手動実行、Releaseは作らない） | [41018259997](https://github.com/HIRO15254/solvers/actions/runs/41018259997) | `7cbc129c` | success |
| 開発機（AWS m6i.4xlarge）の`cargo fmt --check`・clippy（既定とP1専用）・`cargo test --workspace`・P1専用`cli`の試験・web build・web試験 | — | `6d95d59b`の作業tree | すべてpass。workspace 1147 passed・39 ignored、P1専用`cli` 123 passed、web 65 passed |
| 通常CI（PR #11） | [41023991515](https://github.com/HIRO15254/solvers/actions/runs/41023991515) | `6d95d59b` | success |
| 受入 | [41023989717](https://github.com/HIRO15254/solvers/actions/runs/41023989717) | `6d95d59b` | 1185 passed、0 failed、除外1件 |
| release workflow（手動実行、Releaseは作らない） | [41023993226](https://github.com/HIRO15254/solvers/actions/runs/41023993226) | `6d95d59b` | version、notices、web、6 buildがすべてsuccess |

GUIは、release workflow `41023993226`のLinux x86-64-v3 archiveを新しいAWS Spot（m6i.4xlarge）で展開して再受入した。
`solvers --version`は`solvers 0.2.0 (6d95d59b), heads-up postflop only`、`solvers upgrade`は`unrecognized subcommand`だった。

| 操作 | 確認した結果 |
|---|---|
| chip額面の卓（`bb_chips = 3`、`sb_chips = 1`、`stack_chips = 60`） | lineを`BTN r2, … BTN b9c, …`と書いた。`solvers validate`とGUIのreplayは`BTN r6c, … BTN b9c, …`と正規表記で示し、`r6c`と書いたconfigと`validate --format json`の出力がbyte単位で一致した。Setupに「Chips per BB」と「Chip rounding」が表示された。solve後、Runsの経済条件は`60c · No rake`、Resultsのactionは`bet 15c` |
| chip卓のLock & re-solve（上流固定） | BTNのcall/foldにlock（fold 0.8）を置き、「Freeze upstream to this solution」で再solveした。node欄は`r15c`、Resultsは「lock: r15c · 1 nodes · 42 hands」「Frozen nodes: 1 · root」「Upstream frozen to run …」 |
| validate → solve → 結果閲覧・profile | 以前と同じRiver開始のspot（lock・incentive・頻度目標）で、検証valid（9 node、48.51 MiB）、頻度目標50.0%に対し達成49.9% |
| Setupでの上流固定 | 上のrunを基準に選ぶと見積りが52.90 MiBになり、Resultsに「Upstream frozen to run …-2 · SHA-256 2debde811c83…」が出た |
| `no-rivers`のRiver再solveと履歴表記 | Turnで BB check・BTN bet 1・BB call の後、River Ahを選ぶと「River re-solved on demand」と戦略が出た。RiverのBB betは`bet 1`、node欄は`xr1c[Ah]r1`で、street内のraise-to額になった |
| 進捗とcancel | 1,164,212 nodeのflop spotを2本投入し、待機中の2本目をcancelすると`canceled`になった。実行中の1本目はcancel後にcheckpoint（iteration 100）を書いて`canceled`になった。Resumeすると続きから再開し、iteration 150まで進んだ |
