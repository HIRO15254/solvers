# R1: HU Postflopの検証・入出力・資源改善

[全体計画 §5](solver-implementation-plan.jp.md)と[R0引継ぎ](hu-postflop-r0/handoff.md)を実行する作業票。
状態・担当・実行時の待ち関係は[Linear](../status.jp.md)に置く。

## 利用者決定と成果

2026-09-25の依頼により、R1ではNLH HU Postflopを先行し、他ゲームへの拡張性を残しながら、
入出力形式と、同等のExploitabilityを持つ解までの時間・メモリを改善する。
開発段階のため破壊的な仕様変更を許容する。互換性維持自体を最適化の制約にしないが、
変更する公開契約はAGENTS.mdの全同期対象を同じ変更で揃え、旧形式の扱いを明示する。
GCP Spot VMでの実験は、初期20米ドルと2026-09-26の2回の追加各10米ドルの許可により
**累計40米ドル以内**とする。計算時間より節約を優先し、検証・buildには必要なメモリを満たす
小型Spot VMを使い、32 vCPUの比較機は検証後の短時間計測に限定する。ローカルの他実験との競合も利用理由に含む。

2026-09-26の追加要求により、HU Postflopは各席の初期レンジで正weightかつ開始boardと非衝突の
comboだけを厳密に保持し、root local IDを後続dealでも固定して省メモリ化する。正weightを切る閾値は設けない。
同日の32 vCPU試験要求を含め、同一32 vCPU host・同一bootでcompactのthread数を
1・2・4・8・16・32に変え、denseの1 thread対照を加えた7条件で比較する。
4 caseそれぞれにcompact 1 threadのpilotを1回置き、その後は各条件のwarmup 1回と
測定3回、合計112標本と4 pilotを実行する。`threads = 1`を直列のexact対照とする。
F32の保持support上のstrategy/state/CFVとEV/BRの一致を要求し、I16のdense/compact bit一致とは区別する。
chanceを含まない木のaction並列化は、rootのstorage要素数`W`と実行poolのworker数`P`から
`grain = clamp(ceil(W / (4 * P)), 4096, 65536)`を決める。子subtreeのstorage要素数が
grain以上の枝を2本以上持つnodeで分割し、単一の大枝にも深さ制限なく進む。
`threads = 1`は直列対照で、thread数を増やした際の速度向上は保証しない。
公開仕様に速度閾値を追加せず、品質一致と実測条件を固定した実験証拠で判断する。
この分割方式は[River/F32の同一boot比較](../../experiments/hu-postflop-r1/action-scaling/source06/report.jp.md)を根拠に採用する。
1000反復固定の新旧48実行でstrategy/state/CFVと品質bitsが一致し、事前固定した採用guardを満たした。
16 threadsが最速で32 threadsはそれより遅く、2 threadsでは旧実装より遅かった結果も保持する。
これは当該条件での実装選択の根拠であり、他条件の性能やR1全体の受入を認定しない。

R0の準備完了を開始条件とし、対象sourceを`9632d8b`として引き継ぐ。
これは基準測定の成功を意味しない。測定ごとに実際のsource/config/binary/hostを記録する。
R1は参照取得、汎用境界、独立照合、抽象化、測定・認定に加えて、下記T1-07/08を含む。
R2の実用範囲拡張、教師用action EV、GUI、MLの完成は本票へ追加しない。

## 作業単位と成果物

| 作業 | 行うこと・成果物 | 受入に必要な証拠 |
|---|---|---|
| T1-01 | 24候補を取得・照合し、日常/拡張fixtureと比較手順を固定。凍結oracleの外側に小HU adapterを追加 | 両range・全継続木・rake/utility・参照精度と欠測。独立EV/BRの照合。参照条件不明を同一ゲームと認定しない |
| T1-02 | 配札、観測、私的履歴、phase、legal action、精算の境界をNLHE実装と対応付ける | NLHE kernelへのlowering条件、完全記憶・相関・隠された行動の適用/拒否条件 |
| T1-03 | 縮小Stud/Drawとsplit potのprototypeを共通境界で表現 | 独立全列挙/期待値、情報集合、手札次元変更、記憶、非線形utilityの検査 |
| T1-04 | bet/hand抽象化のID・写像・chance重み・評価範囲を定義 | 粗密対照と元表現への復元。lossless同型処理とlossy抽象化を区別 |
| T1-05 | 外部supervisorをtoyで較正し、基準sourceの全工程を測定 | 停止・子process掃除、process peakと静的見積りの区別、初期化/CFR/BR/保存/読戻し、checkpoint再開 |
| T1-06 | 基準結果から比較用閾値版を先に固定し、最後に受入範囲を認定 | 対象・単位・不等号・validator版、零和/一般和、欠測と非認定範囲。T1-07/08の比較結果を含む |
| T1-07 | 入力の自己完結性、保存時コピー、成果物構造と部分読込みを改善 | 外部tree sourceを含むroundtrip、破損/旧版の扱い、保存前/保存後profileの区別。容量・保存/読込時間・peakを測定 |
| T1-08 | 不要な構築・重複EV/BR・checkpoint・作業領域を削減し、必要ならkernelを改善 | 同じ有限ゲーム・品質目標で時間とprocess peakをA/B測定。CFR/oracle/storage/parallelの回帰なし |

