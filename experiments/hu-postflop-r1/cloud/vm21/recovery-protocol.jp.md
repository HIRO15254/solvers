# VM21 回収・転送の上限

解析は元の `/opt/r1/sparse-rank-proof01` に対してGCPの2 CPUで実行し、報告を外部の `/opt/r1/sparse-rank-analysis01.json` に保存する。元proofへrecovery fileを追加しない。`solvers-r1-vm21-analyze2` はRuntimeMaxSec120、MemoryMax2G、Swap0、KillMode=control-groupで実行する。全38 solve未完了なら性能未評価のまま扱う。

`recover.sh` はbuild2・measure32・analyze2の3 unitについてinactive/failed・MainPID0・残存cgroup processなしを要求する。PACKAGE `/opt/r1/sparse-rank-package`、元proof、既存build/measure wrapperと外部analysis原本を保存する。全memberのbytes/SHAと変更前後のstatを検査し、GCPで作成したarchive全streamを再読取りして原本との一致を確認する。成功した元memberを消さず、partial/failed証拠もそのまま保存する。失敗archiveを上書きしない。

archiveとmember manifest・SHA sidecarを合わせて**255 MiB以下**とする。`split-on-cloud.py` は最大48 MiBのpartsを排他作成し、連結SHAを確認する。parts、member manifest、SHA sidecar、parts SHA、recovery receipt、存在する4つのanalysis出力を正確なtransport一覧へ固定する。全転送file payloadは**256 MiB以下**。analysis等の各sidecarは1 MiB以下、transport JSONは64 KiB以下。残り**64 MiBを小さなcontrol応答とSSH/SCP等の余裕**に充て、総予約上限を320 MiBとする。これはwire bytesの実測や請求額ではない。

すべてのVM21 file downloadは `download.py` を使う。次の固定split commandを`capture-command.py --label split01 --timeout ... --`で実行したreceiptを読み取る。

```text
compute ssh solvers-r1-20260928-21 --project=solvers-abstraction-20260723 --zone=us-central1-b --command="sudo python3 -B /opt/r1/sparse-rank-package/experiments/hu-postflop-r1/cloud/vm21/split-on-cloud.py" --quiet
```

`download.py --label proof01` は一覧全体を一度取得する。`--files`を指定する場合も一覧中の正確なbasenameだけを認める。各要求を排他的lockで直列化し、1 file / 1 SDK SCPを順番に実行し、各fileの開始直前にその予定bytesをfsync済みintentへ計上する。完了・途中失敗・timeout・controller中断のいずれでもintent全額を累積し、返却しない。未着手のsuffix filesは計上せず、0 bytesの既知sidecarは0 bytesのintentとしてSCP後にempty SHAを確認する。累積payload+64 MiBが320 MiBを超える要求は開始しない。自動retryはなく、再要求には新しいlabelと新しい保存先、残枠の再検査が必要。controllerがcrashしてlockが残った場合は既存processとintentを確認してから扱う。

保存先はローカル `downloads/<label>/` の新しいdirectory。既存partの上書き、remote wildcard、再帰SCP、任意pathは許可しない。各fileのサイズ・SHAを転送後に検証する。各SCPは最大180秒かつ原STOPまでの残り−60秒以内、開始時の残りは90秒超を要求する。失敗時も元の絶対STOPは変えない。

control payloadは個々4 MiB以下・累計8 MiB以下に抑え、残りの64 MiB枠を将来の小さな制御応答やプロトコル等の余裕として残す。rootはstatus/analysis応答を有限サイズに制限し、wrapper外のfile downloadを行わない。`download.py`は既存stdout/stderrのサイズも照合する。wire再送などを完全に観測したとの主張はしない。

最後に `check-download.py --directory downloads/proof01 --bytes N --sha256 SHA --members PAYLOAD_COUNT` で、sidecarと4〜6等のpartsの連結圧縮stream SHAを照合する。`--members`はpayload file数で、埋込recovery manifestの1件を含まない。ローカルarchive展開・native実行は行わない。検証後に元VMとboot diskを削除し、不在・元STOP内・disk24時間以内をSDK記録で確認する。Git保全にはparts、manifest/hash、compact reader reportを一緒に含める。

`recover.sh`のshell構文はGCPで実行前に`bash -n`する。ローカルではPython ASTと小さな純粋guard testだけを行い、archiveを生成・読取り・展開しない。
