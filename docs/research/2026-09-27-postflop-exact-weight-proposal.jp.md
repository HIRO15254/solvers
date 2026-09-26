# HU Postflopの互換weightを線形時間で保つ案

2026-09-27の未採用設計。公開仕様・既定動作を変更する文書ではない。
対象は[数値監査](../../experiments/hu-postflop-r1/range-scaling/source06/normalizer-edge-audit.md)の
「大きい不合法handのweightを減算すると、唯一の小さい合法massも消える」問題。
役強さ・カード番号・席別IDの事前計算を比較する[既存実験](../../experiments/hu-postflop-r1/showdown-kernel/README.md)とは
source identity・数値受入・性能測定を分ける。この案はsolver全体の任意精度化を意味しない。

## 1. 分岐と前提

各呼出で非zeroの相手reachを一度走査し、以下の十分条件を満たす場合だけ既存のf64 sweepをそのまま使う。
満たさない場合は**呼出全体を5×u64の厳密なmass sweep**で計算する。手札ごとの総当たりfallbackは使わない。
52枚deckを固定すれば、判定・いずれのsweepもO(n)、追加作業領域は手札数に依存しない。

- 入力は有限かつ非負のf32。±0はzeroとして扱い、NaN・Inf・負値を判定前に明示的に排除する。
  epsilonで正weightをzeroへ変えない。ここで保証するのは、kernel入口ですでに表現されているreachである。
- 同じglobal comboをtable内に重複させない。rank group・prefix・self補正は同じ母集合に従う。
  Nと指数範囲は演算に使う全非zero reachを覆う必要がある。相手vector全体を使うのは安全側であり、
  boardで絞る場合はtableとself補正も同じ絞り込みにする。
- HUの母集合は最大1,326項。この上限と5 limbの根拠を他ゲームの汎用engineへ暗黙に持ち込まない。
  root configのweight範囲だけから、変換後の全reachが常に1以下とは仮定しない。

## 2. f64でmassが厳密になる十分条件

正のf32のexponent fieldをE、fraction fieldをFとし、整数mとshiftを次のように復号する。

```text
E = 0:  m = F,          shift = 0       （subnormal）
E > 0:  m = 2^23 + F,   shift = E - 1   （normal、E < 255）
w = m × 2^(shift - 149),   0 < m < 2^24
```

非zero項数をN、shiftの最小・最大の差をDとする。N≥1について、

```text
B = D + 24 + ceil(log2(N + 1))
fast path ⇔ B ≤ 53
```

`ceil(log2(N+1))`は整数の`bit_length(N)`で求める。浮動logや`ceil(log2(N))`へ置き換えない。
Bは最大288になるため、shift fieldに合わせたu8加算でwrapさせず、u32以上で計算する。
N=0ではmin/maxを求めず、既存のzero-reach演算を使う。負utilityによる−0も含め、
単なる出力zero埋めへの短絡で旧bitsを変えない。

**証明。** 共通単位をq=`2^(minshift−149)`とすると、各項は`a×q`で整数aは`2^(D+24)`未満。
最大N+1項の和の整数係数も`(N+1)×2^(D+24) ≤ 2^B`未満となる。
B≤53なら、これらの整数係数とその負値はf64で厳密に表現できる。qは最小`2^-149`、
全finite f32を1,326項集めても和は`2^139`未満で、f64のexponent範囲から外れない。

全体和T、各カード和Ca/Cb、同順位group、strict-below prefixは同じ母集合の部分和なので、
任意の既存加算順で各中間結果が厳密になる。`below_total += group_total`もこの範囲内にある。
hand自身の相手reachをsとすると、カード集合の交差はその同一comboのみで、`Ca+Cb ≤ T+s`。
現在の左結合`T−Ca−Cb`が負になっても下限は−sであり、範囲を超えない。
最後の+sをN+1項の余裕が保護する。同順位tieも同じ議論で成立する。
win・tie・loseは互いに素な集合のmassなので、`lose = compat−win−tie`の各差も厳密である。

したがって、この条件に**合格する入力**では、既存の加算・減算順を変えずにmassの厳密性と旧bitsを保てる。
不合格は旧計算が必ず誤る証拠ではない。例えばN=1,081ではD≤18しか合格せず、
深いactionで広がった通常のreachでもfallbackし得る。「極小weightの場合だけ切り替わる」とは保証しない。

