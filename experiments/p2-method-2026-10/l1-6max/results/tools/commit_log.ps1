param([Parameter(Mandatory = $true)][string]$Log)
# Committed memory every 15 s, with trunk_solve's private bytes and, when the headroom is below 3 GB, the largest
# process groups.
while ($true) {
    $m = Get-CimInstance Win32_PerfFormattedData_PerfOS_Memory
    $solve = (Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Name -like 'trunk_solve*' -or $_.Name -like 'l0_eval*' } |
        ForEach-Object { "$($_.Name):$([math]::Round($_.PrivateMemorySize64 / 1GB, 2))" }) -join ','
    $line = "{0} committed={1:N2} limit={2:N2} solve={3}" -f (Get-Date -Format o), ($m.CommittedBytes / 1GB), ($m.CommitLimit / 1GB), $solve
    if ($m.CommitLimit - $m.CommittedBytes -lt 3GB) {
        $top = Get-Process | Group-Object ProcessName | ForEach-Object {
            "$($_.Name)x$($_.Count)=$([math]::Round((($_.Group | Measure-Object PrivateMemorySize64 -Sum).Sum) / 1GB, 2))"
        } | Sort-Object { [double]($_ -split '=')[1] } -Descending | Select-Object -First 8
        $line += " top=" + ($top -join ',')
    }
    $line | Add-Content $Log
    Start-Sleep -Seconds 15
}
