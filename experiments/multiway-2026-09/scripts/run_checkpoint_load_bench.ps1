param(
    [Parameter(Mandatory = $true)][string]$Binary,
    [Parameter(Mandatory = $true)][string]$Checkpoint,
    [Parameter(Mandatory = $true)][string]$OutDir,
    [int]$TimeoutSeconds = 300
)
$ErrorActionPreference = 'Stop'
if ($TimeoutSeconds -le 0) { throw 'TimeoutSeconds must be positive' }
$benchBinary = (Resolve-Path -LiteralPath $Binary).Path
$benchCheckpoint = (Resolve-Path -LiteralPath $Checkpoint).Path
if (Test-Path -LiteralPath $OutDir) { throw 'Use a fresh OutDir for each measurement' }
$benchOutput = (New-Item -ItemType Directory -Path $OutDir).FullName
$benchStdout = Join-Path $benchOutput 'stdout.json'
$benchStderr = Join-Path $benchOutput 'stderr.txt'
$benchWatch = [Diagnostics.Stopwatch]::StartNew()
$benchStarted = [DateTime]::UtcNow.ToString('o')
$benchProcess = Start-Process -FilePath $benchBinary -ArgumentList @('--checkpoint', ('"' + $benchCheckpoint + '"')) -WindowStyle Hidden -PassThru -RedirectStandardOutput $benchStdout -RedirectStandardError $benchStderr
$benchPeak = 0L
$benchTimedOut = $false
while (-not $benchProcess.HasExited) {
    $benchProcess.Refresh()
    $benchPeak = [Math]::Max($benchPeak, $benchProcess.PeakWorkingSet64)
    if ($benchWatch.Elapsed.TotalSeconds -gt $TimeoutSeconds) {
        $benchTimedOut = $true
        $benchProcess.Kill()
        break
    }
    Start-Sleep -Milliseconds 50
}
$benchProcess.WaitForExit()
$benchWatch.Stop()
$benchRecord = [ordered]@{
    schemaVersion = 'solvers.checkpoint-load-measurement/v1'
    startedUtc = $benchStarted
    binary = $benchBinary
    binarySha256 = (Get-FileHash -LiteralPath $benchBinary -Algorithm SHA256).Hash.ToLowerInvariant()
    checkpoint = $benchCheckpoint
    checkpointSha256 = (Get-FileHash -LiteralPath $benchCheckpoint -Algorithm SHA256).Hash.ToLowerInvariant()
    sourceRevision = (git rev-parse HEAD)
    sourceStatus = @(git status --short)
    timeoutSeconds = $TimeoutSeconds
    timedOut = $benchTimedOut
    exitCode = $benchProcess.ExitCode
    wallSeconds = $benchWatch.Elapsed.TotalSeconds
    observedPeakWorkingSetBytes = $benchPeak
    peakMethod = 'Windows process lifetime PeakWorkingSet64 queried every 50 ms; final unsampled interval may be omitted. Includes decoded-state digest and disposal.'
    validationReport = 'docs/validation/multiway-checkpoint-streaming-2026-09-10.md'
}
$benchRecord | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $benchOutput 'measurement.json') -Encoding utf8
if ($benchTimedOut -or $benchProcess.ExitCode -ne 0) { throw 'Checkpoint load benchmark failed; see measurement and stderr' }
Get-Content -LiteralPath $benchStdout
$benchRecord | ConvertTo-Json -Depth 5
