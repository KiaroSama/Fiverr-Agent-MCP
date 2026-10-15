# Authentication and account scope

## Two owner-selected session methods

Menu option 6 and `auth sign-in` open the session menu. Choose local cookie-file import or pair the bundled browser extension. These are unofficial session adapters, not a Fiverr OAuth grant. Neither enables remote writes or establishes Fiverr automation permission. Live Fiverr compatibility has not been verified with an owner account.

### Local cookie file

1. Create a private text file under the installation, for example `config/session-input.txt`, containing only your own Cookie header value. Do not include `Cookie:`, a trailing newline or exported JSON. Never send that file or value to an assistant. The app validates ASCII cookie pairs, duplicate names, control characters and the 16 KiB maximum.
2. Select your intended route in the launcher. Cookie verification uses only that route, verified TLS and the fixed `https://www.fiverr.com/users/manage_gigs` page. No redirect, endpoint guessing, direct fallback or proxy rotation occurs.
3. Import using your expected username:

```bash
bin/fiverr-agent-mcp session import config/session-input.txt your_username
```

Relative input paths resolve against the installation in both CLI and menu; absolute paths must remain inside that installation.

```bash
bin/fiverr-agent-mcp session verify
```

A saved cookie is `credential_saved`, not authentication. Verification requires positive management-page markup, one account-navigation username matching the expected account, and a same-origin logout link. A 200 response or empty page is insufficient. Signed-out, challenge, rate-limit, account mismatch and route failures remain explicit errors. Current live markup may differ; a changed page fails closed.

### Logged-in browser extension

Load the bundled `extension/` directory as an unpacked Chromium MV3 extension. It requires only `activeTab`, `scripting` and `nativeMessaging`; no cookie access or network interception. See [extension setup](../extension/README.md) for manual host registration on each supported desktop browser.

```bash
bin/fiverr-agent-mcp session extension-pair abcdefghijklmnopabcdefghijklmnop your_username
```

Replace the example extension ID with the exact ID displayed by your browser.

```bash
bin/fiverr-agent-mcp session extension-manifest
```

Run manifest generation from the installed `bin` executable. It writes a private manifest allowing only the paired extension origin, and never changes registry/browser settings automatically. Existing different manifests are preserved for review. In your already signed-in Fiverr management tab, click the extension and request the account observation. No password, cookie or message body leaves the browser. The Rust host validates the caller, bounded message schema, exact HTTPS authority/path and expected account.

Extension observations report `account_observed:true`, but `live_authenticated:false`: DOM evidence is untrusted and is not independent server authentication. Browser networking remains operator-managed/unverified, separate from native HTTP route settings. The app does not navigate that browser or certify its egress. Native caller arguments are not cryptographic authentication against another same-user local process.

## Status, freshness and removal

```bash
bin/fiverr-agent-mcp session status
```

```bash
bin/fiverr-agent-mcp session forget
```

Selecting import or extension pairing explicitly replaces the selected local session mode; errors never switch it automatically. Account/route changes invalidate observations. Successful observations expire after 300 seconds. Status includes mode, expected username, state, observation time/freshness, native/browser route distinction and unverified automation permission, never raw credentials. `fiverr_account_get` and `fiverr_session_status` expose this sanitized summary; no MCP tool imports cookies or pairs an extension.

Credentials reside in a separately protected SQLite store under `.state/browser`, outside workspace backups/exports. This is private filesystem storage, not application-level encryption. Forget clears the app's credential and observation, not your original input file, browser session or remote account. SQLite secure deletion does not promise forensic erasure on SSDs, backups or journal recovery. Protect/remove your original cookie file yourself when no longer needed. Do not include private input or session files in support attachments.

Remote logout remains a manual official-site action. No recursive browser-profile or draft deletion occurs. The existing dedicated browser fixture infrastructure is distinct from this owner-operated extension.

## Verification limits

Native CI must exercise import/status/forget, routing failures, private backup exclusion, account/URL/schema guards, native binary framing and the exact extension extractor against synthetic browser fixtures. Those tests are not live Fiverr authentication, a loaded-extension/browser registration test, platform permission, strict whole-browser containment or real client acceptance. No live credentials belong in public CI. Real messages, deliveries or financial transactions are never sent just to obtain a passing test.
