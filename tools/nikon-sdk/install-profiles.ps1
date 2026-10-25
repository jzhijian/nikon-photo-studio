param([Parameter(Mandatory=$true)][string]$SdkRoot)
$ErrorActionPreference = 'Stop'
$sdkBinaries = Join-Path (Resolve-Path -LiteralPath $SdkRoot).Path 'Module/Win/BinaryFile'
$sdkProfileDirectory = Join-Path $env:LOCALAPPDATA 'Nikon/NXTether'
$profileNames = @('DC_PTP_Config.config', 'MaidLayer.config', 'RangeValue.config')
foreach ($profileName in $profileNames) {
    if (-not (Test-Path -LiteralPath (Join-Path $sdkBinaries $profileName) -PathType Leaf)) {
        throw "Missing SDK profile: $profileName"
    }
}
$null = New-Item -ItemType Directory -Path $sdkProfileDirectory -Force
foreach ($profileName in $profileNames) {
    $source = Join-Path $sdkBinaries $profileName
    $destination = Join-Path $sdkProfileDirectory $profileName
    if (Test-Path -LiteralPath $destination) {
        $same = (Get-FileHash -LiteralPath $source).Hash -eq (Get-FileHash -LiteralPath $destination).Hash
        Write-Output "$profileName already exists; preserved (matches this SDK: $same)"
        continue
    }
    # File.Copy with overwrite=false also protects against a concurrent creation.
    [System.IO.File]::Copy($source, $destination, $false)
    Write-Output "Installed $profileName"
}
