Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Security.Cryptography.Pkcs

function Assert-OwnerSignature {
    param([Parameter(Mandatory)][string]$File,[Parameter(Mandatory)][string]$Policy)
    $policyData=[IO.File]::ReadAllText($Policy,[Text.Encoding]::UTF8)|ConvertFrom-Json
    if($policyData.schema_version -ne 1 -or $policyData.trust -ne 'self_signed_private'){throw 'Unsupported signing policy.'}
    $bytes=[IO.File]::ReadAllBytes($File)
    if($bytes.Length -lt 512 -or [BitConverter]::ToUInt16($bytes,0) -ne 0x5a4d){throw 'Expected a PE executable.'}
    $pe=[BitConverter]::ToInt32($bytes,60)
    if($pe -lt 64 -or $pe+256 -ge $bytes.Length -or [BitConverter]::ToUInt32($bytes,$pe) -ne 0x4550){throw 'Invalid PE header.'}
    $optional=$pe+24
    $magic=[BitConverter]::ToUInt16($bytes,$optional)
    $directory=if($magic -eq 0x20b){$optional+112}elseif($magic -eq 0x10b){$optional+96}else{throw 'Unsupported PE optional header.'}
    $security=$directory+32
    $checksum=$optional+64
    $headers=[BitConverter]::ToUInt32($bytes,$optional+60)
    $certOffset=[BitConverter]::ToUInt32($bytes,$security)
    $certSize=[BitConverter]::ToUInt32($bytes,$security+4)
    if($certOffset -lt $headers -or $certSize -lt 8 -or [long]$certOffset+$certSize -ne $bytes.Length){throw 'Missing or nonterminal PE certificate table.'}
    $length=[BitConverter]::ToUInt32($bytes,$certOffset)
    if($length -lt 8 -or $length -gt $certSize -or [BitConverter]::ToUInt16($bytes,$certOffset+4) -ne 0x200 -or [BitConverter]::ToUInt16($bytes,$certOffset+6) -ne 2){throw 'Invalid Authenticode certificate record.'}
    $cms=[Security.Cryptography.Pkcs.SignedCms]::new()
    $cms.Decode($bytes[($certOffset+8)..($certOffset+$length-1)])
    $cms.CheckSignature($true)
    if($cms.SignerInfos.Count -ne 1 -or $cms.ContentInfo.ContentType.Value -ne '1.3.6.1.4.1.311.2.1.4'){throw 'Expected one Authenticode signer.'}
    $certificate=$cms.SignerInfos[0].Certificate
    $pin=$certificate.GetCertHashString([Security.Cryptography.HashAlgorithmName]::SHA256).ToLowerInvariant()
    if($pin -cne $policyData.certificate_sha256 -or [Convert]::ToBase64String($certificate.RawData) -cne $policyData.certificate_der_base64){throw 'Signing certificate does not match the owner pin.'}
    if($certificate.NotBefore.ToUniversalTime() -gt [DateTime]::UtcNow -or $certificate.NotAfter.ToUniversalTime() -lt [DateTime]::UtcNow){throw 'Signing certificate is outside its validity period.'}
    if(-not ($certificate.Extensions | Where-Object {$_.Oid.Value -eq '2.5.29.37'} | ForEach-Object {$_.EnhancedKeyUsages} | Where-Object Value -EQ '1.3.6.1.5.5.7.3.3')){throw 'Certificate is not authorized for code signing.'}
    $reader=[System.Formats.Asn1.AsnReader]::new([ReadOnlyMemory[byte]]$cms.ContentInfo.Content,[System.Formats.Asn1.AsnEncodingRules]::DER)
    $sequence=$reader.ReadSequence();[void]$sequence.ReadEncodedValue()
    $digestInfo=$sequence.ReadSequence();$algorithm=$digestInfo.ReadSequence()
    if($algorithm.ReadObjectIdentifier() -ne '2.16.840.1.101.3.4.2.1'){throw 'Only SHA256 Authenticode is permitted.'}
    $expected=$digestInfo.ReadOctetString()
    $hash=[Security.Cryptography.IncrementalHash]::CreateHash([Security.Cryptography.HashAlgorithmName]::SHA256)
    try {
        if($headers -gt $certOffset -or $security+8 -gt $headers){throw 'Invalid PE header bounds.'}
        $hash.AppendData($bytes,0,$checksum)
        $hash.AppendData($bytes,$checksum+4,$security-$checksum-4)
        $hash.AppendData($bytes,$security+8,$headers-$security-8)
        $sections=[BitConverter]::ToUInt16($bytes,$pe+6)
        $table=$optional+[BitConverter]::ToUInt16($bytes,$pe+20)
        $ranges=@(for($i=0;$i -lt $sections;$i++){
            $entry=$table+40*$i
            if($entry+40 -gt $headers){throw 'Invalid PE section header.'}
            [pscustomobject]@{Offset=[BitConverter]::ToUInt32($bytes,$entry+20);Size=[BitConverter]::ToUInt32($bytes,$entry+16)}
        })
        $ranges=@($ranges | Sort-Object Offset)
        $end=[long]$headers
        foreach($range in $ranges){
            if($range.Size -eq 0){continue}
            if($range.Offset -ne $end -or [long]$range.Offset+$range.Size -gt $certOffset){throw 'Unsupported overlapping or gapped PE sections.'}
            $hash.AppendData($bytes,[int]$range.Offset,[int]$range.Size)
            $end=[long]$range.Offset+$range.Size
        }
        if($end -lt $certOffset){$hash.AppendData($bytes,[int]$end,[int]($certOffset-$end))}
        if(-not [Security.Cryptography.CryptographicOperations]::FixedTimeEquals($hash.GetHashAndReset(),$expected)){throw 'Signed PE digest mismatch; executable was modified.'}
    } finally {$hash.Dispose()}
    [pscustomobject]@{file_sha256=(Get-FileHash -LiteralPath $File -Algorithm SHA256).Hash.ToLowerInvariant();certificate_sha256=$pin;signature_valid=$true;trust='self_signed_private';publicly_trusted=$false}
}
function Set-OwnerSignature {
    param([Parameter(Mandatory)][string]$File,[Parameter(Mandatory)][string]$Policy,[Parameter(Mandatory)][Security.Cryptography.X509Certificates.X509Certificate2]$Certificate)
    $result=Set-AuthenticodeSignature -LiteralPath $File -Certificate $Certificate -HashAlgorithm SHA256 -IncludeChain Signer
    if($result.Status -notin @('Valid','NotTrusted','UnknownError')){throw 'Native Authenticode signing failed.'}
    Assert-OwnerSignature -File $File -Policy $Policy
}
