# S4-1b-2: while the B4 timing runs, record the total CPU use and other heavy processes (builds, tests, evaluators)
# once per 10 s, to mark contaminated measurements. Stop it with Ctrl+C:
#   pwsh experiments/p2-method-2026-10/trunk-speedup/sample_load.ps1 -Log <load.log>
param([Parameter(Mandatory = $true)][string]$Log)
while ($true) {
    $busy = Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Name -in 'cargo', 'rustc', 'l0_eval', 'l0_real', 'link' -or ($_.Name -like '*test*' -and $_.Name -ne 'trunk_solve') }
    $cpu = [math]::Round((Get-Counter '\Processor(_Total)\% Processor Time' -SampleInterval 1 -MaxSamples 1).CounterSamples[0].CookedValue, 0)
    "$(Get-Date -Format o) cpu=$cpu busy=$(($busy | ForEach-Object { $_.Name }) -join ',')" | Add-Content $Log
    Start-Sleep -Seconds 9
}
