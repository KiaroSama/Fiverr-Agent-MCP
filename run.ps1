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
function Assert-UnlinkedPath([string]$Path) {
    $current = [IO.Path]::GetFullPath($Path)
    while ($current) {
        try {
            $attributes = [IO.File]::GetAttributes($current)
            if (($attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw 'Reparse-point installation paths are refused before native startup.'
            }
        } catch [IO.FileNotFoundException] {
            # Missing descendants are created only by the native validated root.
        } catch [IO.DirectoryNotFoundException] {
        }
        $parent = [IO.Path]::GetDirectoryName($current)
        if ($parent -eq $current) { break }
        $current = $parent
    }
}
try {
    if ($Status -and $Preview) { throw 'Choose only one launcher mode: Status or Preview.' }
    if (($Status -or $Preview) -and $Arguments.Count -gt 0) { throw 'Launcher mode switches cannot be combined with native command arguments.' }
    # Validate every native-owned child before starting code that creates directories.
    # This is a preflight, not protection against arbitrary concurrent path replacement.
    Assert-UnlinkedPath $root
    foreach ($relative in @('.cache/tmp', '.cache/http', '.cache/browser', '.state/workspace', '.state/audit', '.state/browser', 'config', 'logs', 'exports', 'bin/fiverr-agent-mcp.exe')) {
        Assert-UnlinkedPath (Join-Path $root $relative)
    }
    $binary = Join-Path $root 'bin/fiverr-agent-mcp.exe'
    if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) { throw 'Native executable missing. Download the verified Windows package; no compilation or automatic dependency installation occurs.' }
    $tempMode = $env:FIVERR_AGENT_TEMP_MODE
    if ($null -ne $tempMode -and $tempMode -cnotin @('rooted', 'system')) { throw 'FIVERR_AGENT_TEMP_MODE must be rooted or system.' }
    # Native Root/bootstrap/logger own private first writes and temporary-environment setup.
    [string[]]$native = if ($Status) { @('status') } elseif ($Preview) { @('preview') } elseif ($Arguments.Count -gt 0) { $Arguments } else { @('menu') }
    & $binary --root $root @native
    exit $LASTEXITCODE
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}
