param(
    [Parameter(Mandatory)][string]$AssetsPath,
    [switch]$ValidateOnly
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$expectedNames = @('Nikon-Photo-Studio-windows-x64-setup.exe', 'SHA256SUMS.txt', 'release-manifest.json')
$files = @(Get-ChildItem -LiteralPath $AssetsPath -Force)
if ($files.Count -ne $expectedNames.Count -or @($files | Where-Object { $_.PSIsContainer -or $_.Name -notin $expectedNames }).Count -gt 0) {
    throw 'Release assets must contain exactly the installer, SHA256SUMS.txt, and release-manifest.json.'
}
$manifest = Get-Content -Raw -LiteralPath (Join-Path $AssetsPath 'release-manifest.json') | ConvertFrom-Json
if ($manifest.schemaVersion -ne 1 -or $manifest.target -ne 'x86_64-pc-windows-msvc' -or
    $manifest.installer -ne $expectedNames[0] -or $manifest.version -notmatch '^\d+\.\d+\.\d+$' -or
    $manifest.commit -notmatch '^[a-f0-9]{40}$' -or $manifest.commit -ne $env:GITHUB_SHA -or
    $manifest.ref -ne $env:GITHUB_REF -or $manifest.proprietarySdkIncluded -ne $false -or
    $manifest.sdkHelpersIncluded -ne $false -or $manifest.fullCloudPresetRenderingVerified -ne $false) {
    throw 'Release manifest does not match this workflow commit, target, or SDK distribution policy.'
}
$installerPath = Join-Path $AssetsPath $manifest.installer
$hash = (Get-FileHash -LiteralPath $installerPath -Algorithm SHA256).Hash.ToLowerInvariant()
$checksumLine = (Get-Content -Raw -LiteralPath (Join-Path $AssetsPath 'SHA256SUMS.txt')).Trim()
if ($hash -ne $manifest.sha256 -or $checksumLine -ne "$hash  $($manifest.installer)") {
    throw 'Installer SHA256 does not match the checksum file and build manifest.'
}
$rolling = $env:GITHUB_REF -eq 'refs/heads/main'
$tag = if ($rolling) { 'latest' } else { "v$($manifest.version)" }
if (-not $rolling -and $env:GITHUB_REF -ne "refs/tags/$tag") { throw 'Only main or an exact vX.Y.Z tag can publish.' }
if ($ValidateOnly) {
    Write-Host "Verified $tag release inputs without contacting GitHub."
    return
}
if (-not $env:GH_TOKEN -or $env:GITHUB_REPOSITORY -notmatch '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$') {
    throw 'GitHub Actions repository and token environment are required.'
}
$apiBase = 'https://api.github.com'
$headers = @{ Authorization = "Bearer $env:GH_TOKEN"; Accept = 'application/vnd.github+json'; 'X-GitHub-Api-Version' = '2022-11-28' }
$repoPath = "/repos/$env:GITHUB_REPOSITORY"
function Invoke-ReleaseApi {
    param([string]$Method, [string]$Path, $Body = $null, [switch]$AllowNotFound)
    $request = @{ Method = $Method; Uri = "$apiBase$Path"; Headers = $headers }
    if ($null -ne $Body) {
        $request.Body = ConvertTo-Json -InputObject $Body -Depth 8 -Compress
        $request.ContentType = 'application/json; charset=utf-8'
    }
    try { Invoke-RestMethod @request }
    catch {
        if ($AllowNotFound -and $_.Exception.Response -and [int]$_.Exception.Response.StatusCode -eq 404) { return $null }
        throw
    }
}

if ($rolling) {
    $branch = Invoke-ReleaseApi GET "$repoPath/git/ref/heads/main"
    if ($branch.object.sha -ne $manifest.commit) {
        Write-Host 'A newer main commit exists. Keep the current release and let the newer run publish.'
        return
    }
}
$release = Invoke-ReleaseApi GET "$repoPath/releases/tags/$tag" -AllowNotFound
if ($release -and @($release.assets | Where-Object { $_.name -notin $expectedNames }).Count -gt 0) {
    throw 'The existing release contains unknown assets. Refusing to publish or delete unrelated attachments.'
}
if ($release -and $release.PSObject.Properties.Name -contains 'immutable' -and $release.immutable) {
    throw 'This release is immutable. Its tag and assets will not be changed.'
}
if ($release -and -not $rolling -and -not $release.draft) {
    throw "Published version $tag already exists; never replace a formal version. Create a new version tag."
}
$marker = '<!-- managed-by: nikon-photo-studio/windows-release -->'
if ($release -and -not $rolling -and
    ($release.target_commitish -ne $manifest.commit -or -not $release.body.Contains($marker))) {
    throw 'Refusing to modify an unrelated draft release.'
}
$notes = @"
$marker
Windows x64 installer built from $($manifest.commit).

Channel: $(if ($rolling) { 'main rolling prerelease; its assets are replaced after successful builds' } else { "version $tag; published assets are never overwritten by this workflow" }).

Download the setup executable and SHA256SUMS.txt. release-manifest.json records the exact commit and installer hash.

This installer does not contain Nikon proprietary SDKs, SDK helper executables, third-party NP3 presets, or research photographs. Nikon SDK features require a separate authorized local SDK setup. Full Nikon Imaging Cloud preset rendering is not verified by this build.

This workflow updates the release download. It does not implement automatic updates inside an installed application.
"@

if ($rolling) {
    $tagRef = Invoke-ReleaseApi GET "$repoPath/git/ref/tags/latest" -AllowNotFound
    if (-not $tagRef) {
        Invoke-ReleaseApi POST "$repoPath/git/refs" @{ ref = 'refs/tags/latest'; sha = $manifest.commit } | Out-Null
    }
} else {
    # Resolve annotated tags through the commits endpoint; never create or move a version tag.
    $tagCommit = Invoke-ReleaseApi GET "$repoPath/commits/$tag"
    if ($tagCommit.sha -ne $manifest.commit) { throw 'The version tag no longer points at the built commit.' }
}

$releaseBody = @{ tag_name = $tag; target_commitish = $manifest.commit; name = "Nikon Photo Studio $tag"; body = $notes; draft = $true; prerelease = $rolling; make_latest = 'false' }
if ($release) {
    # Hide the rolling release during replacement, so a partial upload is not a published release.
    $release = Invoke-ReleaseApi PATCH "$repoPath/releases/$($release.id)" $releaseBody
} else {
    $release = Invoke-ReleaseApi POST "$repoPath/releases" $releaseBody
}
foreach ($name in $expectedNames) {
    foreach ($oldAsset in @($release.assets | Where-Object { $_.name -eq $name })) {
        Invoke-ReleaseApi DELETE "$repoPath/releases/assets/$($oldAsset.id)" | Out-Null
    }
    $uploadBase = ($release.upload_url -split '\{')[0]
    if (-not $uploadBase.StartsWith('https://uploads.github.com/')) { throw 'Unexpected release upload host.' }
    $uploadUri = "${uploadBase}?name=$([Uri]::EscapeDataString($name))"
    Invoke-RestMethod -Method Post -Uri $uploadUri -Headers $headers -ContentType 'application/octet-stream' -InFile (Join-Path $AssetsPath $name) | Out-Null
}
if ($rolling) {
    Invoke-ReleaseApi PATCH "$repoPath/git/refs/tags/latest" @{ sha = $manifest.commit; force = $true } | Out-Null
}
$published = Invoke-ReleaseApi PATCH "$repoPath/releases/$($release.id)" @{ draft = $false; prerelease = $rolling; make_latest = 'false'; body = $notes }
Write-Host "Published $($published.html_url)"
