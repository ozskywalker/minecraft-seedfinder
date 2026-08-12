# build-release.ps1 — produce the production single-`.exe` deliverable.
#
# Builds the web UI, then compiles the server in release mode so the UI is EMBEDDED into
# the binary (see crates/server/build.rs). The result is a single self-contained exe that
# a non-technical user can double-click: it starts the local server and opens the default
# browser automatically — no Rust, no Node, and no ui/dist folder needed on their machine.
#
# Usage:
#   .\scripts\build-release.ps1
#
# Output: <repo>/dist/seedfinder.exe  (plus a copy at <repo>/seedfinder.exe for convenience)
#
# Requires: a Rust toolchain and Node.js (>=20.19) on the BUILD machine. The end user
# needs neither.

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)  # repo root
Push-Location $root
try {
    Write-Host "==> Building web UI (ui/dist) ..." -ForegroundColor Cyan
    Push-Location "ui"
    try {
        if (-not (Test-Path "node_modules")) { npm ci | Out-Host }
        npm run build | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "UI build failed (exit $LASTEXITCODE)" }
    } finally {
        Pop-Location
    }

    Write-Host "==> Building server (release, UI embedded) ..." -ForegroundColor Cyan
    cargo build --release -p server
    if ($LASTEXITCODE -ne 0) { throw "server build failed (exit $LASTEXITCODE)" }

    $exe = Join-Path $root "target\release\server.exe"
    if (-not (Test-Path $exe)) { throw "expected binary not found: $exe" }

    $dist = Join-Path $root "dist"
    New-Item -ItemType Directory -Force -Path $dist | Out-Null
    Copy-Item $exe (Join-Path $dist "seedfinder.exe") -Force
    Copy-Item $exe (Join-Path $root "seedfinder.exe") -Force

    $size = (Get-Item (Join-Path $dist "seedfinder.exe")).Length / 1MB
    Write-Host ""
    Write-Host "Done." -ForegroundColor Green
    Write-Host "  Single exe:  dist\seedfinder.exe  ($([math]::Round($size,1)) MB, UI embedded)"
    Write-Host "  Give this ONE file to a non-technical user. Double-click it; it serves"
    Write-Host "  the UI on http://127.0.0.1:8080 and opens their default browser."
    Write-Host ""
    Write-Host "Note: to run it now from this terminal instead, use:  .\dist\seedfinder.exe"
} finally {
    Pop-Location
}
