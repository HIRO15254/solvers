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

受入中に気づいた点で、今回は直していないもの:

- 待機中のrunをcancelしても、Runsの一覧はRefreshを押すまで`queued`のまま表示される。APIとmanifestは直後から`canceled`である。
- 上流固定したrunのResultsでは、node cardの手番表示がBB/BTNではなくOOP/IPになる。
- Freeze upstream欄は生のJSONとhost pathを表示する（既知の制限）。
- TOML editorの補完は、board変数に開発用の呼び名「P1 only」を表示する。
