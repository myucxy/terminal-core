[CmdletBinding()]
param(
    [string]$OutputDirectory = (Join-Path $PSScriptRoot '../dist/native-build'),
    [string]$CargoPath = 'D:/Dev/Env/Rust/cargo/bin/cargo.exe',
    [string]$AndroidCargoPath = 'D:/Dev/Env/Rust/rustup/toolchains/1.97.1-x86_64-pc-windows-msvc/bin/cargo.exe',
    [string]$NdkDirectory = 'D:/Dev/Env/Android/Sdk/ndk/27.0.12077973',
    [string]$CMakePath = 'D:/Dev/Env/msys64/mingw64/bin/cmake.exe',
    [string]$NinjaPath = 'D:/Dev/Env/msys64/mingw64/bin/ninja.exe',
    [switch]$Development
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'CoreRelease.Common.ps1')
$core = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$output = [IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $output) { throw 'Use a new native output directory; existing builds are never overwritten.' }
$source = Get-CoreRevision $core -Development:$Development
$fingerprint = Get-CoreSourceFingerprint $core
foreach ($tool in @($CargoPath, $AndroidCargoPath, $CMakePath, $NinjaPath,
    (Join-Path $NdkDirectory 'build/cmake/android.toolchain.cmake'))) {
    if (-not (Test-Path -LiteralPath $tool -PathType Leaf)) { throw "Required tool missing: $tool" }
}
if ((& $AndroidCargoPath --version) -notmatch '^cargo 1\.97\.1\b') { throw 'Android Cargo must be 1.97.1.' }
$null = New-Item -ItemType Directory -Path $output
$artifacts = @()
Push-Location $core
try {
    Invoke-CoreTool $CargoPath @('+1.97.1','build','-p','termlink-terminal-ffi','--release','--locked','--offline','--target','x86_64-pc-windows-msvc','--target-dir',(Join-Path $core 'target'))
    $relative = 'native/terminal-core/windows-x64/termlink_terminal_ffi.dll'
    $target = Assert-CoreFilePath $output $relative
    $null = New-Item -ItemType Directory -Path (Split-Path -Parent $target) -Force
    Copy-Item -LiteralPath (Join-Path $core 'target/x86_64-pc-windows-msvc/release/termlink_terminal_ffi.dll') -Destination $target
    $artifacts += @{ platform = 'windows'; path = $relative }
    foreach ($abi in @('arm64-v8a','armeabi-v7a','x86','x86_64')) {
        $build = Join-Path $core "target/android-jni/$abi"
        $relative = "native/terminal-core/android/$abi/libtermlink_terminal_jni.so"
        $target = Assert-CoreFilePath $output $relative
        Invoke-CoreTool $CMakePath @('-S',(Join-Path $core 'android/jni'),'-B',$build,'-G','Ninja',
            "-DCMAKE_MAKE_PROGRAM=$NinjaPath","-DCMAKE_TOOLCHAIN_FILE=$NdkDirectory/build/cmake/android.toolchain.cmake",
            "-DANDROID_ABI=$abi",'-DANDROID_PLATFORM=android-26',"-DANDROID_NDK=$NdkDirectory",
            "-DTERMLINK_TERMINAL_CORE_ROOT=$core","-DTERMLINK_CARGO=$AndroidCargoPath",
            "-DTERMLINK_JNI_OUTPUT_DIR=$(Split-Path -Parent $target)",'-DCMAKE_BUILD_TYPE=Release')
        Invoke-CoreTool $CMakePath @('--build',$build,'--target','termlink_terminal_jni')
        Invoke-CoreTool (Join-Path $NdkDirectory 'toolchains/llvm/prebuilt/windows-x86_64/bin/llvm-strip.exe') @('--strip-unneeded',$target)
        $artifacts += @{ platform = 'android'; path = $relative }
    }
} finally { Pop-Location }
foreach ($record in $artifacts) {
    $file = Join-Path $output $record.path
    $record.size = (Get-Item -LiteralPath $file).Length
    $record.sha256 = Get-CoreSha256 $file
}
$after = Get-CoreRevision $core -Development:$Development
if ($after.revision -cne $source.revision -or $after.source_dirty -ne $source.source_dirty -or
    (Get-CoreSourceFingerprint $core) -cne $fingerprint) { throw 'Source changed during the build; no usable lock was written.' }
$lock = [ordered]@{schema_version=2;repository='git@github.com:myucxy/terminal-core.git';revision=$source.revision;
    source_dirty=$source.source_dirty;toolchain='1.97.1';artifacts=$artifacts}
$lock | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $output 'terminal-core.lock.json') -Encoding utf8
$null = Assert-CoreNativeLayout $output -RequireClean:(-not $Development)
[pscustomobject]@{output=$output;revision=$source.revision;source_dirty=$source.source_dirty;libraries=5}
