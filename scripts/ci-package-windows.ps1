param([Parameter(Mandatory)][string]$Sha)
$ErrorActionPreference='Stop'
$root=Split-Path $PSScriptRoot -Parent
$binary=Join-Path $env:CARGO_TARGET_DIR 'release/fiverr-agent-mcp.exe'
$xtask=Join-Path $env:CARGO_TARGET_DIR 'debug/xtask.exe'
& "$PSScriptRoot/ci-sign-windows.ps1" -Binary $binary -Sha $Sha
$env:FIVERR_WINDOWS_PFX_BASE64=$null;$env:FIVERR_WINDOWS_PFX_PASSWORD=$null
& $xtask run --wall 300 --idle 90 -- $xtask package --target x86_64-pc-windows-msvc --sha $Sha --binary $binary --out (Join-Path $root 'dist')
if($LASTEXITCODE -ne 0){throw 'Signed Windows packaging failed.'}
. "$PSScriptRoot/windows-signing.ps1"
$archive=Join-Path $root 'dist/fiverr-agent-mcp-0.1.0-x86_64-pc-windows-msvc.zip'
$zip=[IO.Compression.ZipFile]::OpenRead($archive)
$check=Join-Path $root '.ci-work/signed-package.exe'
try {
    $entry=$zip.GetEntry('bin/fiverr-agent-mcp.exe')
    if(-not $entry){throw 'Signed package executable missing.'}
    [IO.Compression.ZipFileExtensions]::ExtractToFile($entry,$check,$false)
    [void](Assert-OwnerSignature -File $check -Policy (Join-Path $root 'config/signing-public.json'))
    if((Get-FileHash $check).Hash -cne (Get-FileHash $binary).Hash){throw 'Packaged signed executable changed.'}
} finally {$zip.Dispose();if(Test-Path $check){Remove-Item -LiteralPath $check -Force}}
Copy-Item -LiteralPath (Join-Path $root '.ci-work/windows-signing-receipt.json') -Destination (Join-Path $root 'dist/windows-signing-receipt.json')
