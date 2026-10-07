# S4-1b-2: time DCFR iterations and the final evaluation on the B4 trees (Simple and General reference menus with
# postflop checkdown) and record each run's peak working set. Without -SolverK4 the solver uses the model's K4
# (2048 samples, seed 0); -SolverK4 256 uses the solver-only approximation of P2D7. The final evaluation always uses
# the model. Run from the workspace root after building the examples (see README.md):
#   pwsh experiments/p2-method-2026-10/trunk-speedup/run_b4.ps1 <out_dir> [-Solve <trunk_solve.exe>] [-Iterations 3] [-SolverK4 256] [-Tag name]
param(
    [Parameter(Mandatory = $true)][string]$Out,
    [string]$Solve = 'target/release/examples/trunk_solve.exe',
    [int]$Iterations = 3,
    [string]$SolverK4 = '',
    [string]$Tag = ''
)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $Out | Out-Null
$solve = (Resolve-Path $Solve).Path
$trees = [ordered]@{
    simple  = 'examples/bench/6max_100bb_nl50_partial_simple_reference_checkdown.toml'
    general = 'examples/bench/6max_100bb_nl50_partial_reference_checkdown.toml'
}
$extra = @()
if ($Tag -eq '') { $Tag = if ($SolverK4 -eq '') { 'model' } else { "s$SolverK4" } }
if ($SolverK4 -ne '') { $extra = @('--solver-k4-samples', $SolverK4) }
foreach ($tree in $trees.Keys) {
    $name = "b4_${tree}_$Tag"
    $arguments = @('--config', $trees[$tree], '--iterations', "$Iterations", '--eval-every', '0', '--print-every', '1') +
        $extra + @('--output', "$Out/$name.json")
    $start = Get-Date
    $process = Start-Process -FilePath $solve -ArgumentList $arguments -NoNewWindow -PassThru `
        -RedirectStandardOutput "$Out/$name.log" -RedirectStandardError "$Out/$name.err"
    $peak = 0
    while (-not $process.HasExited) {
        try { $process.Refresh(); $peak = [math]::Max($peak, $process.PeakWorkingSet64) } catch {}
        Start-Sleep -Milliseconds 500
    }
    $process.WaitForExit()
    $seconds = ((Get-Date) - $start).TotalSeconds
    if ($process.ExitCode -ne 0) { throw "$name exited with $($process.ExitCode)" }
    if ((Get-Item "$Out/$name.err").Length -eq 0) { Remove-Item "$Out/$name.err" }
    [ordered]@{ run = $name; solve = $Solve; wall_seconds = [math]::Round($seconds, 1); peak_working_set_mib = [math]::Round($peak / 1MB) } |
        ConvertTo-Json | Set-Content -Encoding utf8NoBOM "$Out/$name.metrics.json"
    "${name}: $([math]::Round($seconds))s, peak $([math]::Round($peak / 1MB)) MiB"
}
