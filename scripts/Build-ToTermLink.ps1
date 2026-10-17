[CmdletBinding()]
param(
    [string]$TermLinkDirectory = (Join-Path $PSScriptRoot '../../termlink'),
    [string]$CargoPath = 'D:/Dev/Env/Rust/cargo/bin/cargo.exe',
    [string]$AndroidCargoPath = 'D:/Dev/Env/Rust/rustup/toolchains/1.97.1-x86_64-pc-windows-msvc/bin/cargo.exe',
    [string]$NdkDirectory = 'D:/Dev/Env/Android/Sdk/ndk/27.0.12077973',
    [string]$CMakePath = 'D:/Dev/Env/msys64/mingw64/bin/cmake.exe',
    [string]$NinjaPath = 'D:/Dev/Env/msys64/mingw64/bin/ninja.exe',
    [switch]$Development
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'CoreRelease.Common.ps1')
$application = [IO.Path]::GetFullPath($TermLinkDirectory).TrimEnd('\','/')
if (-not (Test-Path -LiteralPath (Join-Path $application 'android/settings.gradle.kts')) -or
    -not (Test-Path -LiteralPath (Join-Path $application 'win-agent'))) {
    throw 'TermLinkDirectory must be an application source checkout.'
}
$output = Join-Path $PSScriptRoot ('../dist/local-export-'+[Guid]::NewGuid().ToString('N'))
$build = & (Join-Path $PSScriptRoot 'Build-NativeArtifacts.ps1') -OutputDirectory $output `
    -CargoPath $CargoPath -AndroidCargoPath $AndroidCargoPath -NdkDirectory $NdkDirectory `
    -CMakePath $CMakePath -NinjaPath $NinjaPath -Development:$Development
$lock = Assert-CoreNativeLayout $build.output -RequireClean:(-not $Development)
# Convenience for a local build; release downloads use Import-CoreRelease.ps1.
foreach ($relative in @(Get-CoreArtifactPaths)+@('terminal-core.lock.json')) {
    $destination = Assert-CoreFilePath $application $relative
    $null = New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force
    Copy-Item -LiteralPath (Join-Path $build.output $relative) -Destination $destination -Force
}
$null = Assert-CoreNativeLayout $application -RequireClean:(-not $Development)
[pscustomobject]@{Application=$application;Revision=$lock.revision;SourceDirty=$lock.source_dirty;Libraries=5;BuildDirectory=$build.output}
