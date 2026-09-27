# Process CPU時計による追加診断案

**静的なadapter生成と純テストのみ。Linuxでのcompile・実行・校正は未実施。**
元のCloud32計測が終了した後、別の有限診断を行うか判断するための準備である。
進行中のrunner・protocol・source・proofを変更しない。

`prepare.py`はCloud32の`solve.rs`（SHA-256
`63cba37a5b0f79dfed2ca0ddf879daada3f0fee094ee5f287f86bc1f37414d46`）を読み、
CPU時計の宣言、測定、`cpu.json`出力だけを追加する。逆変換が元bytesへ完全一致することを
要求する。fixture、workers/反復数の許容範囲、solver呼出しと順序、state書込み、
既存invocation/quality/resultのJSON書式は維持する。生成元・helper・生成物のpinsは
`provenance.json`、差分は`adapter.patch`に記録する。native toolは呼ばない。

## 測定する区間

追加の`cpu.json`は秒単位で次を記録する。

- `cfr_cpu_seconds`: CFRの全反復。
- `quality_cpu_seconds`: 既存quality区間全体。
- `ev_cpu_seconds[2]`、`br_cpu_seconds[2]`: 各seatの公開呼出し。
- `exploitability_cpu_seconds`: 公開exploitability呼出し全体。内部の3walkは分解しない。
- `cfr_wall_seconds`、`quality_wall_seconds`: 元adapterの対応wall timer値。

CFRとquality全体のCPU開始は既存`Instant::now()`の直前、CPU終了は既存wall elapsed読取の
直後である。CPU区間がwall区間をわずかに包み、境界のclock呼出し・assert・時刻変換の
費用が完全には除かれない。個別quality呼出しのCPU計測は開始eventの後からsolver戻り直後まで。
全体qualityには既存event出力と追加した内部時計の費用も含まれるため、個別CPU秒の合計と
一致することを合格条件にしない。合計14回のprocess clock読取りを追加する。
CPU時計はnanoseconds整数で差分を取り、最後に秒へ変換する。取得失敗、負値、不正nanoseconds、
overflow、逆行はいずれも明示的に停止する。

`CPU秒 / wall秒`は区間中に消費された論理CPU数の平均に相当する診断値で、workersで割った値も
補助表示できる。ただしRayonの待機時spinもprocess CPUへ含まれる。高い値だけで有効な計算や
帯域飽和を証明できず、低い値も逐次部分・待機・OSからの非スケジュールを区別しない。
16→32のCPU利用増とwall短縮の有無をまず比較する。SMTとメモリ帯域の分離はこの時計だけでは
できない。元の測定結果へCPU値を後付けしたり、別adapterの時間を同一標本として混ぜたりしない。

## Linux ABIの範囲と一次根拠

対象は`x86_64-unknown-linux-gnu`のLP64だけ。`target_os`、`target_arch`、`target_env`、
pointer widthをcompile guardし、C int=4B、C long=8B、timespec=16B/alignment8、
field offset 0/8をcompile-time assertする。Rustの`repr(C)`と`std::ffi::c_int/c_long`で
独立した小さいFFI宣言を置く。32bit、x32、Windows、muslへ一般化していない。

- [Linux man-pages: clock_gettime](https://man7.org/linux/man-pages/man3/clock_gettime.3.html)
  のSYNOPSIS/RETURN VALUEとCLOCK_PROCESS_CPUTIME_ID記述で、C関数宣言、成功0・失敗-1、
  当該processの全threadを合算する意味を確認した。glibc 2.17以降は別途`-lrt`を要しない。
- [Linux v6.16 UAPI time.h](https://github.com/torvalds/linux/blob/v6.16/include/uapi/linux/time.h#L48)
  は時計IDを2と定義する。syscallそのものではなくglibcの関数を呼ぶ。
- [GNU libc 2.42 x86 typesizes.h](https://github.com/bminor/glibc/blob/glibc-2.42/sysdeps/unix/sysv/linux/x86/bits/typesizes.h#L60)
  と[基礎型定義](https://github.com/bminor/glibc/blob/glibc-2.42/posix/bits/types.h#L100)
  でLP64のtime_tがsigned long、clockid_tがsigned intとなる型連鎖を確認した。
- [GNU libcの時刻型説明](https://sourceware.org/glibc/manual/2.40/html_node/Time-Types.html)
  はtimespecのtime_t秒・long nanosecondsとnanosecondsの範囲を定義する。

ヘッダーはABI事実の参照のみで、GNU libcやkernelの関数実装を移植していない。
この確認は実機リンク・clock動作・摂動の校正を代替しない。

## 再生成と次の受入境界

```text
python -B experiments/hu-postflop-r1/flop-scaling/flat-ev/cloud32/diagnostic/prepare.py
python -B -m unittest discover -s experiments/hu-postflop-r1/flop-scaling/flat-ev/cloud32/diagnostic -p test_prepare.py -v
```

凍結runnerはartifactを4ファイルに限定するため、追加`cpu.json`をそのまま渡すと拒否される。
将来実行する場合は別harnessと新しいproofが必要である。まず同一source/profileでのLinux compile、
短い境界clock費用とworker1/16/32のclock動作、元adapterとの全state/quality一致を確認し、
同じ入力・固定反復・CPU制限・bootで少数反復する有限手順を別途固定する。
現時点で追加build・実験の起動を許可する文書ではない。
