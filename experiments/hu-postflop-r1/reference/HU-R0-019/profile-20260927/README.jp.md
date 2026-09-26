# HU-R0-019: Whole / action Copyによるprofile取得の検査

この検査器は、既存Riverの各nodeでCopyしたWhole rangeとaction別rangeから、
profileを復元できる範囲と欠測を区別する。solver、ブラウザー、外部サービスは実行しない。
**外部品質の合否や同一ゲームを認定せず、strategyの自動補完・再正規化もしない。**
取得原文と実測結果は別ファイルで保持し、ここでは解析方法だけを定義する。

[5判断点の有限監査結果](report.jp.md)、[検証JSON](verification.json)、
[全support行の診断JSON（gzip）](profile-audit.json.gz)を保持する。
親子のown reach不一致と表示/Copyの差があるため、全木profileとしての復元やEV/BR認定は行っていない。

## 必要な入力

[既存観測](../observed.json)の12 decision nodesと32 action edgesを使う。
全取得なら12 Whole + 32 actionで44のCopy原文が必要になる。
各Copyには原text、byte数・SHA-256、node履歴・actor・Whole/action選択を結び付ける。
取得UTC、URL、loaded履歴、filter、board/pot/stacks、library/depth、表示されたversion・precision・
warningも取得側のmetadataに残す。未表示、空Copy、未取得、明示的な0を区別する。
過去の両root rangeを参照する場合は、今回のloaded solutionとの対応を別途確認する。

manifestのschemaは`r1.reference-profile-capture/v1`。
`observed`と`root_ranges`、各Copyの参照は全て`{path, bytes, sha256}`で、
pathは`--evidence-root`の内側にある相対pathとする。

```json
{
  "schema": "r1.reference-profile-capture/v1",
  "case_id": "HU-R0-019",
  "observed": {"path": "observed.json", "bytes": 0, "sha256": "replace-with-actual-hash"},
  "root_ranges": {
    "oop": {"path": "oop-range.txt", "bytes": 0, "sha256": "replace-with-actual-hash"},
    "ip": {"path": "ip-range.txt", "bytes": 0, "sha256": "replace-with-actual-hash"}
  },
  "captures": [
    {"history": "", "actor": "BB", "whole": null,
     "actions": [{"label": "check", "range": null}]}
  ],
  "capture_metadata": {"note": "Illustrative structure only; not measured evidence"}
}
```

上記は構造例であり、有効な証拠ではない。historyとaction labelは既存`observed.json`に完全一致させる。
未取得node/actionは省略でき、未取得fileはnullで表せる。空ファイルは取得済みだがtokenなしと判定する。
`notes`と`capture_metadata`は任意の取得側記録で、検査器はその文章の真実性やUI状態を証明しない。
補助の再Copy原文は任意の`supplemental_files`配列へ
FileRefを置くとbytes/hash検査に含める。補助fileからpolicyは生成しない。

```text
python -B experiments/hu-postflop-r1/reference/HU-R0-019/profile-20260927/check_profile.py --manifest <capture-manifest.json>
python -B experiments/hu-postflop-r1/reference/HU-R0-019/profile-20260927/check_profile.py --manifest <capture-manifest.json> --assumed-absolute-error 5e-13
python -B experiments/hu-postflop-r1/reference/HU-R0-019/profile-20260927/test_check_profile.py -v
```

既定のevidence rootは`HU-R0-019/`。結果JSONはstdout、無効なfile identity・構文・カード等は
stderrとexit 2で返す。exit 0は**診断JSONを作れた**ことだけを意味し、欠測や数値不整合がないとは限らない。
出力保存は呼出側で成功後に行い、失敗した実行で既存報告を上書きしない。

## 解析と区間の意味

原文はUTF-8の`combo:weight`をcomma区切りとする。科学表記を含む十進tokenをexactな有理数へ
変換し、tiny positiveをf32/f64で失わない。カード順を正規化して重複comboを拒否し、
負・非有限・1超のweight、正weightのboard衝突も拒否する。原bytesは書き換えない。