T1-07のhand領域変更は `.sol` v4 / HU共通checkpoint v2の破壊的境界とする。
旧SOL v1–3およびcheckpoint v1を明示拒否し、新規solveで作り直す。旧checkpointのresume移行は行わない。
toy/preflop HUも共通checkpoint container v2を使い、Multiwayの別containerは維持する。
T1-08では狭い・非対称・重複指定とゼロ/微小正weight・board衝突・iso on/offの各rangeで、
dense比較経路とのnormalizer・root weight・strategy/state・EV/BR一致と永続領域の削減を検査する。
過去の研究記録のv3や全1,326-combo計測は当時の証拠として保持する。

T1-01/02とT1-05の監視器準備は並行可能。T1-03はT1-02を使う。
T1-04/05の本計算・T1-06の認定は全体計画の前提を満たすcaseから進める。
T1-07/08のコード調査・独立testは基準測定と並行可能だが、変更前sourceを保持し、
性能比較の測定条件と品質目標を変更後の結果を見て都合よく選び直さない。
T1-06は比較開始前の基準版発行と、全成果を照合する最終認定を分ける。

## 同等品質での比較

1. game/両range/board/pot/stack/menu/utility/chance/抽象化と反復停止条件を固定する。
   IOや作業領域だけの変更は平均戦略・EV/BRの一致を検査する。
2. 零和では平均profileに対する正確なBRから`Exploitability = NashConv / 2`を測る。
   pot比の分母は開始pot。rake等の一般和ではseat別gainとNashConvを用い、零和保証を付けない。
3. 固定反復で値が一致する改善は同じ反復数でも比較する。更新式・数値表現・収束挙動を変える場合は、
   比較開始前に固定した同じ品質目標へ到達するまでの時間・peak・反復数を比較する。
   未到達、timeout、resource_exceededを高速な成功として扱わない。
4. 同一VM/CPU/thread/build条件で基準と改造を交互に複数回実行する。初期化・CFR・BR・保存・読戻しと
   process全体を分け、warm/cold、計測分解能とばらつきを記録する。
   `.sol`のsummary読戻しだけを量子化profileのBR再評価と呼ばない。
5. 本体のメモリ削減、出力生成時peak、保存容量、部分照会の改善を別々に示す。
   小ケースの成功だけで全24候補やFlop全範囲の品質・性能を認定しない。

固定条件・生ログ・小さい結果・validator・採否は`experiments/hu-postflop-r1/<experiment>/`へ保存する。
新規実行出力は`runs/`、再生成可能cacheは`.cache/`、Cargo出力だけを`target/`へ置く。

## 実行資源とクラウド費用

ローカル実行は[R0実行票](hu-postflop-r0/local-run-plan.md)の直前資源確認と停止器較正を引き継ぐ。
他実験が稼働する場合は重いローカルbuild/solveを重ねず、許可済みのSpot枠を使える。
VM起動前に一次料金、最大稼働期間、disk、IP、転送、保存、税等の予備を含めた予約額を記録する。
既支出と未精算の予約額を足し、追加予約後も40米ドルを超えない場合だけ起動する。
請求の反映遅れを未使用予算と扱わない。自動再試行で予約額を増やさない。

費用の予約・精算と資源操作の記録は[予約台帳](../../experiments/hu-postflop-r1/cloud/budget.json)へ置く。
32 vCPU試験はVM08の4 vCPUで正しさを検証した後、同じdiskを使って最大90分だけ実行する。
安い`e2-highcpu-32`を先に試し、quota・空きがなければ`n2-highcpu-32`を使える。
両方を覆う通常料金上限1.15 USD/h×1.5hと予備1 USDから追加3 USDを事前予約する。
cloudの絶対STOP期限は拡大時刻+90分と元期限の早い方に設定し、実際の計測期限はさらに前とする。
追加許可・予約自体を起動・請求・性能測定の完了とは扱わない。

単一Spot VMに絶対終了時刻を設定し、boot diskは明示削除時にauto-deleteとする。
Spot回収による未回収証拠の喪失を受け、回収時の動作はSTOPも使用できる。
STOPでは元の計算期限を延長せず、停止diskの有限保持費用を予約に含め、
同じ実験窓内に回収・明示削除する。再起動にも残時間と残予算の照合を要する。
自分が作成したinstance/disk/IPだけをIDで追跡し、終了後に残存物を確認する。
成果物は削除前に回収・hash照合する。Spot回収は中断として記録し、再試行前に予算を再計算する。
利用者は本依頼に関連するファイル転送を全面許可済み。参照入力の転送可否を追加の待ち条件にしない。
budget alertだけを強制上限とせず、実行時間・転送量・保存量の有限枠で支出を制御する。

## 必須検証と終了条件

通常コード変更には`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、
`cargo test --workspace`を実行する。Pythonには`python -m unittest discover -s tools/tests -v`、
文書には`python tools/check_docs.py`。CFR/BR・次元遷移・保存・OS停止は変更に対応する追加testを実行する。
Flop/iso/storage等のignored試験は採用範囲に対応するものを明示的に実行し、未実行と区別する。

終了にはT1-06の認定、汎用境界の異なる情報構造による検証、入力/成果物契約の同期、
同等品質での時間・メモリ・入出力改善の実測と再現証拠が必要。
24候補の未取得や品質未判定を、テスト成功や改善した別fixtureで埋めない。
