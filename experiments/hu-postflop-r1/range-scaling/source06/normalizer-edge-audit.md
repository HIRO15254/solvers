# 微小weightと互換reachの桁落ち: source06数値監査

2026-09-26の読み取り監査。**合法pairが存在しても、包除原理の減算でその全weightが0に丸まり、
builderのnormalizer assertへ到達する入力がある。** 同じ式はcompact導入前の
`fd740d9c7e28c44e1263051d0a42611008046071` にも存在し、支持集合圧縮の新規回帰ではない。
この記録は数値境界の証拠であり、作業状態の台帳ではない。本体・固定済みsource・実験protocolは変更していない。

## 入力と根拠

任意の通常のRiver betting設定に、次のboard/rangeを指定する。

```toml
board = "2c 7d 9h Js Qs"
oop_range = "AsAh:1"
ip_range = "AsKh:1,KcKd:1e-20"
```

boardと3comboはいずれも整合する。`AsAh × AsKh` はAsが重複して不合法、
`AsAh × KcKd` は合法で、そのjoint weightは正の `f32(1e-20)`。
range parserはこの有限な正weightを受け付け、支持集合にも残す。
CLIの`validate_board_ranges`とlazy riverの`has_compatible_reach`は合法supportを直接調べるので、
この入力を「互換pairなし」とは判定しない。

一方、現normalizerはIP全weightのf64和からAsを持つIP weightを引く。
`1 + f32(1e-20)` はf64でも1に丸まるため、`1 - 1 - 0 + 0 = 0`。
唯一のOOP comboのweightは1なので、集計normalizerも0となる。

| 対象 | source06の行 | compact前commitの行 | 確認した式・境界 |
| --- | --- | --- | --- |
| `crates/holdem/src/postflop.rs` | 1159、1170、1173 | 1064、1071–1072、1075 | normalizerの包除減算と`assert!(normalizer > 0.0)` |
| `crates/holdem/src/kernel.rs` | 147、174–175 | 86、113–114 | showdown/foldの互換weightも同種の減算 |
| `crates/cli/src/sol.rs` | 110、132 | 110、125 | 保存EVの分母となる`compatible_reach`も同種の減算 |

旧commitではroot rangeがglobal1326配列、source06ではseat-localからglobalへ対応付けているが、
この入力の和・減算とnormalizerのassert条件は同じである。
旧内容は`git show fd740d9c7e28c44e1263051d0a42611008046071:<path>`で直接確認した。

source06 archive SHA-256:
`ab9c4a8d32d83de2827319019c77361a1f36192c194185469700d90d9f6a3fab`。
監査時の3ファイルは[source manifest](source-candidate-manifest.json)のsize/SHAと全件一致した。

| File | SHA-256 |
| --- | --- |
| postflop.rs | `073fbd08d9767f140a16c503cb513f4265801d2d96b23db79a2f0bb72f9ca1c6` |
| kernel.rs | `f48e780d558dd64b9da6375a99ee832261d800a1ff82c5af9d562d89b5e3ff37` |
| sol.rs | `20d251369f80c6338bfcab6832c1750e153edeced7d260bf4edd101be1243781` |

## 実施した再現と実施していないこと

次のPython標準ライブラリによるIEEE浮動小数点算術だけを実行した。Rust build、solver実行、
CLI panicの動的再現、実験の再測定はしていない。Rustの該当コードとこの算術からassert到達を導いた静的診断である。

```python
import struct
from fractions import Fraction

tiny = struct.unpack("<f", struct.pack("<f", 1e-20))[0]
total = 1.0 + tiny
compatible = total - 1.0 - 0.0 + 0.0
print(tiny, total, compatible, compatible > 0.0)
print(Fraction.from_float(tiny))
```

実測出力:

```text
9.999999682655225e-21 1.0 0.0 False
1547425/154742504910672534362390528
```

`tiny`のf32 little-endian bytesは`08e53c1e`。正の合法pairを直接列挙した和は
`9.999999682655225e-21`であり、ゼロではない。

## 後続修正の境界

normalizerだけを正に直しても十分ではない。terminal kernelのCFVとSOLの条件付きEV分母にも
同種の桁落ちが残る。この例では大きい不合法pairを先に除外する必要がある。
任意のepsilonでtiny handを切り落とす変更は、正weightの支持集合を保つ契約の修正にはならない。

- Build時normalizerは、board整合する両席の手札からカード非重複pairだけを列挙し、
  `f64(own_weight) * f64(opp_weight)`を加算する方式を検討する。
  計算量はO(H0×H1)、追加memoryはO(1)。最大でも1326²未満のpair検査をbuildごとに行うため、
  iteration内kernelと分けて費用を評価できる。正のf32 weight同士の積はf64でunderflowしない。
  これは減算による全weight消失を避ける案であり、任意の異なる桁の実数和を数学的に厳密表現する保証ではない。
- `kernel.rs`のfold/showdownと`sol.rs::compatible_reach`も整合して修正する。
  全terminalで常にO(H0×H1)を行うと、現在の概ねO(H0+H1) sweepに対して大きな計算費用になり得る。
  まず直接合法pair和を独立referenceとして用意し、減算の誤差が支配的なhandだけにfallbackするなら、
  その判定・誤差上界・通常caseのbit変化を別途検証する。`result == 0`だけでは正だが不正確な残差を覆えない。
- 差分は原則として上記3ファイルと対応するholdem/CLI回帰test、数値契約の説明に限定する。
  frozen `crates/cfr-ref`は変更しない。通常caseのbit列やqualityが変われば既存性能比較と混ぜず、
  新しいsource identityで検証し直す。

必要なtestは、(1)上の唯一の合法pair例でbuilderが成功しnormalizerが直接和と一致すること、
(2)両席入替・共有1枚/2枚・合法pairなし・複数のweight桁・最小正subnormalを含む境界、
(3)foldとshowdownのwin/tie/loseを直接合法pair積分と照合すること、
(4)Full SOL保存EVの分母・offset・P1非対称dimsとlazy riverの再buildが正しく扱われること、
(5)通常重みの既存oracle/dense比較・F32/I16・thread等価性の回帰確認である。
現在のtiny-weight testは大きい合法pairも含むため、「唯一の合法massがtiny」という境界を覆っていない。

source06のfull validationや固定iterationの性能証拠は、そのfixtureでの成功を示す。
極端なtiny-weight全域の数値的正確性を認定する証拠としては扱わない。