Copy WholeをW、action CopyをAとして、**Aが同じ尺度のown realization reach × action確率**
である場合だけ`A/W`を条件付きstrategyと解釈できる。action別の個別正規化などがあれば成立しない。
同じactorの直近の祖先action Copyと後続Wholeを比較し、この解釈の整合を検査する。
既に重み付きのAへ祖先Wを再乗算しない。相手だけがactionした後は、自分のown reachは変わらないという
モデルで比較する。これらの一致だけでCopy機能の意味や参照版を承認しない。

各root positive comboについて次を報告する。

- Whole/actionの未取得file、token欠落、明示0、root support外のpositive token。
- literal tokenの`sum(A) - W`。全actionが存在してW>0、和が厳密一致する場合だけexactな比を出す。
- 非閉包なら比を再正規化しない。Whole=0なら全action=0でも比は未定義のまま残す。

任意optionのepsilonは**各present tokenの絶対誤差がepsilon以下**という仮定である。
`5e-13`は最近接の小数12桁丸めを仮定する場合の候補で、UIの出力桁・丸め保証を取得した事実ではない。
科学表記、trailing zeroの省略や少数の例だけから、この誤差上限を認定しない。
欠落tokenへ区間`[0,epsilon]`を自動付与せず、omissionの意味は未知とする。

各tokenの区間は`[max(0,x-epsilon), min(1,x+epsilon)]`。
各comboで`I_W`と全action区間の和の交差を取り、`sum(A)=W`が局所的に成立するか検査する。
Wの下限が正なら、分子と分母の区間から各`A/W`の保守的な包含区間を出す。
区間が0を含むWholeでは比を識別できない。各actionの包含区間の端点を同時採用したpolicyは生成しない。
node内の整合と祖先・子のpairwise整合が全て成立しても、共有変数を含む全木に同時解がある証明にはならない。

## EV / BRへ渡す際の境界

検査器はreference profileのEV/BRを計算せず、取込可能な完成policyも生成しない。
profile到達確率が0の枝でも、一方のプレイヤーが逸脱すれば到達できるため、joint reachだけで欠測を免除できない。
`BR_i`において固定相手`-i`自身の過去actionからown reachが真に0なら、その先の固定相手policyは
値へ影響しない。ただし丸め後0・省略tokenはその証明ではなく、別seatのBRに同じ免除を移せない。
この検査器はzero-reach免除を自動認定せず、各root supportに対する欠測を残す。

取得profileをこちらの仮定ゲームで独立再評価することと、GTO Wizardの元ゲーム・内部精度を認定することは異なる。
rake徴収・丸め、参照版・個別残差、EV基準が不明なら`condition_match=unverified`、
`quality_status=not_evaluated`、`acceptance=null`を維持する。
[外部比較契約](../../../acceptance/external-contract.md)の条件を、この診断のexit 0で代用しない。

## Rootの取得済み3 actionに対する算術確認

[取得manifest](root-capture-manifest.json)のroot記録は、rootが2026-09-26 UTC 17:58頃から
18:03:20.695までに同じloaded BB root・filterなしで取得した3 actionを固定する。
正確な開始時刻は未取得。Whole textは過去の`oop-range.txt`と文字完全一致したというrootの報告に基づき、
同じ原文fileを参照する。checker作成者はブラウザーを独立閲覧していない。

保存rawについて、報告された文字数/FNV-1a32を独立照合した。
Wholeは1830 / `90de9f9e`、Allinは1887 / `0e52789e`、Betは2204 / `7f3f2fd2`、
Checkは1498 / `ad12bde2`。各fileはCopy原文へ末尾LFを1個加えたbytesであり、SHA-256はmanifestに固定する。

130 root positive combosのうち、全3 actionのtokenがpresentな72件は全てliteral sumとWholeが異なり、
仮定epsilon `5e-13`でも全72件が局所的に非整合だった。その72件内の最大絶対残差は`4.5851e-8`。
残る58件は1つ以上のaction tokenが欠落しており、0として集計していない。
例えばAsAdは全tokenがpresentで、sum−Wholeは`+1.0119e-8`。
AsAhはBet tokenが欠落し、Whole `0.2124`に対しAllin `5.63e-10`、Check `0.2124`がある。
この観測は単純な12桁丸めモデルの不成立を示すが、内部f32演算、正規化、別尺度等の原因を確定しない。

