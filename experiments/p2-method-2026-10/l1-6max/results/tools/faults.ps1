# Run trunk_solve once and report its page faults, peak private bytes and CPU time.
param([string]$Bin = '4cc17dc', [string]$Config = 'examples/bench/6max_20bb.toml', [string]$Leaf = 'l0',
    [int]$Iterations = 10, [string]$Out = '.cache/p2-trunk/l1-6max/faults', [string[]]$Extra = @())
New-Item -ItemType Directory -Force $Out | Out-Null
$ehs = "$env:LOCALAPPDATA\solvers\ehs2\v2-f32-t32-r32.postcard"
$exe = if (Test-Path $Bin) { $Bin } else { ".cache/p2-trunk/l1-core/bin/trunk_solve-$Bin.exe" }
$tag = [IO.Path]::GetFileNameWithoutExtension($exe) + "-$Leaf-$Iterations"
$args = @('--config', $Config, '--leaf-model', $Leaf, '--ehs2-cache', $ehs, '--solver-k4-samples', '256',
    '--solver-k4-min-samples', '16', '--iterations', "$Iterations", '--eval-every', '0', '--print-every', '1',
    '--output', "$Out/$tag.json") + $Extra
Remove-Item "$Out/$tag.faults" -ErrorAction SilentlyContinue
$p = Start-Process $exe -ArgumentList $args -NoNewWindow -PassThru -RedirectStandardOutput "$Out/$tag.log" -RedirectStandardError "$Out/$tag.err"
$faults = 0; $peak = 0
while (-not $p.HasExited) {
    $w = Get-CimInstance Win32_Process -Filter "ProcessId=$($p.Id)" -ErrorAction SilentlyContinue
    if ($w) { $faults = $w.PageFaults; $peak = [math]::Max($peak, $w.PeakPageFileUsage)
        "{0:o} {1} {2}" -f (Get-Date), $faults, ((Get-Content "$Out/$tag.log" -ErrorAction SilentlyContinue | Measure-Object -Line).Lines) | Add-Content "$Out/$tag.faults" }
    Start-Sleep -Milliseconds 250
}
$p.WaitForExit()
"{0} exit {1} faults>={2:N0} peak_private={3:N0} MB cpu {4:N1} s" -f $tag, $p.ExitCode, $faults, ($peak / 1KB), $p.TotalProcessorTime.TotalSeconds
