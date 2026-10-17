[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+(?:-rc\.\d+)?$')][string]$Version,
    [Parameter(Mandatory)][string]$NativeDirectory,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [string]$CargoPath = 'D:/Dev/Env/Rust/cargo/bin/cargo.exe'
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'CoreRelease.Common.ps1')
$core = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$source = Get-CoreRevision $core
$fingerprint = Get-CoreSourceFingerprint $core
foreach ($crate in @('termlink-terminal-core','termlink-terminal-ffi')) {
    $text=Get-Content -LiteralPath (Join-Path $core "crates/$crate/Cargo.toml") -Raw
    if ($text -notmatch ('(?m)^version\s*=\s*"'+[regex]::Escape($Version)+'"\s*$')) { throw 'Release and crate versions must match.' }
}
$native = [IO.Path]::GetFullPath($NativeDirectory)
$lock = Assert-CoreNativeLayout $native -RequireClean
if ($lock.revision -cne $source.revision) { throw 'Native libraries must match this clean source revision.' }
$output = [IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $output) { throw 'Release directory already exists; do not replace release assets.' }
$null = New-Item -ItemType Directory -Path $output
$stage = Join-Path $output 'staging'
$nativeStage = Join-Path $stage 'native'
$sourceStage = Join-Path $stage 'source'
$null = New-Item -ItemType Directory -Path $nativeStage -Force
foreach ($record in $lock.artifacts) {
    $target = Assert-CoreFilePath $nativeStage $record.path
    $null = New-Item -ItemType Directory -Path (Split-Path -Parent $target) -Force
    Copy-Item -LiteralPath (Join-Path $native $record.path) -Destination $target
}
foreach ($name in @('LICENSE','NOTICE')) { Copy-Item -LiteralPath (Join-Path $core $name) -Destination $nativeStage }
Copy-Item -LiteralPath (Join-Path $native 'terminal-core.lock.json') -Destination $nativeStage
Copy-Item -LiteralPath (Join-Path $core 'include') -Destination $nativeStage -Recurse
Copy-Item -LiteralPath (Join-Path $core 'third_party') -Destination $nativeStage -Recurse
$manifest = [ordered]@{
    schema_version=1;version=$Version;tag="v$Version";repository='myucxy/terminal-core';revision=$source.revision;
    source_dirty=$false;abi_version=1;snapshot_max='2.22';render_frame_max=5;rust='1.97.1';
    android_ndk='27.0.12077973';android_min_api=26;
    platforms=@('windows-x64','android-arm64-v8a','android-armeabi-v7a','android-x86','android-x86_64');
    artifacts=$lock.artifacts
}
$manifest | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $nativeStage 'core-release.json') -Encoding utf8
@'
# TermLink terminal-core native dependency package

Copy the five libraries under native/terminal-core and terminal-core.lock.json together.
Use the consuming application's verified C#/Kotlin adapters; those adapters are not part
of this core-only package. The public C header is in include/. Android JNI names target
dev.termlink.terminal.nativecore.JniNativeMethods. This is a GPL-3.0-only dependency,
with additional third-party notices in third_party/. Matching source is a sibling asset.

Validate the SHA256SUMS Cosign bundle against a separately trusted public-key fingerprint
before trusting archive hashes. Never select latest for production dependency resolution.
'@ | Set-Content -LiteralPath (Join-Path $nativeStage 'README-PACKAGE.md') -Encoding utf8
$trackedZip = Join-Path $stage 'tracked-source.zip'
Invoke-CoreTool 'git' @('-C',$core,'archive','--format=zip',"--output=$trackedZip",$source.revision)
Expand-CoreZip $trackedZip $sourceStage
$vendor = Join-Path $sourceStage 'vendor'
Push-Location $core
try {
    $vendorConfig = @(& $CargoPath +1.97.1 vendor --locked --offline --versioned-dirs $vendor)
    if ($LASTEXITCODE -ne 0) { throw 'Locked dependency vendoring failed.' }
} finally { Pop-Location }
# Merge source replacement with static CRT configuration; never overwrite that setting.
$portableVendorConfig=($vendorConfig -join "`n") -replace '(?m)^directory\s*=\s*"[^"]+"\s*$','directory = "vendor"'
if ($portableVendorConfig -notmatch 'directory = "vendor"') { throw 'Cargo did not produce a portable source replacement config.' }
$portableVendorConfig | Add-Content -LiteralPath (Join-Path $sourceStage '.cargo/config.toml') -Encoding utf8
Copy-Item -LiteralPath (Join-Path $nativeStage 'core-release.json') -Destination $sourceStage
@"
# Building the corresponding source for terminal-core v$Version

