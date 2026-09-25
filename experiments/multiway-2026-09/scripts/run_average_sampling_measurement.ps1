param(
    [Parameter(Mandatory = $true)][string]$Binary,
    [Parameter(Mandatory = $true)][string]$Job,
    [Parameter(Mandatory = $true)][string]$OutDir,
    [ValidateSet('AverageSampling', 'TreeInitialization', 'CheckpointAudit')][string]$MeasurementKind = 'AverageSampling'
)
$ErrorActionPreference = 'Stop'
$researchBinary = (Resolve-Path -LiteralPath $Binary).Path
$researchJobPath = (Resolve-Path -LiteralPath $Job).Path
$researchJob = Get-Content -LiteralPath $researchJobPath -Raw -Encoding utf8 | ConvertFrom-Json
if ($researchJob.timeoutSeconds -le 0) { throw 'Job timeoutSeconds must be positive' }
if ($researchJob.arguments.Count -eq 0) { throw 'Job arguments must be nonempty' }
# This runner accepts literal arguments, not shell commands. Reject embedded
# quotes rather than attempting another level of Windows command-line parsing.
$researchArguments = @($researchJob.arguments | ForEach-Object {
    if ($_ -match '["\r\n]') { throw 'Job arguments cannot contain quotes or newlines' }
    '"' + $_ + '"'
})
if (Test-Path -LiteralPath $OutDir) { throw 'Use a fresh OutDir for each measurement' }
$researchOutput = (New-Item -ItemType Directory -Path $OutDir).FullName
$researchStdout = Join-Path $researchOutput 'stdout.json'
$researchStderr = Join-Path $researchOutput 'stderr.txt'
$researchStarted = [DateTime]::UtcNow.ToString('o')
$researchWatch = [Diagnostics.Stopwatch]::StartNew()
$researchProcess = Start-Process -FilePath $researchBinary -ArgumentList $researchArguments -WindowStyle Hidden -PassThru -RedirectStandardOutput $researchStdout -RedirectStandardError $researchStderr
$researchPeak = 0L
$researchTimedOut = $false
while (-not $researchProcess.HasExited) {
    $researchProcess.Refresh()
    $researchPeak = [Math]::Max($researchPeak, $researchProcess.PeakWorkingSet64)
    if ($researchWatch.Elapsed.TotalSeconds -gt $researchJob.timeoutSeconds) {
        $researchTimedOut = $true
        $researchProcess.Kill()
        break
    }
    Start-Sleep -Milliseconds 50
}
$researchProcess.WaitForExit()
$researchWatch.Stop()
$researchRecord = [ordered]@{
    schemaVersion = $(if ($MeasurementKind -eq 'TreeInitialization') { 'solvers.tree-initialization-measurement/v1' } elseif ($MeasurementKind -eq 'CheckpointAudit') { 'solvers.checkpoint-audit-measurement/v1' } else { 'solvers.average-sampling-measurement/v1' })
    startedUtc = $researchStarted
    binary = $researchBinary
    binarySha256 = (Get-FileHash -LiteralPath $researchBinary -Algorithm SHA256).Hash.ToLowerInvariant()
    job = $researchJobPath
    jobSha256 = (Get-FileHash -LiteralPath $researchJobPath -Algorithm SHA256).Hash.ToLowerInvariant()
    arguments = $researchJob.arguments
    configSha256 = $researchJob.configSha256
    sourceManifestSha256 = $researchJob.sourceManifestSha256
    sourceRevision = (git rev-parse HEAD)
    sourceStatus = @(git status --short)
    timeoutSeconds = $researchJob.timeoutSeconds
    timedOut = $researchTimedOut
    exitCode = $researchProcess.ExitCode
    wallSeconds = $researchWatch.Elapsed.TotalSeconds
    observedPeakWorkingSetBytes = $researchPeak
    peakMethod = 'Windows lifetime PeakWorkingSet64 queried every 50 ms; final unsampled interval may be omitted. Whole process covers all phases executed by the child binary, including JSON and disposal.'
    stdoutSha256 = (Get-FileHash -LiteralPath $researchStdout -Algorithm SHA256).Hash.ToLowerInvariant()
    validationReport = $researchJob.validationReport
}
$researchRecord | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $researchOutput 'measurement.json') -Encoding utf8
if ($researchTimedOut -or $researchProcess.ExitCode -ne 0) { throw 'Research measurement failed; see measurement and stderr' }
$researchResult = Get-Content -LiteralPath $researchStdout -Raw -Encoding utf8 | ConvertFrom-Json
if ($MeasurementKind -eq 'TreeInitialization') {
    if (-not $researchResult.measurement.complete) { throw 'Tree initialization output is incomplete' }
    [ordered]@{
        output = $researchOutput
        mode = $researchResult.mode
        threads = $researchResult.threads
        timings = $researchResult.measurement.timings
        wallSeconds = $researchWatch.Elapsed.TotalSeconds
        observedPeakWorkingSetBytes = $researchPeak
        treeBlake3 = $researchResult.measurement.treeBlake3
        arenaLayoutBlake3 = $researchResult.measurement.arenaLayoutBlake3
    } | ConvertTo-Json
} elseif ($MeasurementKind -eq 'CheckpointAudit') {
    [ordered]@{
        output = $researchOutput
        sweeps = $researchResult.sweeps
        constructionSeconds = $researchResult.constructionElapsedSecs
        wallSeconds = $researchWatch.Elapsed.TotalSeconds
        observedPeakWorkingSetBytes = $researchPeak
        configurationFingerprint = $researchResult.configurationFingerprint
        abstractionFingerprint = $researchResult.abstractionFingerprint
    } | ConvertTo-Json
} else {
    [ordered]@{
        output = $researchOutput
        variant = $researchResult.result.variant
        sweeps = $researchResult.result.metrics.sweeps
        solveSeconds = $researchResult.result.solve_elapsed_secs
        constructionSeconds = $researchResult.constructionElapsedSecs
        wallSeconds = $researchWatch.Elapsed.TotalSeconds
        observedPeakWorkingSetBytes = $researchPeak
        regretFingerprint = $researchResult.result.current_regret_fingerprint
    } | ConvertTo-Json
}
