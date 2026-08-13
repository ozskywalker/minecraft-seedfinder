# build.ps1 — easy self-build entry point for the Bedrock Seedfinder.
#
# Default action: produce the production single-.exe deliverable (UI embedded), exactly
# like scripts/build-release.ps1, which it delegates to. Also offers a -Dev build and a
# -Test check for everyday local work.
#
# Usage:
#   .\build.ps1              # release exe -> dist\seedfinder.exe  (UI baked in)
#   .\build.ps1 -Dev         # build UI + debug server for local iteration
#   .\build.ps1 -Test        # run Rust workspace tests + UI tests + typecheck
#   .\build.ps1 -Help        # this help
#
# Requires on the BUILD machine: a Rust toolchain and Node.js (>=20.19). The end user
# needs neither (the release exe embeds everything).

param(
    [switch]$Dev,
    [switch]$Test,
    [switch]$Help
)

$ErrorActionPreference = "Stop"

$helpText = @'
build.ps1 - easy self-build for the Bedrock Seedfinder

  .\build.ps1              release exe -> dist\seedfinder.exe (UI embedded, auto-opens browser)
  .\build.ps1 -Dev         build the web UI + a debug server for local iteration
  .\build.ps1 -Test        run Rust workspace tests, UI tests, and the UI typecheck
  .\build.ps1 -Help        show this help

Requires Rust + Node.js (>=20.19) on this machine. The release exe needs neither.
'@

if ($Help) {
    Write-Host $helpText
    exit 0
}

$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$uiDir = Join-Path $root "ui"

function Ensure-NodeDeps {
    if (-not (Test-Path (Join-Path $uiDir "node_modules"))) {
        Push-Location $uiDir
        try { npm ci | Out-Host } finally { Pop-Location }
        if ($LASTEXITCODE -ne 0) { throw "npm ci failed" }
    }
}

function Build-WebUI {
    Write-Host "==> Building web UI (ui/dist) ..." -ForegroundColor Cyan
    Push-Location $uiDir
    try { npm run build | Out-Host } finally { Pop-Location }
    if ($LASTEXITCODE -ne 0) { throw "UI build failed (exit $LASTEXITCODE)" }
}

# --- -Test: run the full test suite + typecheck -------------------------------------
if ($Test) {
    Write-Host "==> Rust workspace tests ..." -ForegroundColor Cyan
    Push-Location $root
    try { cargo test --workspace } finally { Pop-Location }
    if ($LASTEXITCODE -ne 0) { throw "cargo test failed (exit $LASTEXITCODE)" }

    Ensure-NodeDeps
    Write-Host "==> UI tests ..." -ForegroundColor Cyan
    Push-Location $uiDir
    try { npm test | Out-Host } finally { Pop-Location }
    if ($LASTEXITCODE -ne 0) { throw "UI tests failed (exit $LASTEXITCODE)" }

    Write-Host "==> UI typecheck ..." -ForegroundColor Cyan
    Push-Location $uiDir
    try { npm run typecheck | Out-Host } finally { Pop-Location }
    if ($LASTEXITCODE -ne 0) { throw "UI typecheck failed (exit $LASTEXITCODE)" }

    Write-Host ""
    Write-Host "All checks passed." -ForegroundColor Green
    exit 0
}

# --- -Dev: build UI + debug server for local iteration ------------------------------
if ($Dev) {
    Ensure-NodeDeps
    Build-WebUI

    Write-Host "==> Building server (debug) ..." -ForegroundColor Cyan
    Push-Location $root
    try { cargo build -p server } finally { Pop-Location }
    if ($LASTEXITCODE -ne 0) { throw "server build failed (exit $LASTEXITCODE)" }

    Write-Host ""
    Write-Host "Dev build ready: target\debug\server.exe" -ForegroundColor Green
    Write-Host "Run it with SEEDFINDER_NO_OPEN=1 to skip auto-opening the browser, e.g.:"
    Write-Host "  SEEDFINDER_NO_OPEN=1 .\target\debug\server.exe"
    Write-Host "It serves the UI (from ui/dist) on http://127.0.0.1:8080."
    exit 0
}

# --- Default: production single-exe (delegate to the existing release script) -------
& (Join-Path $root "scripts\build-release.ps1")
if ($LASTEXITCODE -ne 0) { throw "release build failed (exit $LASTEXITCODE)" }