## 3. 整数sweepと丸めの境界

fallbackはf32を`2^-149`単位の非負整数として保持する。
最大finite f32は`(2^24−1)×2^253`単位、最大1,326項の和とself追加は`2^288`未満なので、
5×u64＝320 bitで収まる。carry・borrowは明示的に扱い、非負massの減算をwrapさせない。
全reach≤1を証明・検査する別契約なら160 bitで足りるが、本案はその仮定を置かない。

整数のまま`(T+s)−Ca−Cb`、win、tie、`compat−win−tie`を完了し、各massを一度だけf64へ
round-to-nearest, ties-to-evenで変換する。**compat/win/tieを先にf64へ変換してからloseを引くと、
相殺を再導入する。** limbを順次f64に足すだけの変換も、正しい丸めの代用にしない。
group/all/belowの53個ずつのaccumulatorを持つならpayloadは約6,360 bytes
（53×3×5×8）。f64版の約1,272 bytesより増え、速度・RSS削減はこの設計から推定しない。

保証は非負massの計算まで。f64 utilityとの積・符号の違うpayoffの合算・最終f32化には通常の丸めが残る。
f32で表せないほど小さいCFV、reach生成時のf32乗算underflow、SOL量子化によるzero化は別の境界である。
fallbackは誤って消えていたmassを回復するため、旧CFV・solver state・収束軌跡のbitsを変え得る。

## 4. 同時に揃える経路

| 対象 | 数値的な責務 |
|---|---|
| `holdem/kernel.rs`のcompact/global fold・showdown | 共通判定・同じexact mass意味。global側はdense/equityと差分testにも関わる |
| `holdem/postflop.rs`のroot normalizer | 同じ互換massを使い、正の合法pairを誤って「なし」にしない |
| `cli/sol.rs::compatible_reach` | 保存・表示する条件付きEVの分母をkernelと揃える。負値をzeroへclampして相殺を隠さない |
| equity、Full SOL、保存profile監査、lazy river | 共通primitiveを通ることを検査し、旧分母による独自再計算を残さない |

normalizerのown-weight積と最終和まで整数で厳密にする提案ではない。正のf32 weight同士の最小積は
`2^-298`でf64に収まり、互換massが正しく得られれば「唯一の合法pair」の全消失を避けられる。
normalizerだけを一回のpairwise列挙へ変更しても、反復kernelと保存分母の不整合は解消しない。
公開の数値説明、test、source provenanceを一つの変更として揃え、formatの構造互換と数値結果の互換を区別する。
凍結oracle `cfr-ref`を変更しない。

## 5. 受入テストと性能の比較単位

1. 復号と判定: zero、最小subnormal、最大subnormal、最小normal、全exponent境界、最大finite、
   N=0/1/2/2^k−1/2^k/1,326、B=52/53/54。十分条件合格時の全中間massを独立整数oracleと比較する。
2. 算術: 同一combo、共有1枚、互換pairなし、唯一の合法massが`f32(1e-20)`／最小subnormal、
   `T−Ca−Cb<0`となる途中式、複数groupのprefix、win/tie/lose各単独utility、両席入替、
   非対称supportと後続mask。整数→f64変換はhalfway tie、carry跨ぎ、最高位境界も検査する。
3. 分岐間: 同一の合格入力を旧経路・強制exact経路へ与えてmass bitsを比較する。
   全到達kernelで条件を満たすfixtureでは、utility式も同順のままF32/I16 state、strategy、
   EV/BR/NCとthread数別の再現性を検査する。
   不合格入力は旧bitsを正解とせず独立oracleを使う。
4. 一貫性: root構築からFull SOL保存・読戻し・条件付きEV・lazy riverまでtiny合法pairを追う。
   表現可能な分母の正値と、quantizationなど後段の許容誤差を別々に検査する。
5. 資源: 固定source/config/反復数でRiver/Turn/Flop、疎・広いsupportを比較し、判定費用、
   street別fallback率、solve時間とpeak memoryを保持する。整数化を混ぜた結果をprepared-table単独比較へ合算しない。