Original source revision: $($source.revision). This archive includes all tracked core
sources, JNI, CMake, scripts, Cargo.lock and vendored dependencies, without Git history.
Use Rust 1.97.1, MSVC/Windows SDK, Android NDK 27.0.12077973, CMake and Ninja.
Run from this source root so .cargo/config.toml supplies static CRT and vendor settings:

    cargo +1.97.1 test --workspace --all-targets --all-features --locked --offline
    cargo +1.97.1 build -p termlink-terminal-ffi --release --locked --offline --target x86_64-pc-windows-msvc

For all five targets, initialize a LOCAL Git snapshot (git init, git add ., git commit),
then run scripts/Build-NativeArtifacts.ps1 with explicit tool paths and a new output
directory. That snapshot has its own Git ID; SOURCE-MANIFEST.json records the original
release provenance. Do not rewrite the original release manifest to claim a rebuild
has the same byte hashes. Windows and Android C#/Kotlin application bindings are owned
by the TermLink application, not needed to rebuild the native core libraries.

The generator scripts use checked-in Unicode/charset/graphics data; no regeneration
or network access is needed. Dependencies are vendored and their license notices are
included. This archive and the native archive are both covered by signed SHA256SUMS.
"@ | Set-Content -LiteralPath (Join-Path $sourceStage 'BUILD-SOURCE.md') -Encoding utf8
$sourceFiles = @(Get-ChildItem -LiteralPath $sourceStage -Recurse -File | Sort-Object FullName | ForEach-Object {
    $relative = [IO.Path]::GetRelativePath($sourceStage,$_.FullName).Replace('\','/')
    $null = Assert-CoreFilePath $sourceStage $relative
    [ordered]@{path=$relative;size=$_.Length;sha256=(Get-CoreSha256 $_.FullName)}
})
[ordered]@{schema_version=1;revision=$source.revision;source_dirty=$false;version=$Version;files=$sourceFiles} |
    ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $sourceStage 'SOURCE-MANIFEST.json') -Encoding utf8
$null = Assert-CoreNativeLayout $nativeStage -RequireClean
if ((Get-CoreRevision $core).revision -cne $source.revision -or (Get-CoreSourceFingerprint $core) -cne $fingerprint) {
    throw 'Source changed while preparing release archives.'
}
$nativeName = "terminal-core-v$Version-native.zip"
$sourceName = "terminal-core-v$Version-source.zip"
New-CoreZip $nativeStage (Join-Path $output $nativeName)
New-CoreZip $sourceStage (Join-Path $output $sourceName)
$manifest.assets = @($nativeName,$sourceName | ForEach-Object {
    $path = Join-Path $output $_
    [ordered]@{name=$_;size=(Get-Item -LiteralPath $path).Length;sha256=(Get-CoreSha256 $path)}
})
$manifest | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $output 'core-release.json') -Encoding utf8
$lines = foreach ($name in @($nativeName,$sourceName,'core-release.json')) { "$(Get-CoreSha256 (Join-Path $output $name))  $name" }
[IO.File]::WriteAllText((Join-Path $output 'SHA256SUMS'), ($lines -join "`n")+"`n", [Text.UTF8Encoding]::new($false))
[pscustomobject]@{version=$Version;revision=$source.revision;output=$output;source_files=$sourceFiles.Count;libraries=5}
