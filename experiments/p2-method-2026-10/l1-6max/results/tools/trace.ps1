param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$Log,
    [Parameter(ValueFromRemainingArguments = $true)][string[]]$Rest
)
# Run $Exe with $Rest (stdout and stderr to $Log), writing "<seconds> <private GB> <peak commit GB> <page faults>"
# every second to "$Log.trace", then print the peak commit, the page faults and the wall time.
$start = Get-Date
$p = Start-Process -FilePath $Exe -ArgumentList $Rest -NoNewWindow -PassThru -RedirectStandardOutput $Log -RedirectStandardError "$Log.err"
$trace = "$Log.trace"
Set-Content -Path $trace -Value "seconds private_gb peak_commit_gb page_faults"
$peak = 0
$faults = 0
while (-not $p.HasExited) {
    try {
        $p.Refresh()
        if ($p.PeakPagedMemorySize64 -gt $peak) { $peak = $p.PeakPagedMemorySize64 }
        $w = Get-CimInstance Win32_Process -Filter "ProcessId=$($p.Id)" -ErrorAction SilentlyContinue
        if ($w) { $faults = $w.PageFaults }
        "{0:N1} {1:N3} {2:N3} {3}" -f ((Get-Date) - $start).TotalSeconds, ($p.PagedMemorySize64 / 1GB), ($peak / 1GB), $faults |
            Add-Content -Path $trace
    } catch {}
    Start-Sleep -Seconds 1
}
$p.WaitForExit()
"peak_commit_gb={0:N3} page_faults={1} seconds={2:N1} exit={3}" -f ($peak / 1GB), $faults, ((Get-Date) - $start).TotalSeconds, $p.ExitCode
