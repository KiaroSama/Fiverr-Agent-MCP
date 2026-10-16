param([Parameter(Mandatory)][string]$Binary,[Parameter(Mandatory)][string]$Sha)
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
$root=Split-Path $PSScriptRoot -Parent
. "$PSScriptRoot/windows-signing.ps1"
if(-not $IsWindows){throw 'Windows native signing requires Windows.'}
$policy=Join-Path $root 'config/signing-public.json'
if(-not $env:FIVERR_WINDOWS_PFX_BASE64 -or -not $env:FIVERR_WINDOWS_PFX_PASSWORD){throw 'Protected signing inputs are missing.'}
$directory=Join-Path $root ('.ci-work/signing-'+[guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($directory)
$user=[Security.Principal.WindowsIdentity]::GetCurrent().User
$acl=[Security.AccessControl.DirectorySecurity]::new();$acl.SetAccessRuleProtection($true,$false);$acl.SetOwner($user);$acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($user,'FullControl','ContainerInherit,ObjectInherit','None','Allow'));Set-Acl -LiteralPath $directory -AclObject $acl
$certificate=$null
try {
    $pfx=[Convert]::FromBase64String($env:FIVERR_WINDOWS_PFX_BASE64)
    $certificate=[Security.Cryptography.X509Certificates.X509CertificateLoader]::LoadPkcs12($pfx,$env:FIVERR_WINDOWS_PFX_PASSWORD,[Security.Cryptography.X509Certificates.X509KeyStorageFlags]::UserKeySet)
    $pin=([IO.File]::ReadAllText($policy,[Text.Encoding]::UTF8)|ConvertFrom-Json).certificate_sha256
    if(-not $certificate.HasPrivateKey -or $certificate.GetCertHashString([Security.Cryptography.HashAlgorithmName]::SHA256).ToLowerInvariant() -cne $pin){throw 'Private signing identity does not match the pinned public certificate.'}
    $receipt=Set-OwnerSignature -File $Binary -Policy $policy -Certificate $certificate
    $tampered=Join-Path $directory 'tampered.exe';[IO.File]::Copy($Binary,$tampered)
    $bytes=[IO.File]::ReadAllBytes($tampered);$bytes[512]=$bytes[512] -bxor 1;[IO.File]::WriteAllBytes($tampered,$bytes)
    $rejected=$false;try{[void](Assert-OwnerSignature -File $tampered -Policy $policy)}catch{$rejected=$true}
    if(-not $rejected){throw 'Tampered executable was not rejected.'}
    $bytes=[IO.File]::ReadAllBytes($Binary);$pe=[BitConverter]::ToInt32($bytes,60);$optional=$pe+24;$security=$optional+144;$certificateOffset=[BitConverter]::ToUInt32($bytes,$security);$bytes[$certificateOffset+4]=0;$bytes[$certificateOffset+5]=0;[IO.File]::WriteAllBytes($tampered,$bytes)
    $rejected=$false;try{[void](Assert-OwnerSignature -File $tampered -Policy $policy)}catch{$rejected=$true}
    if(-not $rejected){throw 'Malformed certificate record was not rejected.'}
    $wrong=Join-Path $directory 'wrong-pin.json';$wrongPolicy=[IO.File]::ReadAllText($policy,[Text.Encoding]::UTF8)|ConvertFrom-Json;$wrongPolicy.certificate_sha256='0'*64;[IO.File]::WriteAllText($wrong,($wrongPolicy|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
    $rejected=$false;try{[void](Assert-OwnerSignature -File $Binary -Policy $wrong)}catch{$rejected=$true}
    if(-not $rejected){throw 'Wrong signer pin was not rejected.'}
    $receipt|Add-Member -NotePropertyName source_sha -NotePropertyValue $Sha
    $receipt|Add-Member -NotePropertyName negative_tests -NotePropertyValue @('tampered_pe_rejected','wrong_signer_rejected')
    [IO.File]::WriteAllText((Join-Path $root '.ci-work/windows-signing-receipt.json'),($receipt|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
    Write-Output 'Windows owner signature verified; tamper and wrong-signer negatives passed. Certificate is self-signed/private, not publicly trusted.'
} catch {
    throw 'Windows signing or verification failed; private signing inputs omitted.'
} finally {
    if($certificate){$certificate.Dispose()}
    $env:FIVERR_WINDOWS_PFX_BASE64=$null;$env:FIVERR_WINDOWS_PFX_PASSWORD=$null
    if(Test-Path -LiteralPath $directory){Remove-Item -LiteralPath $directory -Recurse -Force}
}
