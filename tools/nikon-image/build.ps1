param(
    [Parameter(Mandatory=$true)][string]$SdkRoot,
    [string]$Compiler = 'clang++',
    [string]$OutputPath = (Join-Path $PSScriptRoot '../../.research-cloud/nikon-image-probe.exe')
)
$ErrorActionPreference = 'Stop'
$sdkHeaderDirectory = Join-Path (Resolve-Path -LiteralPath $SdkRoot).Path 'Library/win/Include'
if (-not (Test-Path -LiteralPath (Join-Path $sdkHeaderDirectory 'Nkfl_Interface.h'))) {
    throw 'SdkRoot must contain Library/win/Include/Nkfl_Interface.h.'
}
$compilerPath = (Get-Command $Compiler -ErrorAction Stop).Source
$probeOutput = [System.IO.Path]::GetFullPath($OutputPath)
$null = New-Item -ItemType Directory -Path ([System.IO.Path]::GetDirectoryName($probeOutput)) -Force
$previousPath = $env:PATH
try {
    $env:PATH = [System.IO.Path]::GetDirectoryName($compilerPath) + ';' + $previousPath
    & $compilerPath -std=c++17 -fno-exceptions -fno-rtti -nostdlib++ -nostdinc++ `
        -Wno-ignored-attributes -municode -static -I $sdkHeaderDirectory `
        (Join-Path $PSScriptRoot 'probe.cpp') -lole32 -o $probeOutput
    if ($LASTEXITCODE -ne 0) { throw "Image SDK probe build failed: $LASTEXITCODE" }
} finally { $env:PATH = $previousPath }
Write-Output $probeOutput
