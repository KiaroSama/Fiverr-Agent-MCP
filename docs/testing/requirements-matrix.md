# Requirement and acceptance matrix

Status below is authored versus outstanding, not executed evidence. Every named workflow has its own entry in capabilities.json. All test source runs in the native CI workspace bucket; tests that were only written are not green.

| Requirement | Code / artifact | Test seam | Outstanding acceptance |
|---|---|---|---|
| FR001,013,014 | server/, CLI HTTP configuration | protocol.rs, native package | Full legacy/modern client loading and progress/cancellation/extensions |
| FR002,015 | bootstrap/, storage/root, logging/database | primitives/workspace/package | Complete ACL/race/IO-failure/log retention proof |
| FR003,004 | network/profiles, RouteClient | network.rs, unit wire_tests | HTTPS/HTTP CONNECT/auth/redirect/TLS/DNS all-platform assurance |
| FR005,006 | browser/observation, profile lease, gated sign-in | browser.rs, browser_runtime.rs | Real managed manual sign-in/permitted account adapter/current selectors/strict routing |
| FR007,008 | capabilities.json, server/catalog, services | inventory validation, protocol | Dedicated unsupported platform workflows; catalog/local mappings are not live equivalents |
| FR009,010 | local/productivity/portability, exact Money | productivity/workspace/primitives/analysis | Full retention/restore/quotas and all domain-local validators |
| FR011,012,016 | actions/bindings, owner CLI | actions.rs/file primitives | Executing/unknown remote dispatch/reconciliation, all file/approval scope races |
| FR017 | licensed analysis/helper/skill/locks/notices | analysis.rs literal goldens | Native runtime and actual host skill loading |
| FR018,019 | plugin/client manifests, registration/CLI/launchers | manifest/package/CLI | Full launcher/menu/registration concurrent failure tests and actual client schemas/loading |
| FR020,021 | native CI/xtask/release/source/license | four-native matrix/package/security | Exact successful CI/source SHA/release;90/95coverage,fuzz,mutation not measured |
| FR022,023 | public research/reference pins, local tracker routing | primary-source reads/static validation | Sources stay version-specific; local tracking not public issue generation |

SC001 requires actual native package/roundtrip evidence. SC002 requires honest dispositions, not every unsupported tool becoming a stub. SC003 requires zero prohibited operations in negative cases. SC004 binds actual runs/assets to exact source SHA. SC005 keeps human credentials private and fixtures separate from live data. Full completion remains blocked by unperformed required acceptance, even if an experimental local package passes CI.
