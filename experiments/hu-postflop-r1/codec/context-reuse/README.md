# SOL chunk圧縮contextの再利用候補

[context-reuse.patch](context-reuse.patch)は、固定candidate
`2fecc099b9911511a0938fb2700bbb124bc1046e`への研究用差分である。
node chunk専用の`Option<CCtx>`をartifactごとに持ち、初回だけlevel 0・空dictionaryで
初期化する。各frameの前に`SessionOnly`でresetし、checksumありの
`write_all`→`finish`を維持する。metadata圧縮、chunk境界、hash、sync、persistは変更しない。
既存`Encoder::new`と同じ`CCtx::create`を使い、allocation失敗時の挙動も維持する。

対象はCCtxの再利用だけである。zstd 0.13.3の`Encoder::with_context`も各回に
32 KiBの出力bufferを作るため、このbufferの確保回数は削減しない。
contextはchunk間で圧縮parametersと空dictionaryを保持するが、frame historyと
pledged source sizeは引き継がない。書込み・圧縮・finishの失敗時は処理を終了する。

## 準備したcopy

- 元source: `E:\codex-work\solvers\r1-writer-smoke-20260926\candidate-original`
- 研究source: `E:\codex-work\solvers\r1-context-reuse-20260926\source`
- 識別記録: 研究source内の`r1-context-reuse-source.json`
- 適用差分: 研究source内にも同一の`context-reuse.patch`を保持

元sourceの197 Cargo/cratesファイルを[固定pins](../write-phases/source-pins.json)に
全件照合してbyte copyし、変更後にも元sourceが不変であることを確認した。
変更は`crates/formats/src/sol_indexed.rs`だけであり、前後197件のsize/SHA-256と
明示したtoolchainのrustfmt実体識別子を上記記録に残した。この研究copy作成時点では
production worktreeのcratesは変更していない。

追加した2つのunit testは、空入力、32 KiB・128 KiB境界前後、固定pseudo-random入力、
large→small→同じlargeの連続frameを扱う。旧手順のfresh encoderとの全byte比較と
各frame単独decodeを確認する。既存のformats integration testsもそのまま残す。

## 固定sourceでの診断

[Windows debugの保持証拠](windows-debug-20260926/README.md)では、別々の空targetから
両exampleをbuildし、候補のformats test 77件とRiver/Turn/Flop各3交互ペアを検査した。
18回すべての再保存SOLは元入力と全byte一致し、canonicalとrootのbyte列も一致した。
基準197ファイルのpinsはGit `2fecc099`のblobにも全件照合した。

| 入力 | 基準の保存中央値 | 候補の保存中央値 | 差 |
|---|---:|---:|---:|
| River | 4.5621 ms | 4.1719 ms | −8.55% |
| Turn | 40.2415 ms | 36.3692 ms | −9.62% |
| Flop | 557.5716 ms | 490.3901 ms | −12.05% |

Flopの全processを含む標本peak中央値は257,024,000→259,747,840 bytesで増加した。
入力読込み・canonical生成・読戻し・console子processも含むためwriter単独のpeakではなく、
この結果からメモリ削減を主張しない。各標本と他streetの値も保持証拠に残す。

これは専有を確認していないWindows hostのdebug・少数回診断である。Linux releaseで
観測した旧Flop書込み回帰の解消や原因、solve全体の高速化、R1受入は認定しない。
正式な[Linux 126標本の計画](../write-phases/README.md)とも別に扱う。
各buildとformats testの初回はCargoが0で終了した後に子processが残り、監視失敗と
強制cleanupになった。同じcommandでの確認は正常終了し、生成exampleのhashも不変だった。
元の失敗記録は成功した確認記録と分けて保持する。

先行した共有targetでの試行は、基準sourceに旧計測版のbinaryが混入したため無効である。
source hashの照合だけではCargo生成物の同一性を保証しない。この失敗も保存し、
有効な診断には空の別target・固定binary・実行前後のbinary/input照合を用いた。

## 採用の確認範囲

上記の77 tests・18実行は研究copy時点の結果であり、その時点ではproduction writerに
適用していなかった。その後の固定候補にはcontext再利用と、windowを超える
large→small→large、write/finish途中のI/O失敗を含む4つの回帰testを組み込んだ。
その正確なsourceは[Linux保持証拠](linux-spot-20260926/README.md)のarchiveに残す。

[採用候補のWindows増分検証](../../validation/context-adoption-20260926/README.md)では、
199 source pinsの実行後一致、fmt/clippy成功、formats 79 tests成功を確認した。
tests初回はCargo 0でも子process残留により監視失敗となったため、その失敗と
同一command・生成binary不変の正常な確認実行を分けて保持する。
同じwriter候補のLinux全workspace testsは906件成功、31件ignored、release SIGINT試験は
1件成功した。releaseの3方式・72標本比較では保存byte列の一致を確認したが、
候補/bulkの保存中央値比はRiver 1.09749、Turn 0.94137、Flop 0.98294となり、
事前の採用screenを満たさなかった。**context再利用は本体へ採用せず、候補差分を戻した。**
この判断と後続のcompact hand layoutによる形式変更は別の変更である。

release比較では同一host・build条件、固定入力、交互実行、SOL/canonical完全一致を維持し、
他の重い実験が稼働するhostでは追加buildや性能測定を重ねない。
CCtx workspaceはartifactの終了まで保持されるため、確保回数削減をメモリ削減とはみなさない。
固定したcontext候補そのものはwire形式、CLI、既定値、依存関係を変更しない。
