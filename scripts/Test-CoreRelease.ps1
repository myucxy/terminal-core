[CmdletBinding()]
param()
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CoreRelease.Common.ps1')
$root=Join-Path ([IO.Path]::GetTempPath()) ('core-release-tests-'+[Guid]::NewGuid().ToString('N'))
$null=New-Item -ItemType Directory -Path $root
$script:checks=0
function Expect-Rejected([scriptblock]$Action,[string]$ExpectedMessage) {
    $caught=$false
    try { & $Action | Out-Null } catch {
        if ($_.Exception.Message -notlike ('*'+$ExpectedMessage+'*')) { throw }
        $caught=$true
    }
    if (-not $caught) { throw "Expected rejection: $ExpectedMessage" }
    $script:checks++
}
foreach ($name in @('../escape','a/../escape','/absolute','C:/absolute','a\b','a//b','a/./b')) {
    Expect-Rejected { Assert-CoreFilePath $root $name } 'Unsafe relative path'
}
$native=Join-Path $root 'native-good'
$null=New-Item -ItemType Directory -Path $native
$records=@(foreach ($path in @(Get-CoreArtifactPaths)) {
    $file=Assert-CoreFilePath $native $path
    $null=New-Item -ItemType Directory -Path (Split-Path -Parent $file) -Force
    [IO.File]::WriteAllBytes($file,[Text.Encoding]::ASCII.GetBytes('fixture '+$path))
    @{platform=$(if($path.Contains('/android/')){'android'}else{'windows'});path=$path;size=(Get-Item $file).Length;sha256=(Get-CoreSha256 $file)}
})
$lock=@{schema_version=2;revision=('a'*40);source_dirty=$false;toolchain='1.97.1';repository='git@github.com:myucxy/terminal-core.git';artifacts=$records}
$lockPath=Join-Path $native 'terminal-core.lock.json'
function Save-Lock { $lock|ConvertTo-Json -Depth 6|Set-Content -LiteralPath $lockPath -Encoding utf8 }
Save-Lock
$null=Assert-CoreNativeLayout $native -RequireClean
$checks++
$lock.source_dirty=$true;Save-Lock
Expect-Rejected { Assert-CoreNativeLayout $native -RequireClean } 'Dirty native libraries'
$lock.source_dirty=$false
$lock.artifacts=@($records)+@($records[0]);Save-Lock
Expect-Rejected { Assert-CoreNativeLayout $native } 'Exactly five'
$lock.artifacts=$records
$lock.artifacts[0].sha256='0'*64;Save-Lock
Expect-Rejected { Assert-CoreNativeLayout $native } 'hash/size mismatch'
$lock.artifacts[0].sha256=Get-CoreSha256 (Join-Path $native $records[0].path)
Save-Lock
$file=Join-Path $native $records[1].path
[IO.File]::AppendAllText($file,'tamper')
Expect-Rejected { Assert-CoreNativeLayout $native } 'hash/size mismatch'
Add-Type -AssemblyName System.IO.Compression
function Fixture-Zip([string]$Name,[string[]]$Entries) {
    $path=Join-Path $root $Name
    $zip=[IO.Compression.ZipFile]::Open($path,[IO.Compression.ZipArchiveMode]::Create)
    try { foreach($name in $Entries) { $e=$zip.CreateEntry($name);$s=$e.Open();try{$s.WriteByte(65)}finally{$s.Dispose()} } }
    finally{$zip.Dispose()}
    $path
}
$bad=Fixture-Zip 'traversal.zip' @('../escape.txt')
Expect-Rejected { Expand-CoreZip $bad (Join-Path $root 'traversal') } 'Unsafe relative path'
if(Test-Path (Join-Path $root 'escape.txt')){throw 'ZIP traversal wrote outside its root.'}
$bad=Fixture-Zip 'duplicate.zip' @('File.txt','file.txt')
Expect-Rejected { Expand-CoreZip $bad (Join-Path $root 'duplicate') } 'Duplicate ZIP path'
$bad=Fixture-Zip 'oversize.zip' @('a','b')
Expect-Rejected { Expand-CoreZip $bad (Join-Path $root 'oversize') 1 } 'ZIP size limit'
$zipPath=Join-Path $root 'symlink.zip'
$zip=[IO.Compression.ZipFile]::Open($zipPath,[IO.Compression.ZipArchiveMode]::Create)
try{$entry=$zip.CreateEntry('link');$entry.ExternalAttributes=[int](-1577058304)}finally{$zip.Dispose()}
Expect-Rejected { Expand-CoreZip $zipPath (Join-Path $root 'symlink') } 'ZIP links and special files'
$good=Fixture-Zip 'good.zip' @('inside.txt')
$expanded=Join-Path $root 'expanded'
Expand-CoreZip $good $expanded
if([IO.File]::ReadAllText((Join-Path $expanded 'inside.txt')) -cne 'A'){throw 'Valid ZIP changed content.'}
$checks++
Expect-Rejected { Expand-CoreZip $good $expanded } 'must not already exist'
$output=Join-Path $root 'created.zip'
New-CoreZip $expanded $output
Expect-Rejected { New-CoreZip $expanded $output } 'Refusing to replace'
[pscustomobject]@{checks=$checks;status='passed';fixture_directory=$root}
