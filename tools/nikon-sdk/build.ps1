param(
    [Parameter(Mandatory=$true)][string]$SdkRoot,
    [string]$Compiler = 'clang++',
    [string]$OutputPath = (Join-Path $PSScriptRoot '../../.research-cloud/nikon-sdk-probe.exe')
)
$ErrorActionPreference = 'Stop'
$sdkHeaderDirectory = Join-Path (Resolve-Path -LiteralPath $SdkRoot).Path 'Module/Win/HeaderFile'
if (-not (Test-Path -LiteralPath (Join-Path $sdkHeaderDirectory 'Maid3.h'))) {
    throw 'SdkRoot must contain the official Module/Win/HeaderFile directory.'
}
$compilerPath = (Get-Command $Compiler -ErrorAction Stop).Source
$probeOutput = [System.IO.Path]::GetFullPath($OutputPath)
$null = New-Item -ItemType Directory -Path ([System.IO.Path]::GetDirectoryName($probeOutput)) -Force
# The unmodified Nikon headers use Microsoft enum forward declarations.
# Clang supports those in full Microsoft compatibility mode; GCC does not.
# The helper uses C/Win32 facilities only, avoiding mixed C++ runtime ABIs.
$previousPath = $env:PATH
try {
    $env:PATH = [System.IO.Path]::GetDirectoryName($compilerPath) + ';' + $previousPath
    & $compilerPath -std=c++17 -fms-extensions -fms-compatibility '-fms-compatibility-version=19.40' '-fgnuc-version=4.2.1' `
        -fno-exceptions -fno-rtti -nostdlib++ -nostdinc++ -Wno-ignored-attributes `
        -D__LITTLE_ENDIAN__=1 -DTARGET_CPU_X86=0 -DTARGET_CPU_X86_64=1 `
        -DTARGET_CPU_PPC=0 -DTARGET_CPU_PPC64=0 -municode -static `
        -I $sdkHeaderDirectory (Join-Path $PSScriptRoot 'probe.cpp') -o $probeOutput
    if ($LASTEXITCODE -ne 0) { throw "SDK probe build failed with exit code $LASTEXITCODE" }
} finally { $env:PATH = $previousPath }
Write-Output $probeOutput
