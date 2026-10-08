$ErrorActionPreference = 'Stop'
$env:RAYON_NUM_THREADS = '1'
$env:CRITERION_HOME = Join-Path $PWD 'target/criterion'
$t21Scratch = 'C:\Users\PC_User\AppData\Local\Temp\claude\C--Users-PC-User-orca-workspaces-solvers-cisco\7b8f3bb9-2812-4052-b020-84dc5d1829b4\scratchpad'
$t21Allowed = (Get-Process -Id $PID).ProcessorAffinity.ToInt64()
$t21Affinity = [long]1
while (($t21Allowed -band $t21Affinity) -eq 0) { $t21Affinity = $t21Affinity -shl 1 }
"Affinity mask: $t21Affinity; allowed: $t21Allowed" | Set-Content runs/t21/bench-affinity.txt
for ($t21Round = 1; $t21Round -le 3; $t21Round++) {
    foreach ($t21Variant in @('base', 'new')) {
        $t21Exe = Join-Path $t21Scratch "t21-bench-$t21Variant.exe"
        $t21Stdout = Join-Path $PWD "runs/t21/bench-$t21Variant-$t21Round.log"
        $t21Stderr = Join-Path $PWD "runs/t21/bench-$t21Variant-$t21Round.stderr.log"
        $t21Process = Start-Process -FilePath $t21Exe -ArgumentList @('--bench', 'kernels_realistic/t21_opponent', '--noplot') -WorkingDirectory $PWD.Path -WindowStyle Hidden -PassThru -RedirectStandardOutput $t21Stdout -RedirectStandardError $t21Stderr
        $t21Process.ProcessorAffinity = [IntPtr]$t21Affinity
        $t21Process.WaitForExit()
        if ($t21Process.ExitCode -ne 0) { throw "Benchmark $t21Variant round $t21Round exited $($t21Process.ExitCode)" }
        foreach ($t21Name in @('t21_opponent_default', 't21_opponent_add')) {
            $t21Estimate = "target/criterion/kernels_realistic/$t21Name/new/estimates.json"
            if (!(Test-Path -LiteralPath $t21Estimate)) { throw "Missing Criterion estimates: $t21Estimate" }
            Copy-Item -LiteralPath $t21Estimate -Destination "runs/t21/bench-$t21Variant-$t21Round-$t21Name-estimates.json"
        }
        Write-Output "Finished $t21Variant round $t21Round"
        Get-Content $t21Stdout | Select-String -Pattern 'time:' -Context 1,0
    }
}
