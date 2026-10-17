[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$ReleaseDirectory,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{64}$')][string]$ExpectedPublicKeySha256,
    [Parameter(Mandatory)][string]$CosignPath,
    [string]$VerificationDirectory
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'CoreRelease.Common.ps1')
$release = [IO.Path]::GetFullPath($ReleaseDirectory)
$key = Join-Path $release 'core-signing.pub'
if ((Get-CoreSha256 $key) -cne $ExpectedPublicKeySha256) { throw 'Signing key does not match the independently pinned fingerprint.' }
Invoke-CoreTool $CosignPath @('verify-blob','--key',$key,'--bundle',(Join-Path $release 'SHA256SUMS.sigstore.json'),(Join-Path $release 'SHA256SUMS'))
$lines = @(Get-Content -LiteralPath (Join-Path $release 'SHA256SUMS'))
if ($lines.Count -ne 3) { throw 'Release must bind exactly native ZIP, source ZIP and release manifest.' }
$names = @()
foreach ($line in $lines) {
    if ($line -cnotmatch '^([0-9a-f]{64})  ([A-Za-z0-9.-]+)$') { throw 'Malformed signed checksum inventory.' }
    $expected=$Matches[1];$name=$Matches[2]
    $path = Assert-CoreFilePath $release $name
    if ((Get-CoreSha256 $path) -cne $expected) { throw "Release checksum mismatch: $name" }
    $names += $name
}
if (@($names | Sort-Object -Unique).Count -ne 3 -or 'core-release.json' -notin $names) { throw 'Duplicate or missing release assets.' }
$manifest = Get-Content -LiteralPath (Join-Path $release 'core-release.json') -Raw | ConvertFrom-Json
if ($manifest.schema_version -ne 1 -or $manifest.version -cnotmatch '^\d+\.\d+\.\d+(?:-rc\.\d+)?$' -or
    $manifest.tag -cne ('v'+$manifest.version) -or $manifest.revision -cnotmatch '^[0-9a-f]{40}$' -or
    $manifest.source_dirty -isnot [bool] -or $manifest.source_dirty -or $manifest.abi_version -ne 1 -or
    $manifest.repository -cne 'myucxy/terminal-core') { throw 'Invalid release identity.' }
$nativeName="terminal-core-v$($manifest.version)-native.zip"
$sourceName="terminal-core-v$($manifest.version)-source.zip"
if (@($manifest.assets).Count -ne 2 -or $nativeName -notin $names -or $sourceName -notin $names) { throw 'Unexpected release asset names.' }
foreach ($name in @($nativeName,$sourceName)) {
    $entry=@($manifest.assets | Where-Object name -CEQ $name)
    if ($entry.Count -ne 1 -or $entry[0].sha256 -cne (Get-CoreSha256 (Join-Path $release $name)) -or
        $entry[0].size -ne (Get-Item -LiteralPath (Join-Path $release $name)).Length) { throw 'Asset inventory differs from checksums.' }
}
if (-not $VerificationDirectory) { $VerificationDirectory=Join-Path ([IO.Path]::GetTempPath()) ('core-verify-'+[Guid]::NewGuid().ToString('N')) }
$verify = [IO.Path]::GetFullPath($VerificationDirectory)
if (Test-Path -LiteralPath $verify) { throw 'Verification directory must be new.' }
$null=New-Item -ItemType Directory -Path $verify
Expand-CoreZip (Join-Path $release $nativeName) (Join-Path $verify 'native') 268435456
Expand-CoreZip (Join-Path $release $sourceName) (Join-Path $verify 'source')
$native=Join-Path $verify 'native';$source=Join-Path $verify 'source'
$lock=Assert-CoreNativeLayout $native -RequireClean
if ($lock.revision -cne $manifest.revision) { throw 'Native/source revision mismatch.' }
$internal=Get-Content -LiteralPath (Join-Path $native 'core-release.json') -Raw | ConvertFrom-Json
if ($internal.version -cne $manifest.version -or $internal.revision -cne $manifest.revision -or $internal.source_dirty) { throw 'Inner native manifest identity mismatch.' }
$inventory=Get-Content -LiteralPath (Join-Path $source 'SOURCE-MANIFEST.json') -Raw | ConvertFrom-Json
if ($inventory.schema_version -ne 1 -or $inventory.revision -cne $manifest.revision -or
    $inventory.version -cne $manifest.version -or $inventory.source_dirty) { throw 'Invalid corresponding source identity.' }
$seen=[Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
foreach ($entry in $inventory.files) {
    if (-not $seen.Add($entry.path)) { throw 'Duplicate source inventory path.' }
    $file=Assert-CoreFilePath $source $entry.path
    if ($entry.sha256 -cnotmatch '^[0-9a-f]{64}$' -or (Get-CoreSha256 $file) -cne $entry.sha256 -or
        (Get-Item -LiteralPath $file).Length -ne $entry.size) { throw "Source inventory mismatch: $($entry.path)" }
}
if (@(Get-ChildItem -LiteralPath $source -File -Recurse).Count -ne $seen.Count+1) { throw 'Unlisted files in corresponding source.' }
foreach ($name in @('LICENSE','NOTICE','include/termlink_terminal.h','Cargo.lock','.cargo/config.toml','BUILD-SOURCE.md','scripts/Build-NativeArtifacts.ps1')) {
    if (-not $seen.Contains($name)) { throw "Missing source/build material: $name" }
}
if (-not @($inventory.files | Where-Object {$_.path.StartsWith('vendor/sha2-')} ).Count) { throw 'Vendored SHA-256 source is missing.' }
[pscustomobject]@{version=$manifest.version;revision=$manifest.revision;libraries=5;source_files=$seen.Count;
    public_key_sha256=$ExpectedPublicKeySha256;native_directory=$native;source_directory=$source;verification_directory=$verify}
