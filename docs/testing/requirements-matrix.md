# Requirement and acceptance matrix

Status below is authored versus outstanding, not executed evidence. Every named workflow has its own entry in capabilities.json. All test source runs in the native CI workspace bucket; tests that were only written are not green.

| Requirement | Code / artifact | Test seam | Outstanding acceptance |
|---|---|---|---|
| FR001,013,014 | server/, CLI HTTP configuration | protocol.rs, native package | Full legacy/modern client loading and progress/cancellation/extensions |
| FR002,015 | bootstrap/, storage/root, logging/database | primitives/workspace/package | Complete ACL/race/IO-failure/log retention proof |
| FR003,004 | network/profiles, RouteClient | network.rs, unit wire_tests | HTTPS/HTTP CONNECT/auth/redirect/TLS/DNS all-platform assurance |
| FR005,006 | browser/observation, profile lease, fail-closed adapter gates | browser.rs, browser_runtime.rs | Main-menu login acceptance cancelled; permitted adapter/current selectors/strict routing remain conditional |
| FR007,008 | capabilities.json, server/catalog, services | inventory validation, protocol | Dedicated unsupported platform workflows; catalog/local mappings are not live equivalents |
| FR009,010 | local/productivity/portability, exact Money | productivity/workspace/primitives/analysis | Full retention/restore/quotas and all domain-local validators |
| FR011,012,016 | actions/bindings, owner CLI | actions.rs/file primitives | Executing/unknown remote dispatch/reconciliation, all file/approval scope races |
| FR017 | licensed analysis/helper/skill/locks/notices | analysis.rs literal goldens | Native runtime and actual host skill loading |
| FR018,019 | plugin/client manifests, registration/CLI/launchers | manifest/package/CLI | Full launcher/menu/registration concurrent failure tests and actual client schemas/loading |
| FR020,021 | native CI/xtask/release/source/license | four-native validation, signed Windows package/security | Source `db42d38` met90/95 line targets and caught two selected mutants; full coverage-guided fuzzing/mutation, other-platform signed packages and GitHub Release publication remain separate |
| FR022,023 | public research/reference pins, local tracker routing | primary-source reads/static validation | Sources stay version-specific; local tracking not public issue generation |
| FR024,025 | session/, owner CLI/menu, extension/native host | session credentials/HTML/wire/storage/CLI/PTY/JS fixtures | Live Fiverr compatibility, loaded-extension registration and platform permission remain unverified |
| FR026 | public signing policy, Windows signing/package CI | pinned CMS/PE digest, tamper/revision/wrong-signer rejection, exact installed artifact | Windows private-trust delivery verified on `db42d38`; public CA trust and other-platform signing are not supplied |

## Current amendments and maintenance

| Requirement | Current disposition / seam | Acceptance limit |
|---|---|---|
| FR027–028 | No main-menu login; fresh Direct, optional proxy, saved routes/no fallback | Low-level session compatibility retained; no live-login requirement |
| FR029 | Owned retained-response deadline and finite lifecycle fixtures | Exact accepted baseline, not current-source carryover |
| FR030–031 | Rust core/tooling; PS7 exception; reviewed branch integration | Browser API glue/full migration parity unresolved; unique history preserved |
| FR032–034 | Actual aggregate/individual/focused CI contexts and finite diagnostics | Focused success never full acceptance; no automatic initiator/retry |
| FR035–036 | Rooted/system-temp examples; GPL-3.0-or-later original code | Persistent rooting/third-party terms preserved |
| FR037 | Token/row/input/registration/root/process/environment/archive/release/capability repairs | Authored current changes await exact native verification; contention characterized, conditional redesign not claimed |
| FR038 | Revision-scoped receipts, signed standalone verifier, dependency decisions,41historical task dispositions | OtherOS signing/actual host loading/races/broader campaigns remain separately bounded |
| FR039 | Existing local action submenu and genuine terminal approval | No automatic commit or remote dispatch |
| FR040 | Conditional immutable-attempt/recovery/no-replay contract | No permitted production dispatch/postcondition adapter; current manual_handoff unchanged |
| FR041 | All reviewed sidebranch content merges into actual default | Final resulting-SHA CI/ancestry/cleanup proof pending |
| FR042 | Keep latest verified installation and remove proven-obsolete task/project scratch, cache and versions after replacement checks | Preserve configuration/private state/recovery; no shared or unknown purge |

Accepted baseline is `219ec3b`/manual41559809201, documented in [verification report](../verification-report.md). It supersedes older receipt examples above but does not verify current edits. All122WF IDs remain in the canonical registry with31implemented local/manual tools and91dedicated unsupported operations; a local alternative is not a live equivalent.

SC001 requires actual native package/roundtrip evidence. SC002 requires honest dispositions, not every unsupported tool becoming a stub. SC003 requires zero prohibited operations in negative cases. SC004 binds actual runs/assets to exact source SHA. SC005 keeps human credentials private and fixtures separate from live data. Full completion remains blocked by unperformed required acceptance, even if an experimental local package passes CI.