合成17 testsは成功した（WindowsのTemp ACL制約により通常sandboxのfixture作成が失敗した後、
同じsuiteを権限付きで実行。最終0.314秒、exit 0）。実参照policyの復元成功を意味しない。

## Root Allin 55 → BTN Fold / Callの追加取得

同じmanifestに`history="allin 55"`、actor BTNを追加した。取得窓は2026-09-26 UTC
18:13頃〜18:15:19.108で、正確な開始時刻は未取得。URLはRanges、`history_spot=13`、
`river_actions=RAI`。rootから報告された表示はpot 95.5 / 40.5bb、pot odds 36.5%、
Call 40% / Fold 60%、BTN weighted combos 0.4であり、厳密な内部数値としては使わない。

Wholeの1817文字 / FNV `5f6656d4`は既存`ip-range.txt`と一致したという報告に基づき同fileを使う。
Callの1188文字 / `b96fb5d0`、Foldの1693文字 / `66e88a52`を保存原文から独立照合した。
Callの初回転記ではAhKh / AhKsが欠けてFNV検査に失敗し、rootがブラウザー原文と再照合して修正した。
このmanifestは修正後の一致したbytesを参照する。取得raw自体はchecker側で変更していない。

115 root positive combosのうち両actionがpresentなのは40件。
literal sumがWholeに一致するのは2件、異なるのは38件で、75件にはaction token欠落がある。
仮定epsilon `5e-13`では、全token presentの40件中3件が局所整合、37件が非整合だった。
その40件内の最大絶対残差はAhKhの`2.113e-9`（sum−Wholeは負）。
Wholeは115件とも固定IP rootと数値一致し、この席に自分の先行River actionがないモデルと整合するが、
各Copyの意味や出力精度の認定にはしない。この追加時点ではrootと合わせて12 decision中2件の取得記録であり、
残りのnodeを未取得として診断する。解析ロジックは変更せず、追加manifestへ同じ検査器を適用した。

## Root Bet 13.5 → BTNの4 action追加取得

`history="bet 13.5"`、actor BTNのWholeとFold / Call / Raise to 37 / Allin 55を追加した。
取得UTCは2026-09-26 Whole 18:19:38.735、Allin 18:19:51.301、Raise 18:19:55.171、
Call 18:19:58.968、Fold 18:20:05.337。URLはRanges、`history_spot=13`、`river_actions=R13.5`。
rootから報告された表示は開始pot 40.5bb、現在pot 54bb、残stack BB 41.5bb / BTN 55bb、
Allin 29.1%、Raise 0%、Call 44.3%、Fold 26.6%だった。

Whole 1817文字 / `5f6656d4`は既存IP rangeと一致したという報告に基づき同fileを使う。
保存したAllin 1776文字 / `ea1ab96c`、Raise 282 / `f5d54325`、
Call 1147 / `42576571`、Fold 1426 / `a7e7e84e`は文字数とFNVを独立照合した。
表示0%のRaiseにも**20 positive tokens、raw weight合計`1.16662e-7`**があり、枝をzero reachと扱わない。

115 root positive combos中、全4 actionのtokenがpresentなのは4件。
4件ともliteral和はWholeと異なり、仮定epsilon `5e-13`でも全件が局所非整合だった。
その4件内の最大絶対残差は`5.06e-10`。残る111件はaction token欠落として保持する。
Wholeは115件とも固定IP rootに数値一致する。Fraction検査器とは別の80桁Decimal集計でも、
上記の件数・残差・条件付き区間判定が一致した。
この追加により取得manifestは12 decision中3件となる。全木policyの補完や品質認定は行わない。

その後のBet→Raise、Bet→Raise→Allinと親子再Copyの観測・不連続の定量値は
[有限監査結果](report.jp.md)へまとめた。検査器は各comboの親own-product値・Wholeとの差、
比較可能数・不一致数・最大差も出力する。空Copyはinventoryの`empty_copy` / `token_count=0`で
明示し、未取得fileとは分ける。親子不整合と補助rawのhash検査を加えた最終suiteは19件成功した。
