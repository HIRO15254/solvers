# HU-R0-019 Copy profile: 5判断点の有限監査

**取得したCopy出力から、全木で連続するreference profileは復元できたと認定しない。**
全12判断点中5点を取得したが、親の同席action Copyと子のWholeに大きな不一致があり、
同じnodeを再選択してCopyしても差を再観測した。表示頻度とCopy内容の相違も残る。
fallback、River再solve、rangeの別尺度、export仕様等の原因は未確定である。
欠落を0やuniformで補完せず、EV/BR評価と外部品質認定は行っていない。

## 原文と算術検査

[manifest](root-capture-manifest.json)は各node/actor/action、URL、取得UTC、原文bytes/SHA、
ブラウザーで報告された文字数・FNVを固定する。root、root Allin応答、root Bet応答、
Bet→Raise応答、Bet→Raise→Allin応答の5点である。
raw原文の文字数・FNV・末尾LFを独立照合し、[検査結果](profile-audit.json.gz)では
既存observed/root rangesと補助再Copyを含む20 FileRefsのbytes/SHAを再検証した。
checker作成者はブラウザーを操作せず、UI状態・時刻はrootの取得報告に依存する。

各行は固定root positive supportに対する検査。`present`はWholeと全actionのtokenがあることを指す。
token欠落は未取得fileや明示0とは異なり、0と推測して集計しない。

| 判断点 | root support | 全token present | literal和一致 | literal和不一致 | token不足 |
|---|---:|---:|---:|---:|---:|
| BB root | 130 | 72 | 0 | 72 | 58 |
| BTN、root Allin 55後 | 115 | 40 | 2 | 38 | 75 |
| BTN、Bet 13.5後 | 115 | 4 | 0 | 4 | 111 |
| BB、Bet 13.5→Raise 37後 | 130 | 93 | 4 | 89 | 37 |
| BTN、Bet 13.5→Raise 37→Allin 55後 | 115 | 0 | 0 | 0 | 115 |

各present tokenへ仮に絶対誤差`5e-13`を許した局所区間検査でも、上表順に
72 / 37 / 4 / 86件が非整合だった。最後のnodeは全action tokenが揃うcomboがなく判定していない。
epsilonはUIの保証ではなく、全木の同時整合や採用policyを生成するものでもない。
小数12桁の丸めだけで説明できるとは認定しない。

## 親子のown reachが連続しない観測

「action Copyがその席のown reach × action確率」というモデルでは、相手の行動だけを挟む
同席の子Wholeは親action Copyと一致するはずである。原文の直接比較は次の通りだった。

| 親action Copy → 子Whole | positive token数 | raw weight合計 | 共通tokenの差 |
|---|---|---|---|
| BB root Bet 13.5 → BB Raise 37応答 | 121 → 114 | 5.989307709197 → 12.987758117815 | 112中92が異なる |
| BTN Raise 37 → BTN Allin 55応答 | 20 → 2 | 1.16662e-7 → 3.95764e-7 | 2中2が異なる |

BBのTc9cは`0.00106885172 → 0.3819`、差`0.38083114828`。
BTNのQd8dは`5.708e-8 → 2.80527e-7`、9h9cは`1e-12 → 1.15237e-7`だった。
BB childで親にない2 tokens、親にだけある9 tokens、BTN childで消えた18 tokensも0へ置換していない。

2026-09-26 UTC 18:32:17.817に親を再選択してBB WholeをCopyし、
[再Copy原文](bet-bb-whole-recheck.txt)の1761文字 / FNV `dba23c42`を確認した。
これは91 tokens、合計`5.989307450994`。root Bet Copyとの共通91件中86件は厳密一致し、
残り5件も差は最大`1e-12`、省略された30件の親weightはすべて`1e-7`未満だった。
この1取得から一般的なomission規則は認定しない。
18:32:45.008に同じRaiseを再選択すると、子Wholeは初回の2017文字 / `9f2bfa6b`と文字一致した。
したがって、この再観測は一度の転記誤り・stale clipboardだけでは説明できない差を示す。
URLの末尾には後続actionが残り得るため、選択中の`history_spot`とUI手番・potも別に記録した。

最後のBTN nodeでは表示がCall 74.2% / Fold 25.8%である一方、WholeとCall Copyは同じ2 tokens、
Fold Copyは空文字だった。保存Foldは追加LFだけの1 byteであり、**取得済みの空Copy**として保持する。
Call 100%やFold 0%のpolicyへ変換しない。
また親BTN Raise 37は表示0%でも20 positive tokensを含むため、表示0%を真のzero reachと認定しない。

## 検証・保持と制限

[verification.json](verification.json)にchecker/manifest/testのhash、CLI、出力hashと集計を保持する。
19個の軽量合成testsは成功し、空Copy・欠落・親子不一致・区間境界・hash改変を検査した。
test成功の根拠はtool実行transcriptであり、独立したraw test logを保持したとは主張しない。
正式checker出力は1,210,791 bytes、gzip後37,352 bytes、SHA-256は
`0614e66307a04c5291448b5bb71668b17b215aa348d8ebaa72e9ef80ad58777f`。
同じCLIを再実行し、展開した元stdout bytesと完全一致することを確認した。

```text
python -B experiments/hu-postflop-r1/reference/HU-R0-019/profile-20260927/check_profile.py --manifest experiments/hu-postflop-r1/reference/HU-R0-019/profile-20260927/root-capture-manifest.json --assumed-absolute-error 5e-13
```

gzipはWindows上でcaptureしたstdout原bytesを保持する。別OSの改行差を跨ぐ場合はJSON値の一致と
source/manifest hashを別に照合し、byte同一とは言わない。
残る7判断点、exportの重み・省略・丸め仕様、個別版、精算条件をこの監査で埋めていない。
`condition_match=unverified`、`quality_status=not_evaluated`、`acceptance=null`を維持する。
