# Verification report

Checked 2026-10-16. Evidence is revision-specific, not a certificate of complete Fiverr access. Older receipts below are historical and do not describe the current installed version unless explicitly stated.

## Current signed Windows delivery

Source `db42d38ed7de29b6b67db45945967bbf3341d4fc` passed [manual CI run38841816625](https://github.com/KiaroSama/Fiverr-Agent-MCP/actions/runs/38841816625): Windows167/167, Linux172/172 and both macOS169/169, zero skipped; all four native jobs and dependency checks succeeded. Linux line coverage was 93.09% application / 95.03% safety, including session modules. Both selected validator mutants were caught. These are source-line and bounded fixture results, not full product acceptance.

The Windows job built, signed and packaged the executable, verified its pinned owner certificate and PE content digest, rejected tampered content, malformed certificate revision and wrong-signer fixtures, and passed extracted minimum-PATH package smoke. The missing `selectors 0.38.0` license text was supplied from the official MPL-2.0 text with an exact package/version/hash check before this accepted run.

The exact signed Windows executable was installed after verifying all471 payload hashes and187 corresponding-source members. Executable SHA256: `7571605efcc2076b2bcf22b4e1f5ca7e1c1a82ab7ab8285c96c0fa0e4c36320b`. Existing client configuration, installation marker and private application state were preserved. Signature integrity and the public certificate pin were independently verified before and after replacement; no local application, Rust build or test was run.

The `KiaroSama` certificate is self-signed/private trust, not a public CA identity or SmartScreen reputation guarantee. Native Windows verification reported an untrusted root; independent CMS signature, PE digest and expected-certificate checks passed. No system trust-store change was made. Other platforms passed native source validation only, not final signed distribution acceptance. No GitHub Release was published. The delivery branch was merged into `feature/fiverr-native` and deleted.

Documentation-only corrections after this revision carry `[skip ci]`; the executable and test evidence remain tied to `db42d38`, not a newer unexecuted documentation SHA. Live Fiverr account/markup, loaded-extension registration, automation permission and actual client loading remain unverified.

## Historical session adapters: source validation, delivery blocked

Source `157860fda6b8c2bda75be578139e381bdd5eb6bc` passed [manual CI run38825724789](https://github.com/KiaroSama/Fiverr-Agent-MCP/actions/runs/38825724789): Windows167/167, Linux172/172 and both macOS169/169, zero skipped. Linux line coverage measured5161/5544 application (93.09163059163059%) and2697/2838 safety (95.03171247357294%); session modules are included. Both selected validator mutants were caught. Locked dependency advisory/license/source checks passed.

Actual CLI import/verify refusal/forget, six session menu actions, relocated native-host framing/manifest and exact extension extractor/response validation ran with synthetic fixtures. Live Fiverr account/markup, loaded-extension registration and automation permission remain unverified. No local Rust build/application/test ran.

This run deliberately used source/test validation only: final release compilation, distribution packaging, signing, publication and installation did not run because approved signer/key/trust inputs are absent. The installed executable remains the previously accepted `d449754`, SHA256 `ac9979af11707a47d08b9cb729dcf75bbd0a30dc4c666355731ca65e52f92610`, with configuration/private state unchanged. `feature/account-sessions` remains for unfinished signed delivery. Green validation is not a completed signed build.

Subsequent documentation-only corrections carry `[skip ci]`; test evidence belongs to source SHA `157860f`, not an unexecuted documentation SHA.

## Verified native baseline

Source `4a573bdcb7a51541e4ce979fe8ef5798842a1b25` completed [CI run38253015746](https://github.com/KiaroSama/Fiverr-Agent-MCP/actions/runs/38253015746): Windows x64 37/37, Linux x64 38/38, macOS ARM64 38/38 and macOS Intel 38/38 tests passed, zero skipped. Formatting, Clippy, manifest/capability gates, native browser fixtures, release builds, dependency audit/license checks and extracted minimum-PATH package smoke passed. Doctest commands discovered zero cases and add no coverage.

Downloaded archive checksums and native manifest payload hashes matched. Every source archive member matched exact committed bytes; common source/plugin archives were identical across targets. No Rust compilation, test or application execution occurred locally. The initial failed CI run is historical and does not describe the verified baseline.

## Current continuation

Source `e2ec4fe17676c0bbc744d2c40b1bd30cef54ed7f` completed [CI run38341811976](https://github.com/KiaroSama/Fiverr-Agent-MCP/actions/runs/38341811976): Windows43/43, Linux/macOS44/44 each, zero skips, all native packages and dependency checks passed. Generated client registration/examples use `Fiverr-Agent-mcp`. Backup validation/empty-workspace restore and unknown/executing journal no-replay regressions executed. Restore never replaces existing records/history/actions, re-enables routes, imports HTTP tokens or revives approvals.

Source `56bfb82ccb9a6b895107c185794ad5c4a4f89d4a` completed [CI run38356755085](https://github.com/KiaroSama/Fiverr-Agent-MCP/actions/runs/38356755085): all four native packages and dependency checks passed, Windows46/46 and Unix47/47 each, zero skips. Retention/capacity regressions and fresh Windows owner-only DACL checks passed. Both selected validator mutations were caught; missed, timed-out and unviable counts were zero. This is a two-mutation check, not a comprehensive campaign. Linux coverage was 62.37% application / 72.09% safety. This revision was the installed baseline at the time; the current installed build is recorded above.

## Coverage and bounded repair acceptance

Source `4f126a9f5dab869af0fe664997b633e7eaa6c050` completed [CI run38471841266](https://github.com/KiaroSama/Fiverr-Agent-MCP/actions/runs/38471841266): all four native builds/packages and dependency checks passed. Windows131/131 (80.836 seconds), Linux138/138 (14.780 seconds), macOS ARM137/137 (8.515 seconds) and macOS Intel137/137 (21.820 seconds), zero skipped. Full LLVM evidence measured4084/4406 application lines (92.691784%) and2090/2200 safety lines (95.0%). Both required targets are met with the same source scope and no threshold weakening. Two selected validator mutants were caught; missed, timed-out and unviable counts were zero.

The matching Windows executable was installed after archive checksums, all412 payload hashes and154 corresponding-source members were verified. Executable SHA256: `269dedcba6c06af5b2ff333e406e6846245368647875aaf651b42d8fbfb616ff`. Existing client configuration and installation marker were preserved. No application was executed locally. The owner subsequently authorized merging the [pull request](https://github.com/KiaroSama/Fiverr-Agent-MCP/pull/1) and its side branch after the deeper-test native CI batch passes; auto-merge is not enabled. That new batch's acceptance is separate from the tested revision above.

The nine export/snapshot/reminder regressions and fixed native CLI reproduction passed in that run. Export suffixes must match the selected format, Markdown exports keep JSON literal inside a sufficiently long fence, snapshot records refuse overwrite centrally, and reminders filter eligible tasks before applying their page limit. No source exclusions or lower acceptance thresholds were introduced. Linux measurements do not cover Windows-only code; native tests and DACL evidence are separate.

## Required outstanding acceptance

Real permitted manual Fiverr account sign-in/current typed production adapter and strict browser routing; remote executing/unknown-outcome postcondition reconciliation; complete platform privacy acceptance; broader coverage-guided fuzzing and mutation evidence beyond the bounded campaigns; actual client/plugin loading. Backup fixture coverage does not certify arbitrary databases, all filesystem races or platform privacy.

Capability implementation and verification are independent. Fixtures are not live account evidence, manual handoff is not remote success, and green native builds do not fulfill all account-backed workflows. No real messages, deliveries or financial mutations are dispatched merely for tests. GitHub release publication remains separate from CI artifact availability.
