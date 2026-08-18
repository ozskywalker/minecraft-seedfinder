# build.ps1 — easy self-build entry point for the Bedrock Seedfinder.
#
# Default action: produce the production single-.exe deliverable (UI embedded), exactly
# like scripts/build-release.ps1, which it delegates to. Also offers a -Dev build and a
# -Test check for everyday local work.
#
# Usage:
#   .\build.ps1              # release exe -> dist\seedfinder.exe  (UI baked in)
#   .\build.ps1 -Dev         # build UI + debug server for local iteration
#   .\build.ps1 -Test        # run ALL CI gates locally (fmt, clippy, tests, perf guard,
#                            #   cargo-deny, UI test/typecheck/build) — mirrors ci.yml
#   .\build.ps1 -Test -SkipPerf   # same but skip the slow SIMD release perf guard
#   .\build.ps1 -Help        # this help
#
# Requires on the BUILD machine: a Rust toolchain, Node.js (>=20.19), and cargo-deny
# (`cargo install cargo-deny --locked` if missing). The end user needs none of these
# (the release exe embeds everything).

param(
    [switch]$Dev,
    [switch]$Test,
    [switch]$SkipPerf,
    [switch]$Help
)

$ErrorActionPreference = "Stop"

$helpText = @'
build.ps1 - easy self-build for the Bedrock Seedfinder

  .\build.ps1              release exe -> dist\seedfinder.exe (UI embedded, auto-opens browser)
  .\build.ps1 -Dev         build the web UI + a debug server for local iteration
  .\build.ps1 -Test        run ALL local CI gates: Rust fmt + clippy (-D warnings) + tests,
                           SIMD-batch perf guard (release), cargo-deny, and UI test/typecheck/build
  .\build.ps1 -Test -SkipPerf   same, but skip the slow SIMD release perf guard (weakens CI parity)
  .\build.ps1 -Help        show this help

Requires Rust + Node.js (>=20.19) + cargo-deny on this machine. The release exe needs neither.
'@

if ($Help) {
    Write-Host $helpText
    exit 0
}

$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$uiDir = Join-Path $root "ui"

function Ensure-NodeDeps {
    param([switch]$Force)
    # CI runs `npm ci` unconditionally; -Force mirrors that for -Test so a stale
    # node_modules can't let local gates pass on outdated deps. -Dev keeps the lazy form.
    if ($Force -or -not (Test-Path (Join-Path $uiDir "node_modules"))) {
        Push-Location $uiDir
        try { npm ci | Out-Host } finally { Pop-Location }
        if ($LASTEXITCODE -ne 0) { throw "npm ci failed" }
    }
}

function Build-WebUI {
    Invoke-Gate -Name "Building web UI (ui/dist)" -Dir $uiDir -Command { npm run build | Out-Host }
}

# Run one gate command in `Dir`, working from the repo root by default, and throw on any
# nonzero exit so the whole -Test run fails fast on the first problem. Keeping this as a
# single helper avoids drift between the (many) gates that mirror ci.yml.
function Invoke-Gate {
    param(
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][scriptblock]$Command,
        [string]$Dir = $root,
        [string]$FailMessage
    )
    Write-Host "==> $Name ..." -ForegroundColor Cyan
    Push-Location $Dir
    try { & $Command } finally { Pop-Location }
    if ($LASTEXITCODE -ne 0) {
        $detail = if ($FailMessage) { " $FailMessage" } else { "" }
        throw "$Name failed (exit $LASTEXITCODE).$detail"
    }
}

# --- -SkipPerf only has meaning under -Test; say so rather than silently ignoring it ---
if ($SkipPerf -and -not $Test) {
    Write-Host "NOTE: -SkipPerf has no effect without -Test." -ForegroundColor Yellow
}

# --- -Test: run the full CI gate suite locally (mirrors .github/workflows/ci.yml) ------
if ($Test) {
    # Mirrors the CI Rust job's gates so local -Test catches what CI catches. Keep these
    # in the same order as ci.yml and fail fast on any nonzero exit.

    Invoke-Gate -Name "Rust format check" -Command { cargo fmt --all -- --check }
    Invoke-Gate -Name "Rust clippy (deny warnings)" -Command { cargo clippy --workspace --all-targets -- -D warnings }
    Invoke-Gate -Name "Rust workspace tests" -Command { cargo test --workspace }

    # SIMD-batch perf guard (release). CI always runs this; -SkipPerf is a deliberate,
    # loud opt-out for slow iteration. Correctness is already covered by cargo test, so
    # skipping only weakens the *perf floor* coverage — never correctness.
    if (-not $SkipPerf) {
        Invoke-Gate -Name "SIMD-batch perf guard (release)" -Command { cargo run -p be-search --release --example bench_sweep -- --check }
    } else {
        Write-Host "!! SKIPPED SIMD-batch perf guard (-SkipPerf). CI parity weakened: a perf regression would only be caught by CI." -ForegroundColor Yellow
    }

    # cargo-deny (licenses/advisories/bans/sources) — same deny.toml + command CI's
    # EmbarkStudios/cargo-deny-action@v2 runs (both default to plain `check`; cargo-deny
    # >=0.20 removed an `--all-features` flag for `check`).
    Invoke-Gate -Name "cargo-deny (licenses/advisories/bans)" -Command { cargo deny check } -FailMessage "install with 'cargo install cargo-deny --locked' if missing"

    Ensure-NodeDeps -Force
    Invoke-Gate -Name "UI tests" -Dir $uiDir -Command { npm test | Out-Host }
    Invoke-Gate -Name "UI typecheck" -Dir $uiDir -Command { npm run typecheck | Out-Host }
    Invoke-Gate -Name "UI build" -Dir $uiDir -Command { npm run build | Out-Host }

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
