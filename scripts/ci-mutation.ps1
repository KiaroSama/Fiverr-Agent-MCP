param([Parameter(Mandatory)][string]$Owner,[Parameter(Mandatory)][string]$Root)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
Set-Location -LiteralPath $Root
$pattern='(validate_id|validate_relative).*with Ok'
# v27.1.0 struct-field discovery bypasses name filters; also restrict source files.
$scope=@('--package','fiverr-agent-mcp','--file','src/core/mod.rs','--file','src/storage/root.rs','--re',$pattern)
$inventory=& $Owner run --wall 60 --idle 30 -- cargo mutants --list --json @scope
if ($LASTEXITCODE -ne 0) { throw 'Mutation inventory failed.' }
$inventoryText=$inventory|Out-String
Set-Content -LiteralPath (Join-Path $Root '.ci-work/mutation-inventory.json') -Value $inventoryText -Encoding utf8
$mutants=@(($inventoryText|ConvertFrom-Json))
if ($mutants.Count -ne 2 -or @($mutants|Where-Object {$_.genre -ne 'FnValue' -or $_.replacement -ne 'Ok(())'}).Count -ne 0 -or @($mutants.file|Sort-Object -Unique).Count -ne 2) { throw "Mutation scope must discover exactly the two validator-return mutants; found $($mutants.Count)." }
$mutants|ForEach-Object {Write-Output "Selected mutant: $($_.name)"}
# Native nextest already passed on this exact checkout; do not rerun an unmutated baseline.
& $Owner run --wall 1500 --idle 240 -- cargo mutants --in-place --baseline skip @scope --timeout 30 --build-timeout 600 --test-tool nextest --output (Join-Path $Root '.ci-work/mutants') --cargo-arg=--locked --cargo-test-arg=--profile=ci --cargo-test-arg=--test-threads=2 --cargo-test-arg=--test=primitives --cargo-test-arg=--test=properties
if ($LASTEXITCODE -ne 0) { throw 'Selected validator mutations survived, timed out or failed.' }
$result=Get-Content -LiteralPath (Join-Path $Root '.ci-work/mutants/mutants.out/outcomes.json') -Raw -Encoding utf8 | ConvertFrom-Json
if ($result.total_mutants -ne $mutants.Count -or $result.caught -ne $mutants.Count -or $result.missed -ne 0 -or $result.timeout -ne 0 -or $result.unviable -ne 0) { throw 'Mutation evidence is incomplete or inconclusive.' }
& git diff --exit-code -- src
if ($LASTEXITCODE -ne 0) { throw 'Mutation source restoration failed; packaging refused.' }
Write-Output "Selected validator mutants caught: $($result.caught)/$($result.total_mutants). This is bounded assertion evidence, not a full mutation campaign."
