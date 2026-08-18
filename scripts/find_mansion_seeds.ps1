# Disposable utility: find seeds that have a woodland mansion near origin, then
# live-validate each against the real Bedrock server.
#
# Method:
#   1. Search the seed space with the engine for candidate seeds where a woodland
#      mansion is predicted within `Dist` blocks of origin (structural + biome gate).
#   2. For each candidate, live-verify it with `be-corpus verify-seed --structures
#      woodland_mansion` (one fresh world per seed, `/locate structure woodland_mansion`,
#      region-backed-out placement check).
#   3. Collect the seeds the server CONFIRMS have a mansion whose placement matches the
#      model (PASS), and print them.
#
# Requires: a live Bedrock server reachable over SSH (see AGENTS.md). Run from the repo
# root. Each candidate costs one full world recreation (~1-2 min) on the remote host.
#
# Exit 0 if at least `Target` confirmed seeds were found (writes `Out`), else 1.

param(
  [int]$Dist = 3000,
  [int]$LowEnd = 500,          # Phase A low32 sweep upper bound (candidate count)
  [int]$HighEnd = 80,          # Phase B high32 sweep upper bound
  [int]$MaxTry = 10,           # max candidate seeds to live-verify
  [int]$Target = 2,            # stop once this many confirmed
  [string]$HostName = "ai-assistant-01.longbranch.lwalker.me",
  [string]$Out = "mansion-seeds.txt"
)

$ErrorActionPreference = "Stop"
$dsl = "woodland_mansion m1 @origin <= $Dist"

Write-Host "[1/2] Searching for mansion candidate seeds (dist<=$Dist, low32 0..$LowEnd, high32 0..$HighEnd) ..."
$candidates = @(
  cargo run -q -p be-search -- search $dsl `
    --low-start 0 --low-end $LowEnd --high-start 0 --high-end $HighEnd --seeds-only 2>$null |
    Where-Object { $_ -match '^\d+$' }
)
Write-Host "      $($candidates.Count) candidate seed(s)."

$confirmed = New-Object System.Collections.Generic.List[string]
$tried = 0
foreach ($s in $candidates) {
  if ($confirmed.Count -ge $Target) { break }
  if ($tried -ge $MaxTry) { break }
  $tried++
  Write-Host "[2/2] ($tried) verifying seed $s (confirmed so far: $($confirmed.Count)) ..."
  $verification = (cargo run -q -p be-corpus -- verify-seed --seed $s --host $HostName --structures woodland_mansion 2>&1 | Out-String)
  if ($verification -match 'woodland_mansion: PASS \(observed \((-?\d+), (-?\d+)\)\)') {
    $x = [int64]$Matches[1]; $z = [int64]$Matches[2]
    Write-Host "      -> MANSION CONFIRMED at ($x, $z) — placement matches model"
    $confirmed.Add("$s ($x,$z)")
  } elseif ($verification -match 'woodland_mansion: FAIL') {
    Write-Host "      -> mansion found but placement MISMATCH (model wrong for this seed)"
  } elseif ($verification -match 'woodland_mansion: SKIP') {
    Write-Host "      -> no mansion near origin (SKIP)"
  } else {
    Write-Host "      -> no verdict line (recreate error / unparseable)"
  }
}

Write-Host ""
Write-Host "=== RESULT ==="
if ($confirmed.Count -gt 0) {
  Write-Host "Confirmed mansion seeds ($($confirmed.Count)):"
  $confirmed | ForEach-Object { Write-Host "  $_" }
  $confirmed | Set-Content -Path $Out
  Write-Host "Wrote $Out"
  exit 0
} else {
  Write-Host "No mansion-confirmed seed found in $($candidates.Count) candidates (tried $tried)."
  Write-Host "This may mean mansions rarely generate near origin in the searched range,"
  Write-Host "or the model over-predicts mansion generation (see PLAN 2.8)."
  exit 1
}
