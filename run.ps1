#requires -Version 7.0
param([Parameter(ValueFromRemainingArguments=$true)][string[]]$Arguments)
$ErrorActionPreference='Stop'
$root=$PSScriptRoot
$log=$null
try {
    $logs=Join-Path $root 'logs'
    [IO.Directory]::CreateDirectory($logs) | Out-Null
    $stamp=[DateTime]::UtcNow.ToString('yyyy-MM-dd_HH-mm-ss')
    $log=Join-Path $logs ('run_'+$stamp+'_UTC_'+[guid]::NewGuid().ToString('N')+'.log')
    function Write-Event([string]$Level,[string]$Message) {
        $entry=@{timestamp=[DateTime]::UtcNow.ToString('o');level=$Level;category='LAUNCHER';message=$Message}|ConvertTo-Json -Compress
        [IO.File]::AppendAllText($log,$entry+[Environment]::NewLine,[Text.UTF8Encoding]::new($false))
    }
    Write-Event 'INFO' 'Native launcher started'
    $binary=Join-Path $root 'bin/fiverr-agent-mcp.exe'
    if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) { throw 'Native executable missing. Download the verified Windows package; this launcher does not compile or install dependencies.' }
    $temp=Join-Path $root '.cache/tmp'
    [IO.Directory]::CreateDirectory($temp)|Out-Null
    $env:TEMP=$temp; $env:TMP=$temp; $env:TMPDIR=$temp; $env:XDG_CACHE_HOME=Join-Path $root '.cache'
    Write-Event 'DEBUG' 'Rooted child temporary paths configured'
    & $binary @Arguments
    $exit=$LASTEXITCODE
    Write-Event $(if($exit -eq 0){'INFO'}else{'ERROR'}) ('Native process exited with code '+$exit)
    exit $exit
} catch {
    if($log){Write-Event 'ERROR' 'Native launcher failed; check package/prerequisites and writable root'}
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}
