#Requires -RunAsAdministrator
param([Parameter(Mandatory=$true)][string]$SdkRoot)
$ErrorActionPreference = 'Stop'
$sourceDirectory = Join-Path (Resolve-Path -LiteralPath $SdkRoot).Path 'Library/win/Profiles'
if (-not (Test-Path -LiteralPath (Join-Path $sourceDirectory 'NKsRGB.icm') -PathType Leaf)) {
    throw 'SdkRoot does not contain the official Nikon Image SDK profiles.'
}
$destinationDirectory = Join-Path ${env:CommonProgramFiles} 'Nikon/Profiles'
$null = New-Item -ItemType Directory -Path $destinationDirectory -Force
$installed = 0
$preserved = 0
foreach ($source in Get-ChildItem -LiteralPath $sourceDirectory -File) {
    $destination = Join-Path $destinationDirectory $source.Name
    if (Test-Path -LiteralPath $destination) {
        $preserved++
        $matches = (Get-FileHash -LiteralPath $source.FullName).Hash -eq (Get-FileHash -LiteralPath $destination).Hash
        Write-Output "Preserved $($source.Name) (matches this SDK: $matches)"
        continue
    }
    [System.IO.File]::Copy($source.FullName, $destination, $false)
    $installed++
}
[pscustomobject]@{ destination=$destinationDirectory; installed=$installed; preserved=$preserved } | ConvertTo-Json
