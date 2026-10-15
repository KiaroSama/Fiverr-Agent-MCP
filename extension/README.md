# One-time browser account observation

This optional, unofficial Chrome/Chromium Manifest V3 extension sends one small account observation to the paired local Fiverr Agent MCP executable, only after you click **Connect this tab**. It is **not OAuth**, a Fiverr API, independent proof of account ownership, or permission to automate an account. The page DOM is untrusted. Extension observations never set `live_authenticated` to true.

## Install and pair

1. Install the verified native application build with its `extension/` directory. In Chrome, open `chrome://extensions`, enable **Developer mode**, choose **Load unpacked**, and select that directory. Record the extension's displayed ID; it must be 32 lowercase letters from `a` through `p`. Other Chromium browsers have their own extension-management page and native-host registration locations.
2. In the owner CLI, pair exactly that ID and the expected Fiverr username. Run from the application installation, or use an absolute executable path. Replace the bracketed values; they are not real account data.

   ```text
   bin/fiverr-agent-mcp.exe session extension-pair <extension-id> <expected-username>
   bin/fiverr-agent-mcp.exe session extension-manifest
   ```

   Unix installations use `bin/fiverr-agent-mcp` instead. Use the same application root for pairing, manifest generation, and the native host. The username must contain 1–80 ASCII letters, digits, underscores, or hyphens. Re-pair and regenerate if the unpacked extension ID or native installation path changes.
3. Review the generated native-host manifest and the CLI's paths/instructions. Its name must be `com.fiverr_agent_mcp.session`, `type` must be `stdio`, `path` must point to the installed native executable, and `allowed_origins` must contain only `chrome-extension://<extension-id>/` for your paired ID. Never add a wildcard or unrelated extension. Do not copy private application state into this extension directory.
4. **Register the native host manually.** On Windows, create the current-user registry key `HKEY_CURRENT_USER\Software\Google\Chrome\NativeMessagingHosts\com.fiverr_agent_mcp.session` and set its default `REG_SZ` value to the absolute path of the generated manifest. Follow the owning browser's native-messaging documentation for Edge/other Chromium browsers or Linux/macOS manifest locations. Review the command output first; neither this extension nor the manifest-generation command automatically changes the registry. This is separate from MCP-client registration.
5. Sign in manually on `https://www.fiverr.com`, open `/users/manage_gigs` or `/users/<expected-username>/manage_gigs`, and click the extension action, then **Connect this tab**. Keep the popup open for the reply, at most 10 seconds after native connection. Use a clean URL with no query or fragment. A page missing the expected management table, own-profile navigation, or logout navigation is refused, not interpreted as an empty account.

## What crosses the bridge

The fixed extractor reads only the current top-level tab. It requires HTTPS `www.fiverr.com` on the default port, the exact management route, `.js-db-table` in the body, exactly one unique navigation/header own-profile username from `/sellers/<username>/edit`, and a same-origin `/logout` or `/users/logout` navigation link. Relative and absolute links are resolved against the actual page URL, never a page-controlled `<base>` element. A named management route must match that username.

It sends only:

```json
{"type":"account_observation","url":"https://www.fiverr.com/users/manage_gigs","username":"example_owner","management":true,"logout":true}
```

The native host independently validates the paired extension caller, expected username, URL/path and schema, and records its own receive time. A valid response is `{ "ok": true, "status": { ... } }`; a refusal is `{ "ok": false, "error": "Account observation refused; check pairing, account and Fiverr page." }`. The popup accepts only the known sanitized extension status with `state: "account_observed"`, `fresh: true`, the same username, and `live_authenticated: false`. It reports **account observed**, never fully authenticated access. The observation expires after five minutes. Routing assurance and platform automation permission remain unverified.

Permissions are limited to `activeTab`, `scripting`, and `nativeMessaging`. There is no background worker, all-tab scan, cookie permission/read/export, network interception, remote fetch, arbitrary JavaScript command interface, account mutation, or page-body transfer. The extension stores no credentials or observation history and writes no browser logs containing account data. Native-host diagnostics follow the application's sanitized logging policy; do not attach private state or account-page captures to support reports.

## Limits and troubleshooting

- A native host that is absent, unpaired, incorrectly registered, or disconnected cannot produce success. Check the recorded ID, expected username, manifest path, and current-user browser registration; reopen the popup and try again.
- Signed-out, ambiguous, changed, or unsupported page markup is unknown/refused. Do not broaden permissions or add arbitrary selectors to force a success.
- Synthetic CI fixtures exercise the exact JavaScript extractor and response validator. They do not prove the current Fiverr DOM, real account authentication, browser extension loading, native registration, or platform permission. Owner-authorized live validation remains separate.

Chrome platform references: [activeTab](https://developer.chrome.com/docs/extensions/develop/concepts/activeTab), [script injection](https://developer.chrome.com/docs/extensions/reference/api/scripting), and [native messaging/manual host registration](https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging).
