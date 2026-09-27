# Fused updateの整形確認 v2

`rustfmt 1.9.0-stable (2d8144b788 2026-07-07)`でcandidateの3ファイルとRust fixtureを
edition 2024としてstdinから整形し、stdoutを保存した。4ファイルとも旧版とbyte単位で同じだった。
このv2は整形確認の追加証拠であり、アルゴリズム・fixture・生成器を変更していない。

- [formatting.json](formatting.json)は4回のcommand、exit、stderr、時間、前後pinと空のdiffを保持する。
- [provenance.json](provenance.json)は旧生成器、原本、candidate、fixtureとの対応を固定する。
- [checks.json](checks.json)は旧生成器の`--check`、3 sourceの逆変換と原本一致を記録する。
- `candidate/`と`tests/fused_update.rs`は旧版の同名ファイルと完全に同じ。
  配備は[旧protocol](../protocol.jp.md)の3 source置換とtest fixtureコピーをそのまま使う。

旧[prepare.py](../prepare.py)、原本snapshot、provenance、checksは編集しない。
新しい生成器や検査frameworkは追加しない。

```text
python -B experiments/hu-postflop-r1/flop-scaling/fused-update/prepare.py --check
```

rustfmtは構文を受け付けたが、Rustの型検査・clippy・native tests・solveはここでは実行していない。
整形成功をこれらの成功や性能の証拠として扱わない。native fixtureには、旧APIの処理列と新methodの
全F32/I16 state比較、full/rebased view、空次元、discount/floor/reset、sentinel、signed zeroを含む。
最終的なcompile・回帰検査・性能比較は固定したGCP資源枠で行う。
