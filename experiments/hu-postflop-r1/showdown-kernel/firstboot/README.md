# VM09 firstboot 回収証拠の独立検算

**このbundleを完全な検証成功の根拠には採用しない。** 新実装のresultと8件のsupervisorは
すべてcompleted／exit 0を記録しているが、回収した出力3件が実行時のhash・byte数と一致しない。
Spot停止後の回収byte不一致として保持し、空になった理由は断定しない。
検算結果は[verification.json](verification.json)、再現手順は[verify_firstboot.py](verify_firstboot.py)。

## 確認できた範囲

- bundleは4,557,773 bytes、SHA256 `a17f29921feb237c7266342796bd9f100d472d1ed7904d9ceb1d552af48fa141`。
  外manifestはSHA256 `80d0fd55d9cc4d4931ce4bc24aa345947e4b3454192e943d9f9616b817500e25`。
  内外manifestは元bytes完全一致。48 included files、skipped 0、archive memberの過不足なし。
  各`files/00000000`形式のpayloadはcollector manifestのbyte数・SHAと一致した。
- old/source archiveは361 files、new/source archiveは362 files。それぞれ明示directory 2件を含み、
  source manifestのexact file set・全byte数・SHAと一致。newは`source-new01`候補で、baseは
  `d8135a12c22c9d5e70216380298bda480e63ad05`。source-after filesystemの一致はrunnerの記録上の主張であり、
  独立した実行後snapshotを取得したものではない。
- newの8 stageについて、command、cwd、target、source archive/manifest、runner、監視script、
  identity before/after、exit 0、cleanup完了を照合。compiler/Python executableはidentityのみを保持する。
  release binaryは3,900,568 bytes、SHA256 `54b85c3886de32e45510b9bee2ff71e0f9440c07e3ee9e224c034da54e94145a`で原bytes一致。
- `/tmp/r1-kernel-old/validate.py`は再起動で消失したため、収集前にfrozen sourceから復元したもの。
  7,499 bytes、SHA256 `0f84890a1860498040fec55b160bdc07c7c1135f10fa8ea8475d8185739bb07a`。
  両source archive内の`experiments/hu-postflop-r1/range-scaling/validate.py`および実行時pinと完全一致する。
  実行後の元`/tmp`fileそのものが生存していたとは扱わない。

## raw logから数えたテスト

| stage | passed | failed | ignored | 根拠 |
|---|---:|---:|---:|---|
| workspace-tests | 933 | 0 | 31 | `files/00000035`、56 summary行と個別test行が一致 |
| release-oracle | 3 | 0 | 0 | `files/00000023`、3 test名とsummaryが一致 |
| release-river-resolve | 不明 | 不明 | 不明 | 回収stdoutが0 bytesでsummaryなし |

workspaceは44通常test harnessと12 doc-test harnessを含み、doc-testsはすべて0件。
新kernelの6 testsも個別名で成功を確認した。release-oracleの3成功のうち2件はworkspace成功済みの再実行、
1件はworkspaceでignoredだったtestの追加実行である。観測した成功実行数は936、固有成功testは934件。
resolveの「1 passed」はraw logから確認できないため加算しない。

## process recordと回収出力の不一致

| role / stage / output | 実行時記録bytes | 回収bytes |
|---|---:|---:|
| new / release-river-resolve / stdout | 161 | 0 |
| old / toolchain / stdout | 196 | 0 |
| old / toolchain / samples | 634 | 0 |

期待SHAと回収SHAはverification.jsonの`process_output_pin_mismatches`に全件保持する。
**collectorに記録された48 filesの検算成功は、process recordが指す元出力との一致を意味しない。**
欠けた出力を推定・再生成して成功に補完していない。

oldはbuild-onlyで、toolchainのrecordはcompleted、release-exampleと全体resultはrunningのまま回収された。
release-exampleの終了code・cleanup完了・identity after・binaryはないため、旧build完了とは扱わない。

この証拠のbootは`02dcc8e8-f1c9-4ec6-a657-f0f9b12c0f0f`、Intel Xeon、guest 2 physical core / 4 logical CPU。
後続bootで同sourceを新規検証した記録や性能時間とは合算しない。性能比較・採用判定・R1全体の受入証拠ではない。

## 再検証

repository rootから実行する。保持codeやbinaryは実行せず、tarをメモリ内で読むだけである。

```text
python -B experiments/hu-postflop-r1/showdown-kernel/firstboot/verify_firstboot.py
```

JSONをstdoutへ出し、上記3不一致のため**exit 2**で終了するのがこの固定bundleの期待結果。
`complete_validation_accepted`はfalseのままである。verification.jsonはsubprocess出力を検査してから保存した。
