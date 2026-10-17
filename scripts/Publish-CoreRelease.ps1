[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('Inspect','Upload','Publish','Download')][string]$Action,
    [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+(?:-rc\.\d+)?$')][string]$Version,
    [string]$ReleaseDirectory,
    [string]$DownloadDirectory,
    [string]$NotesPath,
    [string]$TokenFile,
    [string]$CosignPath,
    [ValidatePattern('^[0-9a-f]{64}$')][string]$ExpectedPublicKeySha256
)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CoreRelease.Common.ps1')
$token=$null
if ($TokenFile) { $token=(Get-Content -LiteralPath $TokenFile -Raw).Trim() }
elseif ($env:GH_TOKEN) { $token=$env:GH_TOKEN }
elseif ($env:GITHUB_TOKEN) { $token=$env:GITHUB_TOKEN }
else {
    # Use the approved Git Credential Manager OAuth login without persisting tokens here.
    $savedInteractive=$env:GCM_INTERACTIVE
    $savedPrompt=$env:GIT_TERMINAL_PROMPT
    try {
        $env:GCM_INTERACTIVE='never';$env:GIT_TERMINAL_PROMPT='0'
        $credential=@("protocol=https`nhost=github.com`nusername=myucxy`n`n" | & git credential fill 2>$null)
        if ($LASTEXITCODE -eq 0) {
            foreach ($line in $credential) { if ($line.StartsWith('password=')) { $token=$line.Substring(9) } }
        }
    } finally {
        $credential=$null;$env:GCM_INTERACTIVE=$savedInteractive;$env:GIT_TERMINAL_PROMPT=$savedPrompt
    }
}
$headers=@{Accept='application/vnd.github+json';'X-GitHub-Api-Version'='2026-03-10';'User-Agent'='TermLink-Core-Release'}
if ($token) { $headers.Authorization='Bearer '+$token }
$api='https://api.github.com/repos/myucxy/terminal-core'
$tag="v$Version"
function Invoke-ReleaseApi([string]$Method,[string]$Uri,$Body=$null) {
    if ($Uri -cne $api -and -not $Uri.StartsWith($api+'/',[StringComparison]::Ordinal)) { throw 'Unexpected GitHub API endpoint.' }
    $args=@{Method=$Method;Uri=$Uri;Headers=$headers;TimeoutSec=90}
    if ($null -ne $Body) { $args.Body=$Body|ConvertTo-Json -Depth 8 -Compress;$args.ContentType='application/json' }
    try {
        $response=Invoke-RestMethod @args
        if ($response -is [array]) { foreach ($item in $response) { $item } }
        else { $response }
    }
    catch {
        $status=if ($_.Exception.Response) {[int]$_.Exception.Response.StatusCode} else {0}
        throw "GitHub API $Method failed (HTTP $status). Check repository access and Contents permission."
    }
}
function Download-ReleaseAsset($Asset,[string]$Directory) {
    $target=Assert-CoreFilePath $Directory $Asset.name
    if (Test-Path -LiteralPath $target) { throw 'Download destination file exists.' }
    $uri=[string]$Asset.url
    if ($uri -cnotmatch '^https://api\.github\.com/repos/myucxy/terminal-core/releases/assets/[0-9]+$') { throw 'Unexpected asset endpoint.' }
    $downloadHeaders=$headers.Clone();$downloadHeaders.Accept='application/octet-stream'
    # PowerShell strips Authorization when redirects change host; do not preserve it.
    Invoke-WebRequest -Uri $uri -Headers $downloadHeaders -OutFile $target -TimeoutSec 300 | Out-Null
    if ((Get-Item -LiteralPath $target).Length -ne $Asset.size) { throw 'Downloaded asset size mismatch.' }
    if ($Asset.digest -and $Asset.digest -cne ('sha256:'+(Get-CoreSha256 $target))) { throw 'Downloaded GitHub asset digest mismatch.' }
}
try {
    if ($Action -in @('Upload','Publish') -and -not $token) { throw 'Authorize Git Credential Manager, set GH_TOKEN/GITHUB_TOKEN or provide -TokenFile with Contents:write permission. Never put a token in a command argument.' }
    $repo=Invoke-ReleaseApi 'GET' $api
    $releases=@(Invoke-ReleaseApi 'GET' ($api+'/releases?per_page=100'))
    $matches=@($releases|Where-Object tag_name -CEQ $tag)
    if ($matches.Count -gt 1) { throw 'Multiple releases use the same tag.' }
    $release=if($matches.Count){$matches[0]}else{$null}
    if ($Action -eq 'Inspect') {
        [pscustomobject]@{repository=$repo.full_name;private=$repo.private;tag=$tag;release_exists=($null -ne $release);
            releases=@($releases|Select-Object tag_name,draft,prerelease,html_url)}
        return
    }
    $assetNames=@("terminal-core-v$Version-native.zip","terminal-core-v$Version-source.zip",'core-release.json','SHA256SUMS','SHA256SUMS.sigstore.json','core-signing.pub')
    if ($Action -in @('Upload','Publish')) {
        if (-not $CosignPath -or -not $ExpectedPublicKeySha256 -or -not $ReleaseDirectory) { throw 'Signed package verification inputs are required.' }
        $verified=& (Join-Path $PSScriptRoot 'Verify-CoreReleasePackage.ps1') -ReleaseDirectory $ReleaseDirectory -CosignPath $CosignPath -ExpectedPublicKeySha256 $ExpectedPublicKeySha256
        if ($verified.version -cne $Version) { throw 'Version differs from the verified package.' }
        if (-not $repo.permissions.push) { throw 'Repository write permission is required.' }
    }
    if ($Action -eq 'Upload') {
        if (-not $NotesPath -or -not (Test-Path -LiteralPath $NotesPath -PathType Leaf)) { throw 'Bilingual release notes are required.' }
        if ($release -and -not $release.draft) { throw 'Published releases are immutable to this script.' }
        $refs=@(Invoke-ReleaseApi 'GET' ($api+'/git/matching-refs/tags/'+$tag))
        $exact=@($refs|Where-Object ref -CEQ ('refs/tags/'+$tag))
        if ($exact.Count) {
            $object=$exact[0].object
            while ($object.type -eq 'tag') { $object=(Invoke-ReleaseApi 'GET' ($api+'/git/tags/'+$object.sha)).object }
            if ($object.type -cne 'commit' -or $object.sha -cne $verified.revision) { throw 'Existing tag targets different source.' }
        }
        if (-not $release) {
            $release=Invoke-ReleaseApi 'POST' ($api+'/releases') @{
                tag_name=$tag;target_commitish=$verified.revision;name="terminal-core $tag";
                body=(Get-Content -LiteralPath $NotesPath -Raw);draft=$true;prerelease=$Version.Contains('-');make_latest='false'
            }
        }
        foreach ($name in $assetNames) {
            $path=Assert-CoreFilePath $ReleaseDirectory $name
            $assets=@(Invoke-ReleaseApi 'GET' ($api+'/releases/'+$release.id+'/assets?per_page=100'))
            $existing=@($assets|Where-Object name -CEQ $name)
            if ($existing.Count) {
                if ($existing.Count -ne 1 -or $existing[0].state -cne 'uploaded' -or
                    $existing[0].digest -cne ('sha256:'+(Get-CoreSha256 $path))) { throw 'Existing asset differs; no asset was replaced.' }
                continue
            }
            $upload='https://uploads.github.com/repos/myucxy/terminal-core/releases/'+$release.id+'/assets?name='+[Uri]::EscapeDataString($name)
            try { Invoke-RestMethod -Method Post -Uri $upload -Headers $headers -ContentType 'application/octet-stream' -InFile $path -TimeoutSec 600 | Out-Null }
            catch { throw "GitHub asset upload failed: $name. Existing draft/assets are retained for recovery." }
        }
    }
    if (-not $release) { throw 'Release not found.' }
    if (-not $DownloadDirectory) { $DownloadDirectory=Join-Path ([IO.Path]::GetTempPath()) ('core-download-'+[Guid]::NewGuid().ToString('N')) }
    if (Test-Path -LiteralPath $DownloadDirectory) { throw 'Use a new download directory.' }
    $null=New-Item -ItemType Directory -Path $DownloadDirectory
    $assets=@(Invoke-ReleaseApi 'GET' ($api+'/releases/'+$release.id+'/assets?per_page=100'))
    if ($assets.Count -ne $assetNames.Count) { throw 'Release asset count is incomplete or unexpected.' }
    foreach ($name in $assetNames) {
        $asset=@($assets|Where-Object name -CEQ $name)
        if ($asset.Count -ne 1 -or $asset[0].state -cne 'uploaded') { throw "Missing or invalid asset: $name" }
        Download-ReleaseAsset $asset[0] $DownloadDirectory
    }
    if (-not $CosignPath -or -not $ExpectedPublicKeySha256) { throw 'Downloaded packages need an independently pinned signing key and Cosign.' }
    $remote=& (Join-Path $PSScriptRoot 'Verify-CoreReleasePackage.ps1') -ReleaseDirectory $DownloadDirectory -CosignPath $CosignPath -ExpectedPublicKeySha256 $ExpectedPublicKeySha256
    if ($remote.version -cne $Version) { throw 'Downloaded release version mismatch.' }
    if ($Action -in @('Upload','Publish') -and $remote.revision -cne $verified.revision) { throw 'Remote/local source revision mismatch.' }
    if ($Action -eq 'Publish') {
        if (-not $release.draft) { throw 'Release is already published; no mutation performed.' }
        $release=Invoke-ReleaseApi 'PATCH' ($api+'/releases/'+$release.id) @{draft=$false;make_latest='false'}
        if ($release.draft) { throw 'Release remained a draft.' }
    }
    [pscustomobject]@{url=$release.html_url;draft=$release.draft;prerelease=$release.prerelease;private=$repo.private;
        version=$remote.version;revision=$remote.revision;verified_assets=$assetNames.Count;download_directory=$DownloadDirectory}
} finally {
    $token=$null
    if ($headers.ContainsKey('Authorization')) { $headers.Remove('Authorization') }
}
