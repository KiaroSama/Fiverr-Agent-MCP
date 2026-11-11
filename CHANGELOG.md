# Changelog

## Unreleased — 2026-11-11

Added owner self-signed distribution signing for macOS and Linux. Full CI runs now package every native target. macOS binaries are signed with `codesign` using an owner identity imported into an ephemeral keychain through Security.framework, then strictly verified and compared against the published leaf pin in `config/signing-macos-public.json`. Non-Windows tar.gz archives and checksum files receive detached OpenSSH signatures in the `fiverr-agent-mcp-release` namespace, verified against the pinned key in `config/signing-release-public.json` with wrong-namespace and wrong-signer negatives. Both identities are self-signed and not publicly trusted. Gatekeeper will still treat the macOS binary as coming from an unidentified developer, and it is not notarized. Exact-source acceptance is pending.

Added a separately locked coverage-guided validation harness with LLVM counters and bounded seed/time/input/RSS settings, executed only after the Linux sandbox controls pass. Production toolchain and dependencies remain separate. Source7620e80 passed the integrated fuzz/native/sign/package run43077882092 and its signed Windows build was installed.

Added a constrained Linux cleanup-denial qualification using actual kernel EPERM, real bootstrap failure reporting and outer-owner descendant reap. Added isolated real loaded-extension/native-host acceptance; its execution and the final integrated native/sign/package checks remain revision-specific. No live Fiverr access or arbitrary race-safety claim is implied.

Windows private permissions now derive the trustee SID from the current process token instead of `USERNAME`. The bounded icacls mechanism and fail-closed storage behavior remain; native regressions cover absent/invalid environment identity and existing owner-only DACL acceptance. This does not make path-based filesystem operations atomic. Current-source acceptance is pending.

Updated the reviewed database cohort to rusqlite0.42.0/libsqlite3-sys0.40.0, terminal password input to rpassword7.5.5 and packaging to zip9.5.1. Extended bounded mutation checks from two to six distinct validators with money/kind/text/provenance negatives. These changes require their own exact-source native and signed-package acceptance; prior delivery receipts do not verify them.

Moved synchronous workspace work for MCP and HTTP authentication off runtime threads. The bounded queue cancels unstarted work. Started writes finish with a private completion receipt; slow writes return an operation ID for `fiverr_status`. Unfinished receipts are never replayed.

Expanded the loopback route matrix with HTTPS-proxy double TLS, independent proxy/destination trust and hostname refusal, and deterministic bootstrap DNS failure without destination resolution or direct fallback. Added xtask dependency-update coverage and aligned the remaining Deep validation action pin with the existing reviewed version. New fixture acceptance is revision-specific; no live-account or OS-wide DNS assurance is implied.

Aligned four local-operation approval labels with enforced controls while retaining owner approval for action commit. Added a per-tool named fixture evidence map with explicit historical/current separation, actual dispatch coverage, bounded Unix supervisor terminal-path fixtures, CONNECT refusal/TLS failure cases and semantic contradictory-selector status tests. These are finite regression contracts, not live verification or complete process/network certification.

Added native process-root-exit cleanup regressions, complete per-run case inventories and a relocated local draft/export workflow across stdio and protected HTTP. The signed Windows package reuses that harness against extracted bytes, and bundled documentation now includes its referenced domain glossary. Reviewed dependency branches retain their merge ancestry; current pins include rmcp3.9.0, flate2 1.1.11, zip9.5.0, uuid1.29.2 and install-action2.90.3. Final integrated native/sign/package acceptance remains required.

Authored maintenance hardening for read-only established HTTP-token access, persisted SQL/JSON identity binding, fatal oversized terminal framing, conservative same-install registration, local action review and pre-write launcher refusal. Added Windows ordinal environment construction, exact release-asset/receipt binding, semantic capability validation and one-shot logging-degradation diagnostics. A signed standalone authenticity utility reuses the existing CMS/PE verifier. These changes remain subject to final exact-source native/sign/package acceptance; no new installed-build or live-platform claim is implied.

## Accepted maintenance baseline — 2026-11-03

Corrected CI resource accounting for the two full-browser fixtures on Windows and Intel macOS: each reserves the existing worker budget while other tests retain parallelism. Readiness/case deadlines, assertions, sandbox and zero-retry policy remain unchanged. Temporary debugger/verbose probes were retired after the controlled comparison.

Added bounded Windows readiness/launcher phase diagnostics and finite optional CI samples without relaxed deadlines or hidden retries. Individual actual native/dependency commit contexts accompany aggregate status. Completed neutral Windows setup and rooted/system-temp examples; explicit system temporary policy preserves persistent rooting. Original project licensing is now GPL-3.0-or-later; third-party terms remain unchanged. Timing cause requires actual diagnostic evidence.

Enforced the100-profile recovery limit on transactional profile growth while preserving legacy over-limit inspection/deletion and fail-closed selection. Added focused regression and atomic import/local-selection/concurrent-growth coverage. Manual native CI now publishes its actual aggregate status on the exact commit. Integrated reviewed database/parser/SDK/archive and workflow dependency updates, subject to revision-specific native acceptance.

Added independent HTTP response-deadline ownership after a native test reproduced capacity retention without downstream polling. Rust now owns CI orchestration, process cleanup, Windows signing and package/release gates; direct native startup remains available. Restored the optional Windows PowerShell 7 launcher at the owner's request, including Preview/Status, rooted logging and packaged delivery; CI/signing/process tooling stays Rust. The optional browser extension still requires JavaScript, so Rust-only migration is not complete. Updated compatible SDK/runtime/archive dependencies; native acceptance is revision-specific.

## Connection setup — 2026-11-01

Removed login/session/logout choices from the launcher and the PowerShell SignIn shortcut; option 6 now shows account status. Fresh installations default to Direct with optional proxy selection. Existing saved routes and private session data remain intact; selected-proxy failures never fall back to Direct. Explicit blocking, blocked backup restore and local MCP HTTP authentication remain unchanged. Low-level session commands remain available for compatibility.

## Prior signed delivery — 2026-10-16

Added explicit private cookie-file import/selected-route verification and a user-gesture Chromium extension/native account-observation bridge, with session menu/CLI/status/forget and account/route/revision fencing. Native validation and signed Windows packaging passed on source `db42d38` in CI38841816625; the exact signed Windows executable was verified and installed with configuration/private state preserved. Live Fiverr and loaded-extension registration remain unverified. Owner-authorized project signing key and protected Windows signing inputs are provisioned. Windows full CI signs and verifies the pinned self-signed identity before packaging; public OS trust is not claimed. Other platforms passed native source validation only; their final signed distributions remain unavailable.

## 0.1.0 — experimental source, 2026-10-12

Initial Rust-native local workspace, MCP stdio/Streamable HTTP adapters, explicit route policy, prepared manual handoffs and reviewed existing optimizer adaptation. Native release and account readiness require executed evidence. No live full-access or universal security/coverage claim.
