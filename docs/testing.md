# Testing and evidence

## CI-only execution

The initial assignment prohibits local Rust compilation/final suites. Cargo lock resolution and source formatting are not builds. The final native matrix is the execution authority: Windows2025 x64/MSVC, Ubuntu22.04 x64/glibc, macOS15 ARM and macOS15 Intel. Actual rustc host must equal target.

CI runs locked fmt, Clippy -Dwarnings, workspace nextest (fail on zero discovery, no retries, all failures collected), doctests, one release build, manifest/capability checks, dependency audit/license checks and archive/extracted-package smoke. Every tests/*.rs target and application unit test is automatically discovered; no hand-maintained per-file skip list. Two build/test workers per runner; nextest per-test30s/global10m; owned command wall/idle and job bounds. Browser fixture is required when compiled with browser feature; missing native browser fails rather than silently skipping.

## Test seams

Core Money/ID/file policy; Store transaction/history/import; shared service productivity/export; prepared-action approval/route/file/draft scope; proxy parse and wire loopback SOCKS5h/dead-route behavior; SDK stdio/HTTP; typed browser/profile leases and controlled local browser schema; licensed pure analysis goldens; archive trust boundary and extracted binary without developer tools.

Synthetic data is labeled fixture/sample. It proves only those cases, never current Fiverr account behavior. No real credentials/private customer data or real messages/deliveries are in public CI. Actual client loading and live account permission/session/selector behavior remain separate acceptance. SSL/HTTPS proxy and all OS-level leak controls are not proven by the current loopback tests.

## Optimization

Pure numeric cases are table-driven, literal independent expectations, no browser/process/network setup. Storage cases use isolated minimal temporary roots. Protocol tests use ephemeral bound listeners rather than sleep-based readiness. Browser fixture uses one bounded headless launch and OS-managed profile lock. Test subprocesses are owned and resource/time bounded; jobs capture sanitized failures. First native duration baseline remains unknown until a run; no speedup or coverage claim is invented.

## Requested deeper evidence not yet fulfilled

Application logic90% and safety primitives95% coverage targets have not been measured/enforced. Full parser fuzzing, mutation testing, executable process-owner cancellation/root-exits-first assertions, complete ACL/reparse races, remote-action unknown-outcome reconciliation and full live/client acceptance remain open. The Deep workflow currently runs only actual archive trust-boundary assertions; it is not a completed security audit or fuzz/mutation campaign. See verification-report.md for actual run identity/results.
