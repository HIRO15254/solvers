param([string]$ReservationId = 'r1-20260925-01')
$ErrorActionPreference = 'Stop'
$campaignDir = $PSScriptRoot
$ledger = Get-Content -LiteralPath (Join-Path $campaignDir 'budget.json') -Raw | ConvertFrom-Json
$held = ($ledger.reservations | Where-Object { -not $_.reservation_released } | Measure-Object -Property reserved_usd -Sum).Sum
$settled = ($ledger.reservations | Where-Object { $_.reservation_released } | Measure-Object -Property billed_usd -Sum).Sum
if (($held + $settled) -gt $ledger.authorized_limit) { throw 'Budget reservation exceeds authorization' }
$reservation = $ledger.reservations | Where-Object id -eq $ReservationId
if (-not $reservation -or $reservation.reservation_released) { throw 'Missing active reservation' }
$record = Join-Path $campaignDir "launch-$ReservationId.json"
if ($ReservationId -eq 'r1-20260925-01' -and (Test-Path (Join-Path $campaignDir 'launch-01.json'))) { throw 'Original launch already attempted' }
if (Test-Path -LiteralPath $record) { throw 'Launch already attempted; reconcile the recorded resource before any retry' }
$started = [DateTimeOffset]::UtcNow
$deadline = $started.AddHours($reservation.maximum_runtime_hours).ToString('yyyy-MM-ddTHH:mm:ssZ')
$terminationAction = if ($reservation.instance_termination_action) { $reservation.instance_termination_action } else { 'DELETE' }
if ($terminationAction -notin @('STOP', 'DELETE')) { throw 'Invalid instance termination action' }
if ($terminationAction -eq 'STOP' -and -not $reservation.explicit_disk_cleanup_hours) { throw 'STOP requires a bounded disk cleanup allowance' }
$gcpArgs = @(
  'compute', 'instances', 'create', $reservation.instance,
  "--project=$($reservation.project)", "--zone=$($reservation.zone)",
  "--machine-type=$($reservation.machine_type)", '--provisioning-model=SPOT',
  "--instance-termination-action=$terminationAction", "--termination-time=$deadline",
  '--no-restart-on-failure', '--maintenance-policy=TERMINATE',
  '--image-family=ubuntu-2404-lts-amd64', '--image-project=ubuntu-os-cloud',
  "--boot-disk-type=$($reservation.disk_type)", "--boot-disk-size=$($reservation.disk_gib)GB",
  '--boot-disk-auto-delete', '--no-service-account', '--no-scopes',
  "--labels=campaign=hu-postflop-r1,reservation=$ReservationId",
  "--metadata-from-file=startup-script=$(Join-Path $campaignDir 'bootstrap.sh')",
  '--format=json(id,name,status,creationTimestamp,scheduling,disks)', '--quiet'
)
@{ reservation_id=$reservation.id; attempted_at=$started.ToString('o'); termination_time=$deadline; argv=$gcpArgs } |
  ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $record -Encoding utf8
& gcloud @gcpArgs | Tee-Object -FilePath (Join-Path $campaignDir "create-result-$ReservationId.json")
if ($LASTEXITCODE -ne 0) { throw "Create returned $LASTEXITCODE; inspect the named instance before retrying" }
