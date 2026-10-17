[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$ReleaseDirectory,
    [Parameter(Mandatory)][string]$CosignPath,
    [Parameter(Mandatory)][string]$PrivateKeyPath,
    [Parameter(Mandatory)][string]$PublicKeyPath,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{64}$')][string]$ExpectedPublicKeySha256
)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CoreRelease.Common.ps1')
$release=[IO.Path]::GetFullPath($ReleaseDirectory)
if ((Get-CoreSha256 $PublicKeyPath) -cne $ExpectedPublicKeySha256) { throw 'Unexpected release signing key.' }
$bundle=Join-Path $release 'SHA256SUMS.sigstore.json'
if (Test-Path -LiteralPath $bundle) { throw 'A signature already exists; verify it instead of overwriting.' }
# Passwords, if required, are supplied by Cosign's protected caller environment.
# No secret is accepted as a command-line argument or copied into the release.
Copy-Item -LiteralPath $PublicKeyPath -Destination (Join-Path $release 'core-signing.pub')
Invoke-CoreTool $CosignPath @('sign-blob','--yes','--key',[IO.Path]::GetFullPath($PrivateKeyPath),
    '--bundle',$bundle,(Join-Path $release 'SHA256SUMS'))
& (Join-Path $PSScriptRoot 'Verify-CoreReleasePackage.ps1') -ReleaseDirectory $release -CosignPath $CosignPath -ExpectedPublicKeySha256 $ExpectedPublicKeySha256
