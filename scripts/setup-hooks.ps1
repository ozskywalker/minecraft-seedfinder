# setup-hooks.ps1 — one-time install of the repo's git hooks via core.hooksPath.
#
# This points this clone's hooks dir at the versioned .githooks/ directory (no copying
# needed, and the hooks live in the repo so every contributor gets the same gates).
#
# Usage:  .\scripts\setup-hooks.ps1
# Verify: git config core.hooksPath   ->  .githooks
$ErrorActionPreference = "Stop"

# scripts/ -> repo root ($PSScriptRoot is CWD-independent).
$root = Split-Path -Parent $PSScriptRoot

Push-Location $root
try {
    git config core.hooksPath .githooks
    if ($LASTEXITCODE -ne 0) { throw "git config core.hooksPath failed" }
} finally {
    Pop-Location
}

Write-Host ""
Write-Host "Git hooks installed (core.hooksPath -> .githooks):" -ForegroundColor Green
Write-Host "  pre-commit: cargo fmt --check + clippy (-D warnings)"
Write-Host "  pre-push  : full local CI gate (.\build.ps1 -Test)"
Write-Host ""
Write-Host "Verify with:  git config core.hooksPath" -ForegroundColor Cyan
Write-Host "Uninstall with: git config --unset core.hooksPath" -ForegroundColor DarkGray
