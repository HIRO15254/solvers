# VM07 source07: 通常検証とcodec buildの独立照合

回収した2 bundle本体と全642 payloadのSHA-256、selected evidence 630 files、
build結果の8 stages、source・supervisor・出力log・4 binariesの結び付きを独立に照合し、
**この範囲の検証はpassed**となった。[再計算JSON](vm07-report.json)にhashと原VM path、
raw bundle/memberを記録する。これはbuild/checkの検証であり、性能差・外部参照品質・R1受入の認定ではない。

## 実行範囲と実集計

VM `solvers-r1-20260925-07`、boot `0727ebb3-36dd-4fae-b978-c629114703ed`、
AMD EPYC 7B12 / 8 logical CPUs、Rust/Cargo 1.97.0。
実行期間は2026-09-25 19:59:44–20:08:45 UTC、driverの全工程541.82秒。
時間は当該stageのcompileを含む観測であり、性能比較の反復測定ではない。

| stage | supervisor時間（秒） | 照合結果 |
|---|---:|---|
| toolchain / host | 0.27 | 完了、host/toolchain log保存 |
| cargo fmt --all --check | 1.30 | passed |
| cargo clippy --locked --workspace --all-targets -- -D warnings | 36.72 | passed |
| cargo test --locked --workspace -- --test-threads=2 | 326.89 | **901 passed / 0 failed / 31 ignored** |
| Python tools/tests | 5.14 | **29 passed / 0 failed / 3 skipped**（全32件） |
| release CLI + saved-profile audit example | 121.35 | 2 binariesのbyte hash一致 |
| release current codec example | 15.73 | binaryのbyte hash一致 |
| release codec-baseline example | 32.73 | binaryのbyte hash一致 |

RustはUTF-8 stdoutの54個の`test result`、各test行、`running N tests`を相互照合した。
901 passedに含まれる新しいSOL互換性6テストは、canonical postcard bytes、旧Vec serializerとの
varint境界を含むbyte一致、JSON配列・整数契約、legacy v1 payload decodeとv1 containerの拒否、
v3 chunkのbytes/hash/圧縮とroundtrip、切れた入力・過大lengthの拒否を検査し、すべてpassed。
名前の全リストはJSONの`workspace_tests.compatibility_tests_passed`に保持する。
31 ignoredはこの実行では未実行。Pythonの3 skippedはWindows Job/suspended launch専用で、
Linuxで成功したものとして数えていない。

8 stagesとも順序・argv/cwd・時刻区間・有限資源枠・exit 0・identity before/after一致・
`cleanup_complete=true`・最終process集合空を確認した。stdout/stderr/samplesの全hashも一致した。
supervisorはLinux process groupであり、単独ではsetsidやsupervisor自身の強制終了を包摂しない。
外側systemdの`RuntimeMaxSec=7500`、`MemoryMax=48G`、`KillMode=control-group`等を指定する
setup scriptは保存されているが、この2 bundleに実systemd propertyの独立観測は含まれない。

## Sourceとbinaryの同一性

source07 archiveは1,314,820 bytes、SHA-256
`a6d346a033cf90f68adf131c7a14f5a754417cf0d952e1008d5736e7e9d6dd2a`。
archive内332 filesを展開済みsourceとdriverのbuild-input identityへbyte単位で照合した。
codec baseline archiveは1,165,415 bytes、SHA-256
`3de4afef13082dca9e17c38c9cc5f5d1734855f017d812ecb04e9cdbe0f7da27`。
240 filesに、currentと同じcodec exampleを1 file追加した241 filesを照合した。
これはbyte serdeの比較用baselineであり、R1初期baseline9632ではない。

`crates/`とCargo/config対象のbyte差は3 paths。production変更は`formats/src/sol.rs`だけで、
`formats/tests/sol_byte_serde.rs`が追加されている。
残る`holdem/tests/oracle_river.rs`は**改行だけの差**だった。baselineとGit `88ffa5d` blobは
16,786 bytes、SHA `05fbd787da6c12d286b7ba0967b2581bb603ea7b14194af7fb342b081b613de1`。
source07は17,272 bytes、SHA `aa4e32171e7b83f9ff9b34759733c0611a7aa77e8bf437f23a9e6cdad6de1861`。
486個のCRLFをLFへ変えた比較では全byte一致する。元bytes/hashは変更・正規化せず保持した。
Git blobとの照合はこの報告作成時に実施し、再検証器は両archiveと固定したLF版hashを検査する。

driverは既存targetを拒否し、source・boot・driver・supervisorを各stage前後と最終時点で検査する。
このdriverの固定bytesとpassed結果を照合した。sourceを後から変更したlocal checkoutを、
このVM上で検証したsourceと同一とみなさない。

| retained binary | bytes | raw member（build bundle） |
|---|---:|---|
| current/release/solvers | 8,868,616 | files/00000614 |
| current/release/examples/hu_saved_profile_audit | 4,248,672 | files/00000000 |
| current/release/examples/sol_codec_bench | 1,709,936 | files/00000615 |
| baseline/release/examples/sol_codec_bench | 1,710,648 | files/00000242 |

4 binariesとrustcはraw bundleから再hashした。この2 bundleにCargo・Python・bash本体はないため、
この検証ではsupervisorの前後identity一致までを確認する。
後続の[codec検証](../codec/vm07-report.md)は追加回収した3実体も再hashしている。
終了後systemd照会もcodec supplementに保持するが、時刻空・制限infinityのため実設定を確認できていない。
この2 bundleの認定範囲へ遡って含めない。

## Retentionの境界

| bundle | archive bytes | payload数 | SHA-256 |
|---|---:|---:|---|
| [checks](../../../runs/r1-cloud/evidence-vm07-checks.tar.gz) | 2,562,647 | 25 | `92ec7553e89a2b822622395f902f16ab2cd65e67fdfcfc688a3f7cb6edfdf6fb` |
| [build](../../../runs/r1-cloud/evidence-vm07-build.tar.gz) | 9,327,291 | 617 | `7c754fd93e2b503faa7d3fbbf0fbe6df039156404af1b2928ccd22589bec70ac` |

archive内manifestとsidecarは完全一致、全payloadがsize/hash一致、skip 0。
重複する21 VM pathsもbytes一致し、642 payloadは621種類のVM pathsを表す。
build collectorは生成cache `/opt/r1/current/.cache`を対象外として明示している。
大きいbinary/source archiveはignored `runs/`内のbundleで可用であり、Gitだけでは再検証できない。
[retention](vm07-complete/retention.json)と[compact-index](vm07-complete/compact-index.json)は変更していない。

汎用retainerの`ready=false`も維持する。required欠測94件は、source07内に保存された歴史的JSON
5件の参照で、内訳は`preempted-05` 7、`preempted-06` 16、`transfers` 38、`current-report` 20、
`vm06-comparison-report` 13。実VM07の8 stage・source・結果binaryを指すrequired欠測ではない。
ほかにoptional未解決52件がある。この監査は歴史的参照を隠蔽・修復・認定せず、VM07の
build/checkに必要な参照をraw bundleへ直接解決した範囲だけをpassedとする。

再検証（ローカルraw 2 bundleとsidecarが必要、Rust・solver・cloud操作なし）:

```text
python experiments/hu-postflop-r1/validation/verify-vm07.py
```

`--out NEW_JSON`は新規fileにのみ書き、既存reportやimmutable evidenceへの上書きを拒否する。
保存コードは実行せず、archiveも展開しない。検証器のSHAはreportに保持している。
