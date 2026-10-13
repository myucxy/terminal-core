[CmdletBinding()]
param(
    [string]$KotlinMethodsPath = (Join-Path $PSScriptRoot '../../termlink/android/terminal-engine-native/src/main/kotlin/dev/termlink/terminal/nativecore/JniNativeMethods.kt')
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$source = Get-Content -LiteralPath (Join-Path $PSScriptRoot '../android/jni/termlink_terminal_jni.cpp') -Raw
$optional = [regex]::Match($source, '(?s)const JNINativeMethod optional_methods\[\] = \{(?<body>.*?)\n\s*\};')
if (-not $optional.Success) { throw 'JNI optional registration table was not found.' }

$signatures = [ordered]@{
    encodeInputV1WithDiagnostic = '(J[J[B[[B[[B)I'
    encodeMouseWithDiagnostic = '(J[J[[B[[B)I'
    encodeInputV2WithDiagnostic = '(J[J[[B[[B)I'
    encodeInputV3WithDiagnostic = '(J[J[B[[B[[B)I'
}
$wrapperNames = @([regex]::Matches(
    $source,
    'Java_dev_termlink_terminal_nativecore_JniNativeMethods_(?<name>\w+WithDiagnostic)\s*\('
) | ForEach-Object { $_.Groups['name'].Value } | Sort-Object -Unique)
if (@($wrapperNames | Where-Object { -not $signatures.Contains($_) }).Count -gt 0 -or
    $wrapperNames.Count -ne $signatures.Count) {
    throw 'JNI WithDiagnostic wrapper list changed without updating the registration contract.'
}
$kotlin = if (Test-Path -LiteralPath $KotlinMethodsPath -PathType Leaf) {
    Get-Content -LiteralPath $KotlinMethodsPath -Raw
} else {
    $null
}
if ($null -ne $kotlin) {
    $kotlinNames = @([regex]::Matches($kotlin, 'external fun (?<name>\w+WithDiagnostic)\s*\(') |
        ForEach-Object { $_.Groups['name'].Value } | Sort-Object -Unique)
    if (@($kotlinNames | Where-Object { -not $signatures.Contains($_) }).Count -gt 0 -or
        $kotlinNames.Count -ne $signatures.Count) {
        throw 'Kotlin WithDiagnostic method list changed without updating the JNI registration contract.'
    }
}
$jniType = @{
    Long = 'J'
    LongArray = '[J'
    ByteArray = '[B'
    'Array<ByteArray?>' = '[[B'
}

foreach ($name in $signatures.Keys) {
    $signature = [regex]::Escape($signatures[$name])
    $symbol = 'Java_dev_termlink_terminal_nativecore_JniNativeMethods_' + $name
    if ($source -notmatch ([regex]::Escape($symbol) + '\s*\(')) {
        throw "JNI wrapper is missing: $name"
    }
    $registration = '(?s)\{const_cast<char\*>\("' + $name + '"\),\s*const_cast<char\*>\("' + $signature + '"\),\s*reinterpret_cast<void\*>\(' + [regex]::Escape($symbol) + '\)\}'
    if ($optional.Groups['body'].Value -notmatch $registration) {
        throw "JNI optional registration is missing or has the wrong signature: $name"
    }
    if ($null -ne $kotlin) {
        $declaration = [regex]::Match($kotlin, '(?s)external fun ' + $name + '\s*\((?<args>.*?)\)\s*:\s*Int')
        if (-not $declaration.Success) { throw "Kotlin external method is missing: $name" }
        $types = [regex]::Matches($declaration.Groups['args'].Value, ':\s*(LongArray|ByteArray|Long|Array<ByteArray\?>)')
        $actual = '(' + (($types | ForEach-Object { $jniType[$_.Groups[1].Value] }) -join '') + ')I'
        if ($actual -cne $signatures[$name]) {
            throw "Kotlin and JNI signatures differ for ${name}: $actual"
        }
    }
}

[pscustomobject]@{ RegisteredInputDiagnostics = $signatures.Count; KotlinVerified = $null -ne $kotlin }
