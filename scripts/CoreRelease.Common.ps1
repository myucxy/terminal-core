Set-StrictMode -Version Latest

function Get-CoreSha256([string]$Path) {
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Invoke-CoreTool([string]$Executable, [string[]]$Arguments) {
    & $Executable @Arguments | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "Tool failed: $Executable (exit $LASTEXITCODE)" }
}

function Get-CoreRevision([string]$Root, [switch]$Development) {
    $revision = & git -C $Root rev-parse HEAD
    if ($LASTEXITCODE -ne 0) { throw 'A Git checkout is required; see BUILD-SOURCE.md for source archives.' }
    $state = @(& git -C $Root status --porcelain --untracked-files=normal)
    if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect source checkout.' }
    if ($state.Count -and -not $Development) { throw 'Release builds require a clean committed checkout.' }
    [pscustomobject]@{ revision = $revision.Trim(); source_dirty = ($state.Count -ne 0) }
}

function Get-CoreSourceFingerprint([string]$Root) {
    $names = (& git -C $Root ls-files --cached --others --exclude-standard -z) -join "`n"
    if ($LASTEXITCODE -ne 0) { throw 'Cannot enumerate source checkout.' }
    $lines = foreach ($name in @($names.Split([char]0, [StringSplitOptions]::RemoveEmptyEntries) | Sort-Object -CaseSensitive)) {
        $path = Join-Path $Root $name
        if (Test-Path -LiteralPath $path -PathType Container) { throw "Source path is not a file: $name" }
        $hash = if (Test-Path -LiteralPath $path) { Get-CoreSha256 $path } else { 'MISSING' }
        "$($name.Length):${name}:$hash"
    }
    [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes(($lines -join "`n")))).ToLowerInvariant()
}

function Get-CoreArtifactPaths {
    'native/terminal-core/windows-x64/termlink_terminal_ffi.dll'
    foreach ($abi in @('arm64-v8a', 'armeabi-v7a', 'x86', 'x86_64')) {
        "native/terminal-core/android/$abi/libtermlink_terminal_jni.so"
    }
}

function Assert-CoreFilePath([string]$Root, [string]$Relative) {
    if ([string]::IsNullOrWhiteSpace($Relative) -or $Relative.Contains('\') -or
        $Relative.Contains(':') -or $Relative.StartsWith('/') -or
        @($Relative.Split('/') | Where-Object { $_ -in @('', '.', '..') }).Count) {
        throw "Unsafe relative path: $Relative"
    }
    $rootPath = [IO.Path]::GetFullPath($Root).TrimEnd('\', '/')
    $path = [IO.Path]::GetFullPath((Join-Path $rootPath $Relative))
    if (-not $path.StartsWith($rootPath + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Path escapes the selected root.'
    }
    $cursor = $path
    while ($cursor) {
        if ((Test-Path -LiteralPath $cursor) -and ((Get-Item -LiteralPath $cursor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw "Reparse points are not accepted: $cursor"
        }
        $parent = [IO.Path]::GetDirectoryName($cursor)
        if ($parent -eq $cursor) { break }
        $cursor = $parent
    }
    $path
}

function Assert-CoreNativeLayout([string]$Root, [switch]$RequireClean) {
    $lock = Get-Content -LiteralPath (Join-Path $Root 'terminal-core.lock.json') -Raw | ConvertFrom-Json
    if ($lock.schema_version -ne 2 -or $lock.revision -cnotmatch '^[0-9a-f]{40}$' -or
        $lock.source_dirty -isnot [bool] -or $lock.toolchain -cne '1.97.1' -or
        $lock.repository -cne 'git@github.com:myucxy/terminal-core.git') { throw 'Invalid core binary lock.' }
    if ($RequireClean -and $lock.source_dirty) { throw 'Dirty native libraries cannot be released or imported.' }
    $expected = @(Get-CoreArtifactPaths)
    if (@($lock.artifacts).Count -ne $expected.Count) { throw 'Exactly five native libraries are required.' }
    foreach ($name in $expected) {
        $records = @($lock.artifacts | Where-Object path -CEQ $name)
        if ($records.Count -ne 1) { throw "Missing or duplicate native entry: $name" }
        $record = $records[0]
        $file = Assert-CoreFilePath $Root $name
        if (-not (Test-Path -LiteralPath $file -PathType Leaf) -or $record.size -le 0 -or
            $record.sha256 -cnotmatch '^[0-9a-f]{64}$' -or
            (Get-Item -LiteralPath $file).Length -ne $record.size -or (Get-CoreSha256 $file) -cne $record.sha256) {
            throw "Native library hash/size mismatch: $name"
        }
    }
    $lock
}

function Expand-CoreZip([string]$Archive, [string]$Destination, [long]$MaxBytes = 1073741824) {
    if (Test-Path -LiteralPath $Destination) { throw 'ZIP destination must not already exist.' }
    Add-Type -AssemblyName System.IO.Compression
    $zip = [IO.Compression.ZipFile]::OpenRead($Archive)
    try {
        $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
        [long]$total = 0
        if ($zip.Entries.Count -gt 20000) { throw 'ZIP entry limit exceeded.' }
        foreach ($entry in $zip.Entries) {
            $name = $entry.FullName.TrimEnd('/')
            $null = Assert-CoreFilePath $Destination $name
            if (-not $names.Add($name)) { throw 'Duplicate ZIP path.' }
            $mode = ($entry.ExternalAttributes -shr 16) -band 0xF000
            if ($mode -notin @(0, 0x8000, 0x4000)) { throw 'ZIP links and special files are forbidden.' }
            $total += $entry.Length
            if ($total -gt $MaxBytes -or $entry.Length -gt $MaxBytes) { throw 'ZIP size limit exceeded.' }
        }
        $null = New-Item -ItemType Directory -Path $Destination
        foreach ($entry in $zip.Entries) {
            $target = Assert-CoreFilePath $Destination $entry.FullName.TrimEnd('/')
            if ($entry.FullName.EndsWith('/')) { $null = New-Item -ItemType Directory -Path $target -Force; continue }
            $null = New-Item -ItemType Directory -Path (Split-Path -Parent $target) -Force
            [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $target, $false)
        }
    } finally { $zip.Dispose() }
}

function New-CoreZip([string]$Source, [string]$Destination) {
    if (Test-Path -LiteralPath $Destination) { throw "Refusing to replace archive: $Destination" }
    Add-Type -AssemblyName System.IO.Compression
    [IO.Compression.ZipFile]::CreateFromDirectory($Source, $Destination, [IO.Compression.CompressionLevel]::Optimal, $false)
}
