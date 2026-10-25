$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$testRoot = Join-Path ([IO.Path]::GetTempPath()) ('nikon-release-test-' + [Guid]::NewGuid().ToString('N'))
$savedEnvironment = @{}
foreach ($name in @('GITHUB_REF', 'GITHUB_SHA', 'GITHUB_RUN_ID', 'GITHUB_REPOSITORY', 'GH_TOKEN')) {
    $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name)
}
function Assert-Rejected {
    param([scriptblock]$Action, [string]$Message)
    $rejected = $false
    try { & $Action } catch { $rejected = $true }
    if (-not $rejected) { throw "Expected rejection: $Message" }
}
try {
    $env:GITHUB_REF = 'refs/heads/main'
    $env:GITHUB_SHA = '0123456789abcdef0123456789abcdef01234567'
    $env:GITHUB_RUN_ID = '123'
    $tauriPath = Join-Path $testRoot 'src-tauri'
    $nsisPath = Join-Path $tauriPath 'target/x86_64-pc-windows-msvc/release/bundle/nsis'
    New-Item -ItemType Directory -Path $nsisPath -Force | Out-Null
    '{"version":"1.2.3"}' | Set-Content -LiteralPath (Join-Path $testRoot 'package.json')
    "[package]`nname = `"release-test`"`nversion = `"1.2.3`"" | Set-Content -LiteralPath (Join-Path $tauriPath 'Cargo.toml')
    $config = @{ version = '1.2.3'; build = @{ frontendDist = '../dist' }; bundle = @{ active = $true } }
    $configPath = Join-Path $tauriPath 'tauri.conf.json'
    $config | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $configPath
    & (Join-Path $PSScriptRoot 'prepare.ps1') -Mode Validate -RepositoryRoot $testRoot
    $env:GITHUB_REF = 'refs/tags/v9.9.9'
    Assert-Rejected { & (Join-Path $PSScriptRoot 'prepare.ps1') -RepositoryRoot $testRoot } 'tag/version mismatch'
    $env:GITHUB_REF = 'refs/heads/main'
    $config.bundle.resources = @('private-sdk.dll')
    $config | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $configPath
    Assert-Rejected { & (Join-Path $PSScriptRoot 'prepare.ps1') -RepositoryRoot $testRoot } 'external SDK resources'
    $config.bundle.Remove('resources')
    $config | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $configPath
    # Synthetic bytes test the manifest/checksum pipeline, never a runnable installer.
    [IO.File]::WriteAllBytes((Join-Path $nsisPath 'synthetic-setup.exe'), [byte[]](77, 90, 1, 2, 3, 4))
    & (Join-Path $PSScriptRoot 'prepare.ps1') -Mode Stage -RepositoryRoot $testRoot
    $assetsPath = Join-Path $testRoot 'release-assets'
    & (Join-Path $PSScriptRoot 'publish.ps1') -AssetsPath $assetsPath -ValidateOnly
    'private-data' | Set-Content -LiteralPath (Join-Path $assetsPath 'unexpected.txt')
    Assert-Rejected { & (Join-Path $PSScriptRoot 'publish.ps1') -AssetsPath $assetsPath -ValidateOnly } 'unexpected artifact'
    Remove-Item -LiteralPath (Join-Path $assetsPath 'unexpected.txt')
    'bad checksum' | Set-Content -LiteralPath (Join-Path $assetsPath 'SHA256SUMS.txt')
    Assert-Rejected { & (Join-Path $PSScriptRoot 'publish.ps1') -AssetsPath $assetsPath -ValidateOnly } 'checksum mismatch'
    $manifestPath = Join-Path $assetsPath 'release-manifest.json'
    $testManifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json
    "$($testManifest.sha256)  $($testManifest.installer)" | Set-Content -LiteralPath (Join-Path $assetsPath 'SHA256SUMS.txt')

    # Intercept every REST request: these publication tests never use the network.
    $env:GH_TOKEN = 'local-test-placeholder'
    $env:GITHUB_REPOSITORY = 'release-test/synthetic'
    $mockState = @{ Calls = [Collections.Generic.List[object]]::new(); NewerMain = $false; ExtraAsset = $false }
    Set-Item -Path Function:Invoke-RestMethod -Value {
        param($Method, $Uri, $Headers, $Body, $ContentType, $InFile)
        $mockState.Calls.Add(@{ method = $Method; uri = $Uri; body = $Body })
        if ($Method -eq 'GET' -and $Uri -like '*/git/ref/heads/main') {
            return @{ object = @{ sha = $(if ($mockState.NewerMain) { 'ffffffffffffffffffffffffffffffffffffffff' } else { $env:GITHUB_SHA }) } }
        }
        if ($Method -eq 'GET' -and $Uri -like '*/git/ref/tags/latest') { return @{ object = @{ sha = 'old' } } }
        if (($Method -eq 'GET' -and $Uri -like '*/releases/tags/*') -or ($Method -eq 'PATCH' -and $Uri -like '*/releases/42')) {
            return @{ id = 42; draft = $false; immutable = $false; target_commitish = $env:GITHUB_SHA;
                body = '<!-- managed-by: nikon-photo-studio/windows-release -->'; assets = $(if ($mockState.ExtraAsset) { @(@{ name = 'unknown-attachment.zip' }) } else { @() });
                upload_url = 'https://uploads.github.com/repos/release-test/synthetic/releases/42/assets{?name,label}';
                html_url = 'https://github.com/release-test/synthetic/releases/tag/latest' }
        }
        if ($Method -eq 'POST' -and $Uri -like 'https://uploads.github.com/*') { return @{ id = 100 } }
        if ($Method -eq 'PATCH' -and $Uri -like '*/git/refs/tags/latest') { return @{ object = @{ sha = $env:GITHUB_SHA } } }
        throw "Unexpected mocked request: $Method $Uri"
    }.GetNewClosure()
    & (Join-Path $PSScriptRoot 'publish.ps1') -AssetsPath $assetsPath
    $tagUpdates = @($mockState.Calls | Where-Object { $_.method -eq 'PATCH' -and $_.uri -like '*/git/refs/tags/latest' })
    if ($tagUpdates.Count -ne 1 -or ($tagUpdates[0].body | ConvertFrom-Json).sha -ne $env:GITHUB_SHA) {
        throw 'Rolling release did not move latest to the built commit.'
    }
    $mockState.Calls.Clear()
    $mockState.ExtraAsset = $true
    Assert-Rejected { & (Join-Path $PSScriptRoot 'publish.ps1') -AssetsPath $assetsPath } 'unknown old release asset'
    if (@($mockState.Calls | Where-Object { $_.method -ne 'GET' }).Count) { throw 'Release with unknown old assets was modified.' }
    $mockState.ExtraAsset = $false
    $mockState.Calls.Clear()
    $mockState.NewerMain = $true
    & (Join-Path $PSScriptRoot 'publish.ps1') -AssetsPath $assetsPath
    if (@($mockState.Calls | Where-Object { $_.method -ne 'GET' }).Count) { throw 'Stale main build performed a mutation.' }
    $mockState.NewerMain = $false
    $mockState.Calls.Clear()
    $env:GITHUB_REF = 'refs/tags/v1.2.3'
    $testManifest.ref = $env:GITHUB_REF
    $testManifest | ConvertTo-Json | Set-Content -LiteralPath $manifestPath
    Assert-Rejected { & (Join-Path $PSScriptRoot 'publish.ps1') -AssetsPath $assetsPath } 'published formal version overwrite'
    if (@($mockState.Calls | Where-Object { $_.method -ne 'GET' }).Count) { throw 'Existing formal release was modified.' }
    Write-Host 'Release validation passed: input guards, checksums, latest tag update, stale main skip, and formal-release preservation (REST mocked).'
} finally {
    foreach ($name in $savedEnvironment.Keys) {
        [Environment]::SetEnvironmentVariable($name, $savedEnvironment[$name])
    }
    # Delete only this script's freshly created, resolved temporary directory.
    if (Test-Path -LiteralPath $testRoot) {
        $resolvedTest = (Resolve-Path -LiteralPath $testRoot).Path
        $resolvedTemp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\', '/')
        if ([IO.Path]::GetDirectoryName($resolvedTest) -ne $resolvedTemp -or
            [IO.Path]::GetFileName($resolvedTest) -notmatch '^nikon-release-test-[a-f0-9]{32}$') {
            throw "Refusing to remove unexpected test directory: $resolvedTest"
        }
        Remove-Item -LiteralPath $resolvedTest -Recurse -Force
    }
}
