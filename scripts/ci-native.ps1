param([Parameter(Mandatory)][string]$Target,[Parameter(Mandatory)][string]$Sha)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$root=Split-Path $PSScriptRoot -Parent
Set-Location -LiteralPath $root
$version = (& rustc -vV | Out-String)
if ($LASTEXITCODE -ne 0 -or $version -notmatch ('host: '+[Regex]::Escape($Target))) { throw 'Actual native Rust target mismatch.' }
if (-not (Test-Path -LiteralPath 'Cargo.lock')) { throw 'Committed Cargo.lock is required.' }
function Bootstrap-Cargo([string[]]$Arguments) {
    & $PSScriptRoot/ci-owned.ps1 -Executable cargo -ArgumentsJson (ConvertTo-Json -InputObject $Arguments -Compress) -Directory $root -Wall 900 -Idle 180
    if ($LASTEXITCODE -ne 0) { throw 'CI bootstrap Cargo command failed.' }
}
Bootstrap-Cargo @('fetch','--locked')
Bootstrap-Cargo @('build','--locked','--package','xtask','--jobs','2')
$xtask=Join-Path $env:CARGO_TARGET_DIR ('debug/xtask'+$(if($IsWindows){'.exe'}else{''}))
function Owned([int]$Wall,[int]$Idle,[string]$Executable,[string[]]$Arguments) {
    & $xtask run --wall $Wall --idle $Idle -- $Executable @Arguments
    if ($LASTEXITCODE -ne 0) { throw 'Bounded CI command failed.' }
}
Owned 60 30 cargo @('fmt','--all','--','--check')
Owned 1200 240 cargo @('clippy','--locked','--workspace','--all-targets','--all-features','--jobs','2','--','-D','warnings')
& $xtask validate-manifests
if ($LASTEXITCODE -ne 0) { throw 'Manifest gate failed.' }
& $xtask verify-capabilities
if ($LASTEXITCODE -ne 0) { throw 'Capability gate failed.' }
$browserCandidates=if($IsWindows){@('C:/Program Files/Google/Chrome/Application/chrome.exe','C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe')}elseif($IsMacOS){@('/Applications/Google Chrome.app/Contents/MacOS/Google Chrome')}else{@('/usr/bin/google-chrome','/usr/bin/chromium','/usr/bin/chromium-browser')}
$env:FIVERR_FIXTURE_BROWSER=@($browserCandidates|Where-Object{Test-Path -LiteralPath $_ -PathType Leaf}|Select-Object -First 1)[0]
if(-not $env:FIVERR_FIXTURE_BROWSER){throw 'Native browser prerequisite missing; required fixture test cannot be skipped.'}
if ($IsLinux) {
    # Rooted HOME/XDG removes the runner's warmed font cache; prepare it before the bounded browser fixture.
    Owned 120 60 fc-cache @('--verbose')
}
if ($IsLinux) {
    Owned 1500 300 cargo @('llvm-cov','nextest','--locked','--workspace','--all-features','--json','--summary-only','--output-path',(Join-Path $root '.ci-work/coverage.json'),'--profile','ci','--test-threads','2','--no-tests','fail','--no-fail-fast')
    & $xtask coverage-report --input (Join-Path $root '.ci-work/coverage.json') --output (Join-Path $root '.ci-work/coverage-summary.json')
    if ($LASTEXITCODE -ne 0) { throw 'Coverage evidence parsing failed.' }
    & $PSScriptRoot/ci-mutation.ps1 -Owner $xtask -Root $root
    if ($LASTEXITCODE -ne 0) { throw 'Bounded mutation campaign failed.' }
} else {
    Owned 1200 240 cargo @('nextest','run','--locked','--workspace','--all-features','--profile','ci','--test-threads','2','--no-tests','fail','--no-fail-fast')
}
Owned 300 90 cargo @('test','--locked','--workspace','--doc','--all-features','--jobs','2')
Owned 1800 300 cargo @('build','--locked','--release','--package','fiverr-agent-mcp','--jobs','2')
$binary=Join-Path $env:CARGO_TARGET_DIR ('release/fiverr-agent-mcp'+$(if($IsWindows){'.exe'}else{''}))
if ($IsWindows) {
    $privacyRoot=Join-Path $root '.ci-work/privacy acceptance'
    New-Item -ItemType Directory -Path $privacyRoot -Force | Out-Null
    Owned 30 15 $binary @('--root',$privacyRoot,'status')
    $sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    foreach ($relative in @('.state','.state/workspace','.state/workspace/workspace.sqlite3','logs')) {
        $acl=Get-Acl -LiteralPath (Join-Path $privacyRoot $relative)
        if (-not $acl.AreAccessRulesProtected) { throw 'Private ACL inheritance is not protected.' }
        $rules=@($acl.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier]))
        if ($rules.Count -eq 0 -or @($rules|Where-Object {$_.IdentityReference.Value -ne $sid -or $_.AccessControlType -ne 'Allow'}).Count -ne 0) { throw 'Private ACL grants another identity or lacks owner access.' }
    }
    Remove-Item -LiteralPath $privacyRoot -Recurse -Force
}
Owned 300 90 $xtask @('package','--target',$Target,'--sha',$Sha,'--binary',$binary,'--out',(Join-Path $root 'dist'))
