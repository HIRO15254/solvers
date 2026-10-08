"""Create the Japanese evidence report and retention manifest after checks."""
import hashlib
import json
from pathlib import Path
import statistics
import subprocess

ROOT = Path(__file__).resolve().parents[4]
EXP = Path(__file__).resolve().parents[1]
result = json.loads((EXP / 'result.json').read_text(encoding='utf-8'))
code_commit = subprocess.check_output(['git', 'log', '-1', '--format=%H', '--', 'crates/hu-engine/src/storage.rs'], cwd=ROOT, text=True).strip()
code_blob = subprocess.check_output(['git', 'hash-object', 'crates/hu-engine/src/storage.rs'], cwd=ROOT, text=True).strip()
machine = json.loads((EXP / 'machine.json').read_text(encoding='utf-8-sig'))
retained = {}
for directory in ['raw', 'configs', 'scripts']:
    for path in sorted((EXP / directory).rglob('*')):
        if path.is_file() and '__pycache__' not in str(path):
            retained[path.relative_to(EXP).as_posix()] = hashlib.sha256(path.read_bytes()).hexdigest()
for name in ['result.json', 'machine.json']:
    retained[name] = hashlib.sha256((EXP / name).read_bytes()).hexdigest()
scratch = []
for path in sorted((ROOT / 'runs/i16-kernels').rglob('solution.sol')):
    scratch.append({'path': str(path.relative_to(ROOT)), 'bytes': path.stat().st_size,
                    'sha256': hashlib.sha256(path.read_bytes()).hexdigest(),
                    'availability': 'machine-local ignored scratch; not Git-backed; reproducible from retained config and source'})
manifest = {'baseCommit': 'c09c0af76d7b2cf999f1082644d8080441654dd6',
            'implementationCommit': code_commit, 'implementationStorageBlob': code_blob,
            'binaryAndBuildSha256': result['sha256'], 'retainedSha256': retained,
            'scratchSolutions': scratch,
            'candidateRuns': 'raw/chunk-candidate is an interrupted exploratory series, excluded from acceptance medians',
            'rejectedQuantizers': ['raw/reciprocal-scale-candidate: Flop 190 -> 200 (+5.263%)', 'raw/direct-max-candidate: Turn i16 310 -> 320 (+3.226%), Flop pair aborted'],
            'linear': 'SOL-32 (C1 scope only; parent issue also includes other work)'}
