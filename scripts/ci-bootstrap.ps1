$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$root = Split-Path $PSScriptRoot -Parent
$paths = @{
    CARGO_HOME='.ci-cache/cargo'; RUSTUP_HOME='.ci-cache/rustup'; CARGO_TARGET_DIR='target'
    TEMP='.ci-work/tmp'; TMP='.ci-work/tmp'; TMPDIR='.ci-work/tmp'; XDG_CACHE_HOME='.ci-cache/xdg'; HOME='.ci-work/home'
}
foreach ($name in $paths.Keys) {
    $path = Join-Path $root $paths[$name]
    [void][IO.Directory]::CreateDirectory($path)
    if ((Get-Item -LiteralPath $path).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'CI storage must not be a link.' }
    [Environment]::SetEnvironmentVariable($name,$path,'Process')
    if ($env:GITHUB_ENV) { [IO.File]::AppendAllText($env:GITHUB_ENV,"$name=$path`n",[Text.UTF8Encoding]::new($false)) }
}
$pwsh = (Get-Command pwsh -CommandType Application -ErrorAction Stop).Source
$env:CI_PWSH=$pwsh
if ($env:GITHUB_ENV) { [IO.File]::AppendAllText($env:GITHUB_ENV,"CI_PWSH=$pwsh`n",[Text.UTF8Encoding]::new($false)) }
$env:CARGO_BUILD_JOBS='2'
$env:HOOKMAKER_MAX_TEST_WORKERS='2'
if ($env:GITHUB_ENV) { [IO.File]::AppendAllText($env:GITHUB_ENV,"CARGO_BUILD_JOBS=2`nHOOKMAKER_MAX_TEST_WORKERS=2`n",[Text.UTF8Encoding]::new($false)) }
