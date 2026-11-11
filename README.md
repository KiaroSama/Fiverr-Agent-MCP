# Fiverr Agent MCP

<div align="center">

[![CI](https://github.com/KiaroSama/Fiverr-Agent-MCP/actions/workflows/ci.yml/badge.svg?branch=feature%2Ffiverr-native)](https://github.com/KiaroSama/Fiverr-Agent-MCP/actions/workflows/ci.yml)
[![Security](https://github.com/KiaroSama/Fiverr-Agent-MCP/actions/workflows/security.yml/badge.svg?branch=feature%2Ffiverr-native)](https://github.com/KiaroSama/Fiverr-Agent-MCP/actions/workflows/security.yml)
[![Secret scanning](https://github.com/KiaroSama/Fiverr-Agent-MCP/actions/workflows/gitleaks.yml/badge.svg?branch=feature%2Ffiverr-native)](https://github.com/KiaroSama/Fiverr-Agent-MCP/actions/workflows/gitleaks.yml)
[![Deep validation](https://github.com/KiaroSama/Fiverr-Agent-MCP/actions/workflows/deep.yml/badge.svg?branch=feature%2Ffiverr-native)](https://github.com/KiaroSama/Fiverr-Agent-MCP/actions/workflows/deep.yml)
[![Release](https://github.com/KiaroSama/Fiverr-Agent-MCP/actions/workflows/release.yml/badge.svg?branch=feature%2Ffiverr-native)](https://github.com/KiaroSama/Fiverr-Agent-MCP/actions/workflows/release.yml)
[![License GPL-3.0-or-later](https://img.shields.io/badge/License-GPL--3.0--or--later-blue)](LICENSE)
[![Source version 0.1.0](https://img.shields.io/badge/Source-0.1.0-orange)](Cargo.toml)
[![Rust 1.99](https://img.shields.io/badge/Rust-1.99-black?logo=rust)](rust-toolchain.toml)
[![Documentation](https://img.shields.io/badge/Docs-installation-blue)](docs/installation.md)
[![MCP SDK rmcp](https://img.shields.io/badge/MCP-rmcp-5c4ee5)](Cargo.toml)
[![HTTP Axum](https://img.shields.io/badge/HTTP-Axum-5c4ee5)](Cargo.toml)
[![Async Tokio](https://img.shields.io/badge/Async-Tokio-5c4ee5)](Cargo.toml)
[![Storage SQLite](https://img.shields.io/badge/Storage-SQLite-003b57?logo=sqlite)](docs/architecture.md)
[![TLS rustls](https://img.shields.io/badge/TLS-rustls-blue)](Cargo.toml)
[![Serialization serde](https://img.shields.io/badge/Serialization-serde-blue)](Cargo.toml)
[![Browser chromiumoxide](https://img.shields.io/badge/Browser-chromiumoxide-orange)](docs/authentication.md)
[![Support donations](https://img.shields.io/badge/Support-donations-d04a9a)](#donate)

</div>

A Rust-native local workspace and MCP server for Fiverr records, drafts, notes/tasks, exports, reviewed market analysis and human handoffs. Work locally without login; connect directly or use an optional proxy. The Windows package includes a native executable and an optional PowerShell 7 launcher.

This community project is not affiliated with Fiverr. Local workspace operations and fixture-tested integrations are distinct from live account results. Automated account access and remote mutations remain gated; see the [capability guide](docs/capabilities.md) for precise supported boundaries.

## Install and connect

### 1. Download and verify the Windows package

Windows x64 is the established signed binary distribution. Full CI runs also produce owner self-signed Linux x64 and macOS ARM64/Intel packages: macOS binaries carry an owner `codesign` signature (not notarized), and each non-Windows archive has a detached OpenSSH `.sig` checkable with `ssh-keygen -Y verify` against `config/signing-release-public.json`. The instructions below cover Windows.

Open [CI](https://github.com/KiaroSama/Fiverr-Agent-MCP/actions/workflows/ci.yml), select a successful **full** run for the intended source revision, and download `fiverr-x86_64-pc-windows-msvc` under **Artifacts**. GitHub may require signing in. Validation-only runs have no distribution. Artifacts expire; [Releases](https://github.com/KiaroSama/Fiverr-Agent-MCP/releases) is an alternative only when an actual matching release exists. **Code → Download ZIP** downloads source, not an installed application.

Extract the artifact wrapper first. It contains a native ZIP, source/plugin archives, `SHA256SUMS-x86_64-pc-windows-msvc.txt` and release metadata. Compare the native ZIP hash with its entry in that checksum file using PowerShell 7:

```powershell
Get-FileHash -Algorithm SHA256 -LiteralPath './fiverr-agent-mcp-0.1.0-x86_64-pc-windows-msvc.zip'
```

Check that `release-x86_64-pc-windows-msvc.json` identifies the intended source SHA. For packages containing signed `bin/fiverr-verify.exe`, follow [the copyable authenticity check](docs/installation.md#packaged-authenticity-utility). Authenticate the utility/archive hash against the exact accepted CI receipt before running it; earlier packages require the verification procedure appropriate to their receipt. A checksum downloaded with an archive detects corruption, not independent publisher authenticity. The signature is self-signed, not publicly CA-trusted; no absence of SmartScreen warnings is promised. Do not disable Windows security or install a root certificate just to suppress a warning.

### 2. Extract and start

Extract the **entire native ZIP** into a writable directory, for example `C:\Apps\Fiverr-Agent-MCP`. Replace this neutral path with your chosen directory in every example. Keep the package together:

```text
Fiverr-Agent-MCP/
  bin/fiverr-agent-mcp.exe
  bin/fiverr-verify.exe
  run.ps1
  fiverr.install.json
  config/signing-public.json
  docs/
  examples/
  licenses/
  LICENSE
  THIRD_PARTY_NOTICES.md
```

Release users need no Rust, Cargo, Node.js, npm, Python or uv. PowerShell 7 is needed only for `run.ps1`; the native executable does not require it. No browser is needed for local records or stdio MCP.

Run these from PowerShell 7; no particular working directory is required:

```powershell
& 'C:\Apps\Fiverr-Agent-MCP\bin\fiverr-agent-mcp.exe' --version
```

```powershell
& 'C:\Apps\Fiverr-Agent-MCP\bin\fiverr-agent-mcp.exe' status
```

`status` returns JSON. Fresh settings report route mode `direct`; saved settings can report `proxy` or `blocked`. This checks local startup, not live Fiverr authentication.

```powershell
pwsh -NoProfile -File 'C:\Apps\Fiverr-Agent-MCP\run.ps1'
```

The last command opens the interactive management menu. Native startup also works by opening `bin/fiverr-agent-mcp.exe` directly.

### Claude Desktop on Windows

Open Claude Desktop's **Settings → Developer → Edit Config**. Its file is normally `%APPDATA%\Claude\claude_desktop_config.json`; packaged installations may use a virtualized location, so prefer the app's editor. Merge only `Fiverr-Agent-mcp` into your existing `mcpServers` object, preserving other entries/settings. Use your actual absolute executable path, not a relative path.

#### Option A — installation-local temp (default)

Startup creates private temporary/cache directories inside the installation. JSON backslashes must be doubled; `%TEMP%` is not expanded inside JSON strings.

```json
{
  "mcpServers": {
    "Fiverr-Agent-mcp": {
      "command": "C:\\Apps\\Fiverr-Agent-MCP\\bin\\fiverr-agent-mcp.exe",
      "args": ["bridge"],
      "env": {
        "FIVERR_AGENT_TEMP_MODE": "rooted",
        "TEMP": "C:\\Apps\\Fiverr-Agent-MCP\\.cache\\tmp",
        "TMP": "C:\\Apps\\Fiverr-Agent-MCP\\.cache\\tmp",
        "TMPDIR": "C:\\Apps\\Fiverr-Agent-MCP\\.cache\\tmp",
        "XDG_CACHE_HOME": "C:\\Apps\\Fiverr-Agent-MCP\\.cache"
      }
    }
  }
}
```

#### Option B — normal system temp

This explicitly retains the client's inherited temporary/cache environment and normal OS temp resolution. Omit Option A's TEMP/TMP/TMPDIR overrides. System temp does not mean a shared public directory. Persistent workspace data, logs, browser profiles and exports remain inside the installation.

```json
{
  "mcpServers": {
    "Fiverr-Agent-mcp": {
      "command": "C:\\Apps\\Fiverr-Agent-MCP\\bin\\fiverr-agent-mcp.exe",
      "args": ["bridge"],
      "env": {
        "FIVERR_AGENT_TEMP_MODE": "system"
      }
    }
  }
}
```

Choose **one** entry, not both. System mode requires a build containing `FIVERR_AGENT_TEMP_MODE`; older builds always root temporary environment variables. The default is `rooted`; invalid/empty values fail without fallback. The optional PowerShell launcher honors the same choice. Ensure the client itself has not inherited installation-local overrides when choosing system temp.

Save the Claude Desktop configuration and fully quit/restart Claude Desktop. Stdio does not need a separately running HTTP server, token, Node.js, Python or Rust. Fresh installations use direct outbound networking without login or proxy setup. Existing saved Direct, Proxy or Blocked choices are preserved; connecting MCP does not sign in to Fiverr. See the [Windows example](examples/clients/claude-desktop.windows.json) and [official local-server guide](https://modelcontextprotocol.io/docs/develop/connect-local-servers).

Generate a rooted configuration for the actual installation path, or use the [system-temp example](examples/clients/claude-desktop.system-temp.windows.json). After restarting the client, confirm the server appears connected and ask it to call `fiverr_health` or `fiverr_status`. A successful result is local health, not account authentication.

Windows example, from the installation directory:

```bash
./bin/fiverr-agent-mcp.exe desktop-config
```

Standalone MCP stdio:

```bash
./bin/fiverr-agent-mcp.exe bridge
```

Local Streamable HTTP, separately started:

```bash
./bin/fiverr-agent-mcp.exe http
```

The listener defaults to `http://127.0.0.1:1597/mcp`. Token protection is on by default. Export authorization only through the explicit interactive owner CLI; never put it in a URL, argument, issue or screenshot. Both transports use the official SDK and the same service validation. [Installation](docs/installation.md) covers Unix, client formats and port settings.

### Troubleshooting

| Symptom | Check |
| --- | --- |
| Executable missing | Extract the native ZIP, not only the wrapper/source archive; preserve bin/ and the marker. |
| Server missing in client | Validate JSON, use an absolute command path and fully restart the client. |
| Storage/permission refusal | Choose a writable local directory without linked/reparse ancestors; do not elevate to bypass the check. |
| bridge waits in a terminal | It expects MCP frames; use status for a standalone startup check. |
| Saved proxy fails | Select Direct explicitly if desired; automatic fallback is deliberately forbidden. |

Inspect logs/ without posting raw state, cookies or tokens. [Upgrade/recovery](docs/installation.md#upgradeuninstall) explains private backups, stopped clients and replacement of verified payload only.

## Launcher

Run `bin/fiverr-agent-mcp.exe` on Windows (`bin/fiverr-agent-mcp` on Unix) to open the Rust management menu directly. The optional Windows PowerShell 7 launcher is restored: `pwsh -NoProfile -File ./run.ps1` opens the same menu, `-Preview` prints it and `-Status` returns JSON. Native `preview`/`status` remain available; no login shortcut is restored. The existing palette, proxy controls, account status, registration, HTTP startup and workspace commands remain. Hidden proxy input and plain redirected/NO_COLOR output are supported. See [launcher controls](docs/installation.md#launcher-menu).

No login is required for the local workspace. Login, cookie import, extension pairing and logout choices are absent from the launcher; option 6 shows account status. Proxy use is optional: fresh installations start in Direct mode, option 1 selects a saved proxy, option 4 explicitly returns to Direct and option 5 blocks outbound connections. Existing saved routes and private session data are not reset.

## What exists

- Account-scoped SQLite local drafts/imports/history, bounded search/pagination, snapshots, reminders and JSON/CSV/Markdown/HTML exports. Imported data is not a live refresh; an empty local collection is not an empty Fiverr account.
- Exact integer money with explicit currency/scale. The adapted existing optimizer supplies keyword, exact pricing, supplied-dataset and observed-metric analysis, preserving missing values and licensing.
- Direct networking by default on fresh installations, optional explicit proxy profiles, preserved saved routes, explicit blocking, no proxy-failure fallback and generation cancellation. Switching a saved route to Direct requires owner consent. Browser routing is a separate unverified prerequisite.
- Human-reviewed preparation and single-use manual handoff journal, file hashes and draft-version binding. No real message, delivery, Gig publication, checkout or payout is dispatched.

The full [capability inventory](capabilities.json) gives each requested workflow an independent implementation/verification/availability/effect/mode disposition. Unsupported dedicated workflows are named explicitly, not presented as working generic aliases. [Capability guide](docs/capabilities.md).

## Account access is currently gated

Optional low-level `auth`/`session` commands remain for compatibility, but the launcher no longer offers login or session setup. These commands include private local cookie-file import with selected-route verification and a bundled user-gesture browser extension/native account-observation bridge. Saved credentials are not authentication; positive account evidence must match the expected username and expires after five minutes. Extension DOM observations are not independent server authentication. See [authentication/setup](docs/authentication.md) and [extension installation](extension/README.md). A configured browser, local account name or passing synthetic fixture does not prove live Fiverr access. Automation permission and strict browser routing remain unverified.

No official comprehensive Fiverr account API/OAuth/MCP was verified. Fiverr's current Terms restrict automated access; owning an account is not platform permission. [Platform research](docs/research/platform-access.md) and [authentication](docs/authentication.md) explain the prerequisites. No CAPTCHA bypass, anti-detection, cookie extraction, arbitrary JavaScript/CDP/shell/URL proxy, mass messaging or automatic financial actions.

## Privacy and logs

State, browser profiles, exports and logs resolve under the installation, not caller CWD. Temporary/cache environment is rooted by default; explicit system mode retains inherited values. Direct binary startup uses an early same-executable rooted-environment restart; no scripting runtime is required for launch. Private state must remain private when moving/backing up an installation.

Each execution writes a new UTF-8 UTC log under logs/, named `fiverr-agent-mcp_YYYY-MM-DD_HH-mm-ss_UTC_<id>.log`. Structured JSON entries carry timestamp, level, category, message and sanitized context. INFO/WARNING/ERROR/DEBUG are supported; no private message bodies, tokens, cookies, passwords or credential URLs are logged. Bridge stdout contains protocol frames only. Logs are capped per file and retention attempts skip active locked files. Inspect metadata with `logs inspect`; do not attach raw state, profiles or authorization files to an issue. [Privacy/file policy](docs/privacy-and-files.md).

## Development and verification

This task's builds/tests/packages run only in GitHub CI. The required native targets are Windows x64/MSVC, Linux x64/glibc, macOS ARM64 and macOS Intel; architecture is asserted, not cross-compilation. Locked fmt/Clippy, nextest, protocol/state/negative fixtures, browser fixture, dependency checks and extracted-package smoke are configured with worker/time bounds. Actual results are reported separately; no coverage percentage, full security audit, live-account result or client-load success is implied. [Testing](docs/testing.md), [architecture](docs/architecture.md), [release](docs/release.md).

## License

Original application: GPL-3.0-or-later, Copyright (C) 2026 KiaroSama, no warranty. Reviewed upstream helper MIT notices and independently adapted CC-BY-4.0 instructions remain separate. [Third-party notices](THIRD_PARTY_NOTICES.md), [skill adaptation](docs/skills.md).

## Donate

If this project helps you, donations are appreciated.

| Currency | Network | Address |
| --- | --- | --- |
| Bitcoin (BTC) | Bitcoin | `bc1qmth5m03pu5hujw5xw5jmywam3jj3sqwqupesdt` |
| USDT, BNB, USDC, etc. | BEP20 | `0x0Bd0BA443a8B9cf15922bf7f0Bb0a4b495fD06Ef` |
| USDT, TRX, USDC, etc. | TRC20 | `TWBA3xFTqgZAeAYMxqo85xWnzvty3DcAhw` |
| Ethereum (ETH) | ERC20 | `0x0Bd0BA443a8B9cf15922bf7f0Bb0a4b495fD06Ef` |
| TON | TON | `UQCN8Umo_OfOWqImZetQsrNStPcmLkMAKajFyiCOhso23NDb` |
| Litecoin (LTC) | LTC | `ltc1qntqnnrunadurnw4cshv3qgspywrueyyeyngwuy` |
| Solana (SOL) | Solana | `7B2wkczUjmkDhETwQuknBL8sUsbuV7nErxc317TmQuwR` |
| Polygon (POL) | Polygon | `0x0Bd0BA443a8B9cf15922bf7f0Bb0a4b495fD06Ef` |
