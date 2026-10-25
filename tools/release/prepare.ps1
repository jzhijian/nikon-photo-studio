param(
    [ValidateSet('Validate', 'Stage')][string]$Mode = 'Validate',
    [string]$RepositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$package = Get-Content -Raw -LiteralPath (Join-Path $RepositoryRoot 'package.json') | ConvertFrom-Json
$tauri = Get-Content -Raw -LiteralPath (Join-Path $RepositoryRoot 'src-tauri/tauri.conf.json') | ConvertFrom-Json
$cargo = Get-Content -Raw -LiteralPath (Join-Path $RepositoryRoot 'src-tauri/Cargo.toml')
$cargoVersion = [regex]::Match($cargo, '(?ms)^\[package\]\s*.*?^version\s*=\s*"([^"]+)"').Groups[1].Value
$version = [string]$tauri.version
if ($version -notmatch '^\d+\.\d+\.\d+$' -or $package.version -ne $version -or $cargoVersion -ne $version) {
    throw 'package.json, tauri.conf.json, and Cargo.toml must have the same X.Y.Z version.'
}
if ($env:GITHUB_REF -like 'refs/tags/*' -and $env:GITHUB_REF -ne "refs/tags/v$version") {
    throw "A stable release tag must be v$version. Update all three version fields before tagging."
}
if ($tauri.build.frontendDist -ne '../dist') {
    throw 'Release packaging expects only the generated dist frontend.'
}
foreach ($field in @('resources', 'externalBin')) {
    if ($tauri.bundle.PSObject.Properties.Name -contains $field -and $tauri.bundle.$field) {
        throw "Release bundle.$field must be reviewed before shipping external files. SDKs are installed separately."
    }
}
$overrides = @(Get-ChildItem -LiteralPath (Join-Path $RepositoryRoot 'src-tauri') -File |
    Where-Object { $_.Name -match '^tauri\.(windows|linux|macos)\.conf\.' })
if ($overrides.Count -gt 0) { throw 'Review platform configuration overrides before releasing.' }

if ($Mode -eq 'Validate') {
    Write-Host "Validated version $version; external SDK resources and sidecars are excluded."
    return
}

$bundlePath = Join-Path $RepositoryRoot 'src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis'
$installers = @(Get-ChildItem -LiteralPath $bundlePath -File -Filter '*.exe')
if ($installers.Count -ne 1) { throw "Expected exactly one MSVC NSIS installer, found $($installers.Count)." }
$outputPath = Join-Path $RepositoryRoot 'release-assets'
if (Test-Path -LiteralPath $outputPath) { throw 'release-assets must not already exist; use a clean build checkout.' }
New-Item -ItemType Directory -Path $outputPath | Out-Null
$assetName = 'Nikon-Photo-Studio-windows-x64-setup.exe'
$assetPath = Join-Path $outputPath $assetName
Copy-Item -LiteralPath $installers[0].FullName -Destination $assetPath
$hash = (Get-FileHash -LiteralPath $assetPath -Algorithm SHA256).Hash.ToLowerInvariant()
"$hash  $assetName" | Set-Content -LiteralPath (Join-Path $outputPath 'SHA256SUMS.txt') -Encoding utf8NoBOM
$manifest = [ordered]@{
    schemaVersion = 1
    version = $version
    commit = $env:GITHUB_SHA
    ref = $env:GITHUB_REF
    runId = $env:GITHUB_RUN_ID
    target = 'x86_64-pc-windows-msvc'
    installer = $assetName
    sha256 = $hash
    proprietarySdkIncluded = $false
    sdkHelpersIncluded = $false
    fullCloudPresetRenderingVerified = $false
}
$manifest | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $outputPath 'release-manifest.json') -Encoding utf8NoBOM
Write-Host "Staged $assetName ($hash)"
