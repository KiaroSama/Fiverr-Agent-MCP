param([Parameter(Mandatory)][string]$Owner,[Parameter(Mandatory)][string]$Root)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
Set-Location -LiteralPath $Root
$pattern='(validate_id|validate_relative).*with Ok'
$inventory=& $Owner run --wall 60 --idle 30 -- cargo mutants --list --json --re $pattern
if ($LASTEXITCODE -ne 0) { throw 'Mutation inventory failed.' }
$mutants=@(($inventory|Out-String|ConvertFrom-Json))
if ($mutants.Count -lt 1 -or $mutants.Count -gt 6) { throw 'Mutation scope must discover 1-6 selected validator mutants.' }
# Native nextest already passed on this exact checkout; do not rerun an unmutated baseline.
& $Owner run --wall 1500 --idle 240 -- cargo mutants --in-place --baseline skip --re $pattern --timeout 30 --build-timeout 600 --test-tool nextest --output (Join-Path $Root '.ci-work/mutants') --cargo-arg=--locked --cargo-test-arg=--profile=ci --cargo-test-arg=--test-threads=2 --cargo-test-arg=--test=primitives --cargo-test-arg=--test=properties
if ($LASTEXITCODE -ne 0) { throw 'Selected validator mutations survived, timed out or failed.' }
$result=Get-Content -LiteralPath (Join-Path $Root '.ci-work/mutants/mutants.out/outcomes.json') -Raw -Encoding utf8 | ConvertFrom-Json
if ($result.total_mutants -ne $mutants.Count -or $result.caught -ne $mutants.Count -or $result.missed -ne 0 -or $result.timeout -ne 0 -or $result.unviable -ne 0) { throw 'Mutation evidence is incomplete or inconclusive.' }
& git diff --exit-code -- src
if ($LASTEXITCODE -ne 0) { throw 'Mutation source restoration failed; packaging refused.' }
Write-Output "Selected validator mutants caught: $($result.caught)/$($result.total_mutants). This is bounded assertion evidence, not a full mutation campaign."