(EXP / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8')
speed = '\n'.join(f'| {r["case"]} | {r["storage"]} | {len(r["old"]["samples"])} | {r["old"]["median"]:.6f} | {r["new"]["median"]:.6f} | {r["ratio"]:.3f} | {(1-r["ratio"])*100:.1f}% |' for r in result['speed'])
quality = '\n'.join(f'| {r["case"]} | {r["storage"]} | {r["old"]["iteration"]} | {r["new"]["iteration"]} | {(r["ratio"]-1)*100:+.1f}% | {r["old"]["nash_conv"]/2/r["potBB"]*100:.5f}% | {r["new"]["nash_conv"]/2/r["potBB"]*100:.5f}% |' for r in result['convergence'])
controls = {r['case']: (r['ratio'] - 1) * 100 for r in result['speed'] if r['storage'] == 'f32'}
probe = [[float(v) for v in line.split()] for line in (EXP / 'raw/conversion-probe.txt').read_text().splitlines()]
probe_rows = '\n'.join(f'| {label} | {statistics.median(r[2] for r in probe if r[1] == index):.6f} | {statistics.median(r[2] for r in probe if r[1] == index) / 397800000 * 1e9:.3f} |' for index, label in enumerate(['旧quantizer（max走査込み）', '8要素chunkで量子化（maxは更新中に算出）', 'flat loopで量子化（採用、maxは更新中に算出）']))
report = f'''# P1 i16 storage kernelの融合・vector化（C1、2026-10-09）

目的は、i16 / i16-f32avgのmemory削減を保ちながら1 iterationを速くし、同じ木・scheduleで0.1% potまでの反復数を悪化させないこと。
関連はLinear SOL-32のC1、[P1規範](../../../docs/hu-postflop.jp.md)第4節。
再現状態は`verified`。集計は[result.json](result.json)、保持物の識別は[manifest.json](manifest.json)。

## Source・変更

- base: `c09c0af76d7b2cf999f1082644d8080441654dd6`。
- 実装commit: `{code_commit}`。`storage.rs`のGit blob: `{code_blob}`。
  実行時はこのcodeのdirty worktreeからbuildし、上記commitで同じsourceを固定した。報告・script・結果は後続の証拠commitに置く。
- 旧binaryは変更前のclean baseから`target/old`へbuildした。新binaryは`target/new`。4 binaryとCargo.lock・build設定のSHA-256はresult/manifestにある。
- dequantize、discount/add、max-absを1 passへ融合し、8 laneの独立maximaを持つ。floorの有無はloop外で選ぶ。
- 2 pass目のquantizeはscaleの逆数を1回求め、multiply＋ties-to-evenで丸める。逆数と積だけf64にして、f32の積の丸めが半整数tieを作るのを避ける。32,000のheadroomと全0 blockのscale 1.0を保つ。
- f32値をf64の積で丸め、i32→i16へ変換する。block最大値による範囲の証明を付けたunchecked i32変換を使う。NaNを0へ置き換え、scaleがunderflowするか逆数がf32の上限を超えると、従来同様の飽和変換へfallbackする。
- scratchは最大訪問nodeの長さまでだけ伸ばす。小さいnodeへ移るときの縮小・再拡張をなくす。平均resetはweighted入力から直接量子化する。
- I16Storage / I16View / MixedStorage / MixedViewが共有するi16更新だけを変更した。f32更新、評価用のf64平均正規化、公開API、checkpoint / .sol形式は変更していない。cfr-refも変更していない。

## 条件・再現

- {machine['os']} {machine['osVersion']}、{machine['cpu'].strip()}、{machine['cores']} core / {machine['logicalProcessors']} logical CPU、RAM {machine['ramBytes']/2**30:.2f} GiB。
- {machine['rustc'].splitlines()[0]}、{machine['cargo']}。target-cpu=native、AVX2、thin LTO、codegen-units=1。
- 8 threads。他のagentの計算とmachineを共有し、CPU affinityは固定していない。常にold→newを交互に実行し中央値を使った。
- throughput: warmup 5、evals 0。c_turn2は30 iteration×各9回、c_flop1は15 iteration×各3回。
  Turnは計時区間が短いため9回に増やした。時刻・順序・全引数・exit codeは`raw/*.command.json`、生のbench JSONとlogも保持する。
- speed入力はbrief指定のmain checkout内の絶対pathをread-onlyで使用。内容は[configs/c_turn2.toml](configs/c_turn2.toml)と[configs/c_flop1.toml](configs/c_flop1.toml)へ保存した。
- convergence入力はstorageだけを指定値にし、target=0.1%pot、check_every=10、final_checkpoint=falseにしたcopy。全6 runは正常終了しtargetへ到達した。
  potはTurn 22.5 BB、Flop 5.5 BB。rakeありなので`NashConv / 2 / pot`はbrief指定の補助指標として使い、零和の保証とは扱わない。

実際のbuildと計測command（repository rootから。old buildはcodeを編集する前）:

```powershell
cargo build --release -p cli --target-dir target/old
cargo build --release -p hu-postflop --example p1_bench --target-dir target/old
# 実装を変更した後
cargo build --release -p cli --target-dir target/new
cargo build --release -p hu-postflop --example p1_bench --target-dir target/new
python -X utf8 experiments/p1-efficiency-2026-10/i16-kernels-20261009/scripts/probe.py
python -X utf8 experiments/p1-efficiency-2026-10/i16-kernels-20261009/scripts/inspect_assembly.py
python -X utf8 experiments/p1-efficiency-2026-10/i16-kernels-20261009/scripts/measure.py speed --reps 3 --turn-reps 9
python -X utf8 experiments/p1-efficiency-2026-10/i16-kernels-20261009/scripts/measure.py convergence --run-group f64-final
python -X utf8 experiments/p1-efficiency-2026-10/i16-kernels-20261009/scripts/measure.py summarize
```

[measure.py](scripts/measure.py)は各binaryへ次の引数を渡す（完全な絶対pathは各command JSON）:

```text
p1_bench CONFIG --threads 8 --warmup 5 --iters 30 --evals 0 --storage i16 --json RESULT
p1_bench CONFIG --threads 8 --warmup 5 --iters 30 --evals 0 --storage i16-f32avg --json RESULT
p1_bench CONFIG --threads 8 --warmup 5 --iters 30 --evals 0 --storage f32 --json RESULT
# c_flop1だけ --iters 15
solvers solve CONFIG_COPY --out runs/i16-kernels/f64-final/solve_CASE_STORAGE_VERSION --threads 8
```

## 結果

`secsPerIter`の中央値（秒。短縮率が負なら計時増）:

| config | storage | 各版の回数 | old | new | new/old | 短縮率 |
|---|---|---:|---:|---:|---:|---:|
{speed}

i16とmixedは両configで改善した。f32 controlはTurn {controls['c_turn2']:+.1f}%、Flop {controls['c_flop1']:+.1f}%で、共有machineの計時の揺れの範囲と判断した。
validatorのcontrol許容幅は±5%。f32 sourceは変更していない。絶対時間はbriefの過去測定とは比較しない。

最初の評価境界で0.1%へ到達した反復数（補間なし）:

| config | storage | old | new | 反復数の増減 | old停止時 %pot | new停止時 %pot |
|---|---|---:|---:|---:|---:|---:|
{quality}

各runの全progress、events、run.json、manifest、実効run.tomlを`raw/solve_*`へ保存した。
この3比較に限って新の反復数は旧の+3%以内だった。任意の木や深い目標への収束保証は追加しない。

## Loopの選択・assembly

[probe.py](scripts/probe.py)でbaseと実装からquantizerを抽出し、3,978要素×100,000回をold→chunk→flatで交互に3回測った。
単一thread、native最適化build。probeの中央値（maxは融合passで算出済みなので新quantizerには走査を含めない）:

| 方式 | 合計秒 | ns/element |
|---|---:|---:|
{probe_rows}

- chunk conversionはLLVMが複数chunkをまたぐshuffleを増やしたため不採用。flat conversionを採用した。
- 飽和`as i32`の初期案は丸めまでvector化してもconversionがscalarになった。範囲を証明したconversionでpacked命令になった。
- pre-LTOのcrate assemblyにはscalar codeが残るため、それだけで最終loopを判定しない。
  実際に測ったlinked p1_bench binaryを[inspect_assembly.py](scripts/inspect_assembly.py)で逆assemblyし、[抜粋](raw/assembly.txt)を保存した。
  融合passは`vpmovsxwd / vcvtdq2ps / vmulps / vaddps / vmaxps`、quantizeは`vcvtps2pd / vmulpd / vroundpd / vcvttpd2dq`とnarrowing/storeが並ぶ。
- `raw/chunk-candidate/`は初期chunk案の途中までのfull-solver計測。停止した未完了の探索であり、上の受入中央値には含めない。
- f32で`scale.recip()`を使った案はFlop 190→200（+5.263%）、`32000/max_abs`を使った案はTurn i16 310→320（+3.226%）で不採用。
  [前者の記録](raw/reciprocal-scale-candidate/quality.json)・[後者の記録](raw/direct-max-candidate/quality.json)とsource patchを保持した。後者のFlop計測はTurn不合格で中断した。
  f64の逆数と積を使う最終案では、指定3比較が+3%以内に収まった。

## 検証・判断

fmt、clippy、workspace test（984 passed、0 failed、40 ignored）が通った。memory pressureで最初の並列compileが失敗したため、build jobs=1で再実行した。
候補のworkspace testはtest threads=1、最終sourceのtestはtest threads=4で行った。
最終sourceのworkspace test、36件のstorage/engine unit test、clippyのlogは`raw/checks/`にある。
新testはdiscount、floor、reset、32,000 headroom、0 block、SIMD chunk/tail、offset、scratch再利用、subnormal / extreme / nonfiniteを検査する。
既存のview一致、3 backend、独立oracle、保存/resume、thread決定性のtestも通った。

```powershell
cargo fmt --all --check
$env:CARGO_BUILD_JOBS = '1'
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace -- --test-threads=4
python tools/check_docs.py
python -X utf8 experiments/p1-efficiency-2026-10/i16-kernels-20261009/scripts/verify.py
```

C1を採用する。i16 / mixedの反復時間を両木で短縮し、指定3比較の反復数は+3%以内、f32 controlは±5%以内だった。
SOL-32全体の完了を示す報告ではなく、このkernel変更の受入証拠である。

生JSON・小さいrun証拠・入力・scriptはGitへ保持する。solverの大きい`.sol`はignoredのmachine-local scratchであり、Git-backed evidenceとは扱わない。
場所・size・SHA-256・availabilityはmanifestにある。build binaryもlocal Cargo出力で、sourceとcommandから再生成する。
'''
(EXP / 'README.md').write_text(report, encoding='utf-8')
print('Wrote README.md and manifest.json')
