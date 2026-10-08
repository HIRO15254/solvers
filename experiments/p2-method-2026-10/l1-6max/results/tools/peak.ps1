param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$Log,
    [Parameter(ValueFromRemainingArguments = $true)][string[]]$Rest
)
# Run $Exe with $Rest, stdout and stderr to $Log, and print its peak commit (PeakPagefileUsage, which .NET exposes as
# PeakPagedMemorySize64) and wall time. The peak is read every 100 ms while the process runs; the counter only grows,
# so the last value read is the peak up to then.
$start = Get-Date
$p = Start-Process -FilePath $Exe -ArgumentList $Rest -NoNewWindow -PassThru -RedirectStandardOutput $Log -RedirectStandardError "$Log.err"
$peak = 0
while (-not $p.HasExited) {
    try { $p.Refresh(); if ($p.PeakPagedMemorySize64 -gt $peak) { $peak = $p.PeakPagedMemorySize64 } } catch {}
    Start-Sleep -Milliseconds 100
}
$p.WaitForExit()
"peak_commit_gb={0:N3} seconds={1:N1} exit={2}" -f ($peak / 1GB), ((Get-Date) - $start).TotalSeconds, $p.ExitCode
