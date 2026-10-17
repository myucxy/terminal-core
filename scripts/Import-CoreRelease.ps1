[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$ReleaseDirectory,
    [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+(?:-rc\.\d+)?$')][string]$Version,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{40}$')][string]$ExpectedRevision,
    [Parameter(Mandatory)][string]$TermLinkDirectory,
    [Parameter(Mandatory)][string]$CosignPath,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{64}$')][string]$ExpectedPublicKeySha256
)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CoreRelease.Common.ps1')
$app=[IO.Path]::GetFullPath($TermLinkDirectory)
if (-not (Test-Path -LiteralPath (Join-Path $app 'android/settings.gradle.kts')) -or
    -not (Test-Path -LiteralPath (Join-Path $app 'win-agent'))) { throw 'Target must be a TermLink source checkout.' }
$verified=& (Join-Path $PSScriptRoot 'Verify-CoreReleasePackage.ps1') -ReleaseDirectory $ReleaseDirectory -CosignPath $CosignPath -ExpectedPublicKeySha256 $ExpectedPublicKeySha256
if ($verified.version -cne $Version -or $verified.revision -cne $ExpectedRevision) { throw 'Release does not match pinned version/revision.' }
$lock=Assert-CoreNativeLayout $verified.native_directory -RequireClean
$files=@(Get-CoreArtifactPaths)+@('terminal-core.lock.json')
# Copy rollback data outside the app source tree; write the new lock last.
$backup=Join-Path ([IO.Path]::GetTempPath()) ('termlink-core-import-'+[Guid]::NewGuid().ToString('N'))
$null=New-Item -ItemType Directory -Path $backup
$original=@{}
foreach ($relative in $files) {
    $target=Assert-CoreFilePath $app $relative
    $original[$relative]=Test-Path -LiteralPath $target -PathType Leaf
    if ($original[$relative]) {
        $saved=Assert-CoreFilePath $backup $relative
        $null=New-Item -ItemType Directory -Path (Split-Path -Parent $saved) -Force
        Copy-Item -LiteralPath $target -Destination $saved
    }
}
try {
    foreach ($relative in $files) {
        $target=Assert-CoreFilePath $app $relative
        $null=New-Item -ItemType Directory -Path (Split-Path -Parent $target) -Force
        Copy-Item -LiteralPath (Join-Path $verified.native_directory $relative) -Destination $target -Force
    }
    $null=Assert-CoreNativeLayout $app -RequireClean
} catch {
    foreach ($relative in $files) {
        $target=Assert-CoreFilePath $app $relative
        if ($original[$relative]) { Copy-Item -LiteralPath (Join-Path $backup $relative) -Destination $target -Force }
        elseif (Test-Path -LiteralPath $target) { Remove-Item -LiteralPath $target -Force }
    }
    throw
}
[pscustomobject]@{application=$app;version=$Version;revision=$lock.revision;libraries=5;rollback_directory=$backup}
