$ErrorActionPreference = "Stop"
$root = (git rev-parse --show-toplevel).Trim()

Write-Host ""
Write-Host "[pre-commit] Rust format check ..." -ForegroundColor Cyan
Push-Location $root
try { cargo fmt --all -- --check } finally { Pop-Location }
if ($LASTEXITCODE -ne 0) {
    Write-Host "[pre-commit] FAILED: cargo fmt --check. Run 'cargo fmt --all', re-stage, and retry." -ForegroundColor Red
    exit 1
}

Write-Host "[pre-commit] Rust clippy (deny warnings) ..." -ForegroundColor Cyan
Push-Location $root
try { cargo clippy --workspace --all-targets -- -D warnings } finally { Pop-Location }
if ($LASTEXITCODE -ne 0) {
    Write-Host "[pre-commit] FAILED: clippy. Fix warnings, re-stage, and retry." -ForegroundColor Red
    exit 1
}

Write-Host "[pre-commit] OK" -ForegroundColor Green
