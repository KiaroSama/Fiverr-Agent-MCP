# Changelog

## Unreleased — 2026-11-01

Removed login/session/logout choices from the launcher and the PowerShell SignIn shortcut; option 6 now shows account status. Fresh installations default to Direct with optional proxy selection. Existing saved routes and private session data remain intact; selected-proxy failures never fall back to Direct. Explicit blocking, blocked backup restore and local MCP HTTP authentication remain unchanged. Low-level session commands remain available for compatibility.

## Prior signed delivery — 2026-10-16

Added explicit private cookie-file import/selected-route verification and a user-gesture Chromium extension/native account-observation bridge, with session menu/CLI/status/forget and account/route/revision fencing. Native validation and signed Windows packaging passed on source `db42d38` in CI38841816625; the exact signed Windows executable was verified and installed with configuration/private state preserved. Live Fiverr and loaded-extension registration remain unverified. Owner-authorized project signing key and protected Windows signing inputs are provisioned. Windows full CI signs and verifies the pinned self-signed identity before packaging; public OS trust is not claimed. Other platforms passed native source validation only; their final signed distributions remain unavailable.

## 0.1.0 — experimental source, 2026-10-12

Initial Rust-native local workspace, MCP stdio/Streamable HTTP adapters, explicit route policy, prepared manual handoffs and reviewed existing optimizer adaptation. Native release and account readiness require executed evidence. No live full-access or universal security/coverage claim.
