# P1 v0.1.0の受入（SOL-46）

この節は、公開前に実行した検査と、その結果を記録する。
いずれもGitHub Actionsで実行した。ログは各runのページにある（GitHubの保持期間内）。

| 検査 | run | commit | 結果 |
|---|---|---|---|
| release workflow（手動実行、Releaseは作らない） | [38689090848](https://github.com/HIRO15254/solvers/actions/runs/38689090848) | `2ced5412` | 次のjobがすべてsuccess: version、notices（windows-latest）、web、6 build。各buildで`--version`、`validate`・`solve`・`export`（`river_small`）、P2入力の拒否（exit 2）を確かめた |
| 受入 `cargo test --workspace --release --no-fail-fast -- --include-ignored` | [38695419244](https://github.com/HIRO15254/solvers/actions/runs/38695419244) | `2f52c234` | 1071 passed、0 failed。下の1件だけ除外した |
| 通常CI | 各pushのCI（最後は[38835595264](https://github.com/HIRO15254/solvers/actions/runs/38835595264)） | `2f52c234`まで、受入後の修正を含む | success |
| release workflow（Node 24 actions、日本語README同梱の後） | [38835599694](https://github.com/HIRO15254/solvers/actions/runs/38835599694) | 受入後の修正 | 6 buildすべてsuccess。Linux v3は、crates.ioの一時的な通信errorで一度落ちたが再実行で通った |
| 実機（SOL-43） | 利用者のWindows・macOS | 試験build | 2026-10-16に利用者が確かめた。ブラウザーで取得した未署名archiveが、READMEの手順で起動した |

受入の後の変更は、試験（daemonの偽solverの書込み順）、workflow、文書だけである。製品コードは変えていない。

## 除外した試験

`cli`の`nlh_p2::fixed_seed_smoke_resume_and_inspect_lifecycle`（P2）を除外した。
2 sweepのrunを3 sweepまでresumeした結果が、最初から3 sweepでsolveした結果と一致しない。
P2は今回の配布物に含まれない。2026-10-15に利用者が除外を決めた。追跡はLinear SOL-47で行う。

## 受入で直した試験

初めて実行したignored試験とrelease build固有の試験に、試験側の不備が3件あった。

- `a_canceled_heads_up_solve_closes_as_canceled_and_resumes`: exit 0を期待していた。協調停止のexit codeは仕様（`docs/cli-reference.jp.md`）で130なので、130を期待するように直した。
- `report_honors_time_budget`: release buildでは、1-comboのriverが1秒以内に上限の100万iterationへ達していた。上限を10億iterationに上げ、時間で止まることを確かめる形に戻した。
- `fixed_seed_smoke_resume_and_inspect_lifecycle`: `solve`に存在しない`--max-sweeps`を渡していた。sweepの上限は設定で渡すように直した。

## 手元の確認

- `cargo fmt`、両feature構成の`clippy`、`cargo test --workspace`、`cargo test -p cli --no-default-features`、`check_docs.py`、`tools/tests`がすべて通った。
- v3・P1のみのbuildを`package.sh`でzipにした。展開後に次を確かめた。
  - `solvers --version`は`solvers 0.1.0 (3bd466e9), P1 only`を表示した。
  - `river_small`のsolveが通った。
  - `solversd`を`--ui`なしで起動すると、同梱の`ui/`を配信した。
  - そのGUIで、既定templateについて検証→solve→進捗→完了→Resultsまで進めた。
