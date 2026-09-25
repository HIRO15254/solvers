param(
    [Parameter(Mandatory = $true)][string]$Binary,
    [Parameter(Mandatory = $true)][string]$Job,
    [Parameter(Mandatory = $true)][string]$OutDir
)
$ErrorActionPreference = 'Stop'
$benchBinary = (Resolve-Path -LiteralPath $Binary).Path
$benchJobPath = (Resolve-Path -LiteralPath $Job).Path
$benchJob = Get-Content -LiteralPath $benchJobPath -Raw -Encoding utf8 | ConvertFrom-Json
if ($benchJob.timeoutSeconds -le 0) { throw 'timeoutSeconds must be positive' }
function Get-BenchInputHashes {
    [ordered]@{
        binary = (Get-FileHash -LiteralPath $benchBinary -Algorithm SHA256).Hash.ToLowerInvariant()
        job = (Get-FileHash -LiteralPath $benchJobPath -Algorithm SHA256).Hash.ToLowerInvariant()
        config = (Get-FileHash -LiteralPath $benchJob.config -Algorithm SHA256).Hash.ToLowerInvariant()
        inputCheckpoint = $(if ($null -eq $benchJob.inputCheckpoint) { $null } else {
            (Get-FileHash -LiteralPath $benchJob.inputCheckpoint -Algorithm SHA256).Hash.ToLowerInvariant()
        })
        sourceManifest = (Get-FileHash -LiteralPath $benchJob.sourceManifest -Algorithm SHA256).Hash.ToLowerInvariant()
    }
}
$benchBefore = Get-BenchInputHashes
if ($benchBefore.binary -ne $benchJob.binarySha256 -or
    $benchBefore.config -ne $benchJob.configSha256 -or
    $benchBefore.inputCheckpoint -ne $benchJob.inputCheckpointSha256 -or
    $benchBefore.sourceManifest -ne $benchJob.sourceManifestSha256) {
    throw 'Input hashes differ from the declared job before launch'
}
$benchArguments = @($benchJob.arguments | ForEach-Object {
    if ($_ -match '["\r\n]|\\$') { throw 'Unsupported quoted/newline/trailing-backslash argument' }
    '"' + $_ + '"'
})
if (Test-Path -LiteralPath $OutDir) { throw 'Use a fresh OutDir for each measurement' }
$benchOutput = (New-Item -ItemType Directory -Path $OutDir).FullName
$benchStdout = Join-Path $benchOutput 'stdout.json'
$benchStderr = Join-Path $benchOutput 'stderr.txt'
$benchObservations = [System.Collections.Generic.List[object]]::new()
$benchStarted = [DateTime]::UtcNow.ToString('o')
$benchWatch = [Diagnostics.Stopwatch]::StartNew()
$benchProcess = Start-Process -FilePath $benchBinary -ArgumentList $benchArguments -WindowStyle Hidden -PassThru -RedirectStandardOutput $benchStdout -RedirectStandardError $benchStderr
$benchPeak = 0L
$benchTimedOut = $false
while (-not $benchProcess.HasExited) {
    $benchProcess.Refresh()
    $benchPeak = [Math]::Max($benchPeak, $benchProcess.PeakWorkingSet64)
    $benchObservations.Add([ordered]@{
        unixMs = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
        workingSetBytes = $benchProcess.WorkingSet64
    })
    if ($benchWatch.Elapsed.TotalSeconds -gt $benchJob.timeoutSeconds) {
        $benchTimedOut = $true
        $benchProcess.Kill()
        break
    }
    Start-Sleep -Milliseconds 50
}
$benchProcess.WaitForExit()
$benchWatch.Stop()
$benchAfter = Get-BenchInputHashes
$benchInputsUnchanged = @($benchBefore.Keys | Where-Object { $benchBefore[$_] -ne $benchAfter[$_] }).Count -eq 0
$benchRecord = [ordered]@{
    schemaVersion = 'solvers.checkpoint-write-measurement/v1'
    startedUtc = $benchStarted
    binary = $benchBinary
    binarySha256 = (Get-FileHash -LiteralPath $benchBinary -Algorithm SHA256).Hash.ToLowerInvariant()
    job = $benchJobPath
    jobSha256 = (Get-FileHash -LiteralPath $benchJobPath -Algorithm SHA256).Hash.ToLowerInvariant()
    arguments = $benchJob.arguments
    configSha256 = $benchJob.configSha256
    sourceManifestSha256 = $benchJob.sourceManifestSha256
    inputCheckpointSha256 = $benchBefore.inputCheckpoint
    inputHashesBefore = $benchBefore
    inputHashesAfter = $benchAfter
    inputsUnchanged = $benchInputsUnchanged
    sourceRevision = (git rev-parse HEAD)
    sourceStatus = @(git status --short)
    timeoutSeconds = $benchJob.timeoutSeconds
    timedOut = $benchTimedOut
    exitCode = $benchProcess.ExitCode
    wallSeconds = $benchWatch.Elapsed.TotalSeconds
    observedPeakWorkingSetBytes = $benchPeak
    peakMethod = 'Windows lifetime PeakWorkingSet64 queried every 50 ms; final unsampled interval may be omitted. Includes construction/training, writing, hashing and disposal.'
    workingSetObservations = @($benchObservations.ToArray())
    phasePeakMethod = 'Current WorkingSet64 samples timestamped after Refresh, filtered to the benchmark write interval using the same host clock. Sampling and millisecond boundaries may omit peaks; no allocator or process-memory cap is implied.'
    stdoutSha256 = (Get-FileHash -LiteralPath $benchStdout -Algorithm SHA256).Hash.ToLowerInvariant()
    validationReport = $benchJob.validationReport
}
$benchRecord | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $benchOutput 'measurement.json') -Encoding utf8
if ($benchTimedOut -or $benchProcess.ExitCode -ne 0) { throw 'Checkpoint write benchmark failed; see measurement and stderr' }
if (-not $benchInputsUnchanged) { throw 'Benchmark inputs changed during execution' }
$benchResult = Get-Content -LiteralPath $benchStdout -Raw -Encoding utf8 | ConvertFrom-Json
if ($benchResult.schemaVersion -ne 'solvers.multiway-checkpoint-write-bench/v1') { throw 'Unexpected benchmark schema' }
$benchPhase = @($benchObservations | Where-Object {
    $_.unixMs -ge $benchResult.writeStartedUnixMs -and $_.unixMs -le $benchResult.writeFinishedUnixMs
})
[ordered]@{
    output = $benchOutput
    mode = $benchResult.mode
    writeSeconds = $benchResult.writeSeconds
    wallSeconds = $benchWatch.Elapsed.TotalSeconds
    observedPeakWorkingSetBytes = $benchPeak
    observedWriteWorkingSetBytes = $(if ($benchPhase.Count -gt 0) { ($benchPhase | Measure-Object -Property workingSetBytes -Maximum).Maximum } else { $null })
    writePhaseSamples = $benchPhase.Count
    outputBytes = $benchResult.outputBytes
    outputBlake3 = $benchResult.outputBlake3
} | ConvertTo-Json
