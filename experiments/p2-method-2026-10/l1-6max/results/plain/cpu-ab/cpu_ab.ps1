# Rough A/B while the record runs: one solver thread at Idle priority, compare process CPU time.
param([string[]]$Bins = @('4cc17dc', 'plain', 'inter'), [int]$Iterations = 50, [string]$Out = '.cache/p2-trunk/l1-6max/cpu-ab')
New-Item -ItemType Directory -Force $Out | Out-Null
$ehs = "$env:LOCALAPPDATA\solvers\ehs2\v2-f32-t32-r32.postcard"
$order = $Bins + ($Bins[($Bins.Count - 1)..0])
$i = 0
foreach ($bin in $order) {
    $i++
    $exe = ".cache/p2-trunk/l1-core/bin/trunk_solve-$bin.exe"
    $copy = "$Out/trunk_solve-ab-$bin.exe"
    Copy-Item $exe $copy -Force
    $p = Start-Process $copy -ArgumentList '--config', 'examples/bench/hu_20bb_postflop.toml', '--leaf-model', 'l1', '--ehs2-cache', $ehs,
        '--threads', '1', '--iterations', "$Iterations", '--eval-every', '0', '--print-every', '0', '--output', "$Out/$i-$bin.json" `
        -NoNewWindow -PassThru -RedirectStandardOutput "$Out/$i-$bin.log" -RedirectStandardError "$Out/$i-$bin.err"
    try { $p.PriorityClass = 'Idle' } catch {}
    $p.WaitForExit()
    "{0} {1} cpu {2:N2} s exit {3}" -f $i, $bin, $p.TotalProcessorTime.TotalSeconds, $p.ExitCode
}
