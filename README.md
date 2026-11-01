# Fiverr Agent MCP

<div align="center">

[![CI](https://github.com/KiaroSama/Fiverr-Agent-MCP/actions/workflows/ci.yml/badge.svg?branch=feature%2Ffiverr-native)](https://github.com/KiaroSama/Fiverr-Agent-MCP/actions/workflows/ci.yml)
[![License GPL-3.0-only](https://img.shields.io/badge/License-GPL--3.0--only-blue)](LICENSE)
[![Source version 0.1.0](https://img.shields.io/badge/Source-0.1.0-orange)](Cargo.toml)
[![Rust 1.99](https://img.shields.io/badge/Rust-1.99-black?logo=rust)](rust-toolchain.toml)
[![Edition 2024](https://img.shields.io/badge/Edition-2024-black)](Cargo.toml)
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

**Experimental, unofficial, and not live-account verified.** A Rust-native local workspace and MCP server for explicit Fiverr observations, drafts, notes/tasks, reviewed market analysis and human handoffs. It is not an official Fiverr API and does not currently provide full authenticated account automation. See [current verification](docs/verification-report.md) before relying on a binary or an account capability.

## Install and connect

1. Obtain a native archive for your actual OS/architecture from a successful [CI run](https://github.com/KiaroSama/Fiverr-Agent-MCP/actions/workflows/ci.yml), or from [Releases](https://github.com/KiaroSama/Fiverr-Agent-MCP/releases) when a verified release exists. CI artifacts are experimental and expire; a release is not implied. Verify the exact source SHA/checksums and native CI; authored workflows are not release evidence.
2. For the current Windows delivery, also verify the pinned self-signed owner signature; see [signing and trust](docs/installation.md#windows-signing-and-trust). This is private cryptographic identity, not public CA trust or a SmartScreen guarantee. Other targets remain validation-only until appropriate signing is available. Extract the entire archive into a writable directory. Keep bin/, fiverr.install.json, docs/ and notices together. Release users need no Rust, Cargo, Node, npm, Python or uv.
3. Run the extracted binary's `desktop-config` command and merge its `Fiverr-Agent-mcp` entry into your compatible client configuration. Plugin installation, runtime registration and account sign-in are separate operations.

### Claude Desktop on Windows

Keep the verified Windows executable at `bin/fiverr-agent-mcp.exe` beside the installation's `fiverr.install.json`, documentation and notices. The following example uses the owner's installation path; change the absolute paths if you move the installation. Merge only `Fiverr-Agent-mcp` into your existing `mcpServers` object: do not replace your other MCP entries or settings.

```json
{
  "mcpServers": {
    "Fiverr-Agent-mcp": {
      "command": "G:\\Program Files\\Portable\\Scripts\\Fiverr Agent MCP\\bin\\fiverr-agent-mcp.exe",
      "args": ["bridge"],
      "env": {
        "TEMP": "G:\\Program Files\\Portable\\Scripts\\Fiverr Agent MCP\\.cache\\tmp",
        "TMP": "G:\\Program Files\\Portable\\Scripts\\Fiverr Agent MCP\\.cache\\tmp",
        "TMPDIR": "G:\\Program Files\\Portable\\Scripts\\Fiverr Agent MCP\\.cache\\tmp",
        "XDG_CACHE_HOME": "G:\\Program Files\\Portable\\Scripts\\Fiverr Agent MCP\\.cache"
      }
    }
  }
}
```

Save the Claude Desktop configuration and fully quit/restart Claude Desktop. Stdio does not need a separately running HTTP server, token, Node.js, Python or Rust. Fresh installations use direct outbound networking without login or proxy setup. Existing saved Direct, Proxy or Blocked choices are preserved; connecting MCP does not sign in to Fiverr. See the [Windows example](examples/clients/claude-desktop.windows.json) and [official local-server guide](https://modelcontextprotocol.io/docs/develop/connect-local-servers).

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

## Launcher

`run.ps1` opens the native management menu on Windows (`run.sh` on Unix). Its Agensi/Upwork-style palette and layout provide proxy import/selection/addition, direct/block controls, account status without login, MCP registration, local-proxy settings, HTTP startup/port, diagnostics and account/workspace submenus. `run.ps1 -Preview` is non-interactive; `-Status` remains JSON. Hidden proxy input and plain redirected/NO_COLOR output are supported. See [launcher controls](docs/installation.md#launcher-menu).

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

State, browser profiles, exports, logs and temporary paths resolve under the installation, not caller CWD. Direct binary startup uses an early same-executable rooted-environment restart; launchers also set child paths. Private state must remain private when moving/backing up an installation.

Each execution writes a new UTF-8 UTC log under logs/, named `fiverr-agent-mcp_YYYY-MM-DD_HH-mm-ss_UTC_<id>.log`. Structured JSON entries carry timestamp, level, category, message and sanitized context. INFO/WARNING/ERROR/DEBUG are supported; no private message bodies, tokens, cookies, passwords or credential URLs are logged. Bridge stdout contains protocol frames only. Logs are capped per file and retention attempts skip active locked files. Inspect metadata with `logs inspect`; do not attach raw state, profiles or authorization files to an issue. [Privacy/file policy](docs/privacy-and-files.md).

## Development and verification

This task's builds/tests/packages run only in GitHub CI. The required native targets are Windows x64/MSVC, Linux x64/glibc, macOS ARM64 and macOS Intel; architecture is asserted, not cross-compilation. Locked fmt/Clippy, nextest, protocol/state/negative fixtures, browser fixture, dependency checks and extracted-package smoke are configured with worker/time bounds. Actual results are reported separately; no coverage percentage, full security audit, live-account result or client-load success is implied. [Testing](docs/testing.md), [architecture](docs/architecture.md), [release](docs/release.md).

## License

Original application: GPL-3.0-only, Copyright (C) 2026 KiaroSama, no warranty. Reviewed upstream helper MIT notices and independently adapted CC-BY-4.0 instructions remain separate. [Third-party notices](THIRD_PARTY_NOTICES.md), [skill adaptation](docs/skills.md).

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
