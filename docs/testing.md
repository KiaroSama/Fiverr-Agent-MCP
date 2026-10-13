# Testing and evidence

## CI-only execution

The initial assignment prohibits local Rust compilation/final suites. Cargo lock resolution and source formatting are not builds. The final native matrix is the execution authority: Windows2025 x64/MSVC, Ubuntu22.04 x64/glibc, macOS15 ARM and macOS15 Intel. Actual rustc host must equal target.

CI runs locked fmt, Clippy -Dwarnings, workspace nextest (fail on zero discovery, no retries, all failures collected), doctests, one release build, manifest/capability checks, dependency audit/license checks and archive/extracted-package smoke. Every tests/*.rs target and application unit test is automatically discovered; no hand-maintained per-file skip list. Two build/test workers per runner; nextest per-test30s (required browser fixture40s)/global10m; owned command wall/idle and job bounds. Browser fixture is required when compiled with browser feature; missing native browser fails rather than silently skipping.

## Test seams

Core Money/ID/file policy; Store transaction/history/import and bounded empty-workspace backup restore; shared service productivity/export; prepared-action approval/route/file/draft scope; proxy parse and wire loopback SOCKS5h/dead-route behavior; SDK stdio/HTTP; typed browser/profile leases and controlled local browser schema; licensed pure analysis goldens; archive trust boundary and extracted binary without developer tools.

Synthetic data is labeled fixture/sample. It proves only those cases, never current Fiverr account behavior. No real credentials/private customer data or real messages/deliveries are in public CI. Actual client loading and live account permission/session/selector behavior remain separate acceptance. SSL/HTTPS proxy and all OS-level leak controls are not proven by the current loopback tests.

## Optimization

Pure numeric cases are table-driven, literal independent expectations, no browser/process/network setup. Storage cases use isolated minimal temporary roots. Protocol tests use ephemeral bound listeners rather than sleep-based readiness. Browser fixture uses one bounded headless launch and OS-managed profile lock. Test subprocesses are owned and resource/time bounded; jobs capture sanitized failures. Baseline CI38253015746 recorded Windows13.193s, Linux1.586s, macOS ARM6.170s and Intel18.966s for the native test suites. These are one-run platform observations, not a speedup or coverage claim.

## Requested deeper evidence not yet fulfilled

Linux source line coverage measured on `14bfe1151ce5c276d4c558980eac0169a039c3dd` in CI38352515372 was 62.37% application / 72.09% safety. The requested 90%/95% targets are not met; these are line measurements, not branch or cross-platform coverage. Full parser fuzzing, mutation testing, executable process-owner cancellation/root-exits-first assertions, complete ACL/reparse races, remote-action unknown-outcome reconciliation and full live/client acceptance remain open. Linux native CI replaces its normal nextest run with cargo-llvm-cov nextest and reports measured application/safety line coverage against the requested 90%/95% targets, including target_met flags; reporting a shortfall is not meeting it. Windows-only cfg code is outside that Linux denominator. A deterministic 4,096-case generated corpus checks ID/path rejection, exact money roundtrip and remote-DNS proxy normalization. It is fuzz-style invariant testing, not a coverage-guided fuzzer. A bounded cargo-mutants campaign targets validator return-value mutations, runs only selected primitive/property tests, requires exactly two discovered validator-return mutants and both caught, and checks source restoration before packaging. The unmutated baseline is not repeated. Windows CI additionally verifies current-owner-only protected DACLs on an actual fresh native workspace. These checks are not a full security audit or full mutation/fuzz campaign. See verification-report.md for actual run identity/results.
