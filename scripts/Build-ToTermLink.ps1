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
Set-StrictMode -Version Latest
$core = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$application = [IO.Path]::GetFullPath($TermLinkDirectory).TrimEnd('\', '/')
if (-not (Test-Path -LiteralPath (Join-Path $application 'android/settings.gradle.kts')) -or
    -not (Test-Path -LiteralPath (Join-Path $application 'win-agent'))) {
    throw 'TermLinkDirectory must be the application repository, not an installed client directory.'
}

function Invoke-Tool([string]$Executable, [string[]]$Arguments) {
    & $Executable @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Executable failed with exit code $LASTEXITCODE" }
}
function Read-Git([string[]]$Arguments) {
    $result = @(& git -C $core @Arguments)
    if ($LASTEXITCODE -ne 0) { throw 'Cannot read core source revision/state.' }
    return $result
}
function Get-SourceFingerprint {
    # Git lists tracked paths even when deleted and nonignored untracked paths.
    # NUL delimiters preserve names containing whitespace or newlines.
    $raw = (& git -C $core ls-files --cached --others --exclude-standard -z) -join "`n"
    if ($LASTEXITCODE -ne 0) { throw 'Cannot enumerate core source files.' }
    $paths = @($raw.Split([char]0, [StringSplitOptions]::RemoveEmptyEntries) | Sort-Object -CaseSensitive)
    $records = [Text.StringBuilder]::new()
    foreach ($relative in $paths) {
        $path = Join-Path $core $relative
        $hash = if (Test-Path -LiteralPath $path -PathType Leaf) {
            (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
        } elseif (Test-Path -LiteralPath $path) {
            throw "Core source path is not a file: $relative"
        } else {
            'MISSING'
        }
        [void]$records.Append($relative.Length).Append(':').Append($relative).Append(':').Append($hash).Append("`n")
    }
    return [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData(
        [Text.Encoding]::UTF8.GetBytes($records.ToString())))
}

$revision = (Read-Git @('rev-parse', 'HEAD')).Trim()
$dirty = @(Read-Git @('status', '--porcelain', '--untracked-files=normal')).Count -ne 0
if ($dirty -and -not $Development) { throw 'Commit core changes first, or explicitly use -Development for local-only binaries.' }
foreach ($tool in @($CargoPath, $AndroidCargoPath, $CMakePath, $NinjaPath,
    (Join-Path $NdkDirectory 'build/cmake/android.toolchain.cmake'))) {
    if (-not (Test-Path -LiteralPath $tool -PathType Leaf)) { throw "Required tool not found: $tool" }
}
$androidCargoVersion = & $AndroidCargoPath --version
if ($LASTEXITCODE -ne 0 -or $androidCargoVersion -notmatch '^cargo 1\.97\.1\b') { throw 'Android Cargo must be 1.97.1.' }
$sourceFingerprint = Get-SourceFingerprint
$dist = Join-Path $core 'dist'
$artifacts = @()
Push-Location $core
try {
    # Running inside core preserves its static-CRT .cargo/config.toml.
    Invoke-Tool $CargoPath @('+1.97.1', 'build', '-p', 'termlink-terminal-ffi', '--release', '--locked', '--offline', '--target', 'x86_64-pc-windows-msvc', '--target-dir', (Join-Path $core 'target'))
    $windowsRelative = 'native/terminal-core/windows-x64/termlink_terminal_ffi.dll'
    $windowsOutput = Join-Path $dist $windowsRelative
    $null = New-Item -ItemType Directory -Force -Path (Split-Path -Parent $windowsOutput)
    Copy-Item -LiteralPath (Join-Path $core 'target/x86_64-pc-windows-msvc/release/termlink_terminal_ffi.dll') -Destination $windowsOutput -Force
    $artifacts += @{ platform = 'windows'; path = $windowsRelative }

    foreach ($abi in @('arm64-v8a', 'armeabi-v7a', 'x86', 'x86_64')) {
        $build = Join-Path $core "target/android-jni/$abi"
        $relative = "native/terminal-core/android/$abi/libtermlink_terminal_jni.so"
        $output = Split-Path -Parent (Join-Path $dist $relative)
        Invoke-Tool $CMakePath @('-S', (Join-Path $core 'android/jni'), '-B', $build, '-G', 'Ninja',
            "-DCMAKE_MAKE_PROGRAM=$NinjaPath", "-DCMAKE_TOOLCHAIN_FILE=$NdkDirectory/build/cmake/android.toolchain.cmake",
            "-DANDROID_ABI=$abi", '-DANDROID_PLATFORM=android-26', "-DANDROID_NDK=$NdkDirectory",
            "-DTERMLINK_TERMINAL_CORE_ROOT=$core", "-DTERMLINK_CARGO=$AndroidCargoPath",
            "-DTERMLINK_JNI_OUTPUT_DIR=$output", '-DCMAKE_BUILD_TYPE=Release')
        Invoke-Tool $CMakePath @('--build', $build, '--target', 'termlink_terminal_jni')
        Invoke-Tool (Join-Path $NdkDirectory 'toolchains/llvm/prebuilt/windows-x86_64/bin/llvm-strip.exe') @('--strip-unneeded', (Join-Path $dist $relative))
        $artifacts += @{ platform = 'android'; path = $relative }
    }
} finally { Pop-Location }

foreach ($artifact in $artifacts) {
    $file = Get-Item -LiteralPath (Join-Path $dist $artifact.path)
    $artifact.size = $file.Length
    $artifact.sha256 = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
}
if ((Read-Git @('rev-parse', 'HEAD')).Trim() -cne $revision -or
    (Get-SourceFingerprint) -cne $sourceFingerprint -or
    (@(Read-Git @('status', '--porcelain', '--untracked-files=normal')).Count -ne 0) -ne $dirty) {
    throw 'Core checkout changed during build; refusing to export.'
}
$lock = [ordered]@{
    schema_version = 2
    repository = 'git@github.com:myucxy/terminal-core.git'
    revision = $revision
    source_dirty = $dirty
    toolchain = '1.97.1'
    artifacts = $artifacts
}
$lockJson = $lock | ConvertTo-Json -Depth 5
[IO.File]::WriteAllText((Join-Path $dist 'terminal-core.lock.json'), $lockJson, [Text.UTF8Encoding]::new($false))

# Compile all five libraries before touching the consumer. Write the lock last;
# an interrupted copy leaves mismatched hashes, which application builds reject.
foreach ($artifact in $artifacts) {
    $destination = Join-Path $application $artifact.path
    $null = New-Item -ItemType Directory -Force -Path (Split-Path -Parent $destination)
    Copy-Item -LiteralPath (Join-Path $dist $artifact.path) -Destination $destination -Force
    if ((Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant() -cne $artifact.sha256) {
        throw "Export hash mismatch: $($artifact.path)"
    }
}
$lockPath = Join-Path $application 'terminal-core.lock.json'
[IO.File]::WriteAllText("$lockPath.pending", $lockJson, [Text.UTF8Encoding]::new($false))
Move-Item -LiteralPath "$lockPath.pending" -Destination $lockPath -Force
[pscustomobject]@{ Application = $application; Revision = $revision; SourceDirty = $dirty; Libraries = $artifacts.Count }
