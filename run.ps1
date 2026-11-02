param([switch]$Status, [switch]$Preview,
    [Parameter(ValueFromRemainingArguments=$true)][string[]]$Arguments)
$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
if ($PSVersionTable.PSVersion.Major -lt 7) {
    $pwsh = Get-Command pwsh -ErrorAction SilentlyContinue
    if (-not $pwsh) { [Console]::Error.WriteLine('PowerShell 7 is required. Install it explicitly, then run this launcher again.'); exit 1 }
    $forward = @()
    if ($Status) { $forward += '-Status' }
    if ($Preview) { $forward += '-Preview' }
    & $pwsh.Source -NoLogo -NoProfile -File $PSCommandPath @forward @Arguments
    exit $LASTEXITCODE
}
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
[Console]::InputEncoding = [Text.UTF8Encoding]::new($false)
$log = $null
function Write-Event([string]$Level, [string]$Message) {
    if (-not $script:log) { return }
    try {
        $entry = @{timestamp=[DateTime]::UtcNow.ToString('o');level=$Level;category='LAUNCHER';message=$Message} | ConvertTo-Json -Compress
        [IO.File]::AppendAllText($script:log, $entry+[Environment]::NewLine, [Text.UTF8Encoding]::new($false))
    } catch { [Console]::Error.WriteLine('Launcher file logging failed; continuing with console diagnostics.'); $script:log = $null }
}
try {
    try {
        $logs = Join-Path $root 'logs'
        [IO.Directory]::CreateDirectory($logs) | Out-Null
        $stamp = [DateTime]::UtcNow.ToString('yyyy-MM-dd_HH-mm-ss')
        $log = Join-Path $logs ('run_'+$stamp+'_UTC_'+[guid]::NewGuid().ToString('N')+'.log')
        Write-Event 'INFO' 'Native launcher started'
    } catch { [Console]::Error.WriteLine('Launcher file logging unavailable; native startup will validate its own logging.'); $log = $null }
    if ($Status -and $Preview) { throw 'Choose only one launcher mode: Status or Preview.' }
    if (($Status -or $Preview) -and $Arguments.Count -gt 0) { throw 'Launcher mode switches cannot be combined with native command arguments.' }
    $binary = Join-Path $root 'bin/fiverr-agent-mcp.exe'
    if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) { throw 'Native executable missing. Download the verified Windows package; no compilation or automatic dependency installation occurs.' }
    $temp = Join-Path $root '.cache/tmp'
    [IO.Directory]::CreateDirectory($temp) | Out-Null
    $env:TEMP = $temp; $env:TMP = $temp; $env:TMPDIR = $temp
    $env:XDG_CACHE_HOME = Join-Path $root '.cache'
    Write-Event 'DEBUG' 'Rooted child temporary paths configured'
    [string[]]$native = if ($Status) { @('status') } elseif ($Preview) { @('preview') } elseif ($Arguments.Count -gt 0) { $Arguments } else { @('menu') }
    & $binary --root $root @native
    $code = $LASTEXITCODE
    Write-Event $(if ($code -eq 0) { 'INFO' } else { 'ERROR' }) ('Native process exited with code '+$code)
    exit $code
} catch {
    Write-Event 'ERROR' 'Native launcher failed; private argument values omitted'
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}
