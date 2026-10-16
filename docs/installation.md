# Installation and clients

## Native package

Choose a verified archive matching your host. The current signing-enabled delivery path produces Windows x64; Linux/macOS remain source-validation targets until their signing prerequisites are resolved. Extract everything, not just the executable. Verify SHA256SUMS and release JSON against the exact source SHA and successful native job. No native archive is verified merely because the source compiles elsewhere.

Standard layout: bin/fiverr-agent-mcp.exe (Windows) or bin/fiverr-agent-mcp (Unix), fiverr.install.json, launchers, docs, examples, config templates, notices and dependency licenses. The executable derives the root from bin/marker. For an explicit nonstandard installation, supply global `--root` with an existing absolute writable directory. Relative roots, linked/reparse ancestors, unsupported marker and read-only storage fail rather than fall back to OS temp.

Run `--help`, `--version` or `version` without creating state. `status`, `desktop-config` and `capabilities` are JSON commands. Run.ps1 requires PowerShell7 and performs no compiler/tool installation; run.sh is POSIX. Direct executable startup works without a launcher.

## Windows signing and trust

The Windows executable uses the owner-authorized self-signed `KiaroSama` code-signing identity. Public certificate and SHA256 pin are in `config/signing-public.json`. Verify signature integrity and the expected signer as well as package hashes. A self-signed certificate is not publicly trusted and does not guarantee removal of Windows/SmartScreen warnings. Do not install it into a trusted-root store merely to suppress a warning; this project does not do so automatically. The native `version` command includes publisher and source identity. Private keys/passwords are never shipped.

## Launcher menu

Run `run.ps1` on Windows or `run.sh` on Unix, or execute the native binary with `menu`. The native menu follows the Agensi/Upwork console layout: bright cyan heading/numbers, green default/success, yellow warnings, red errors and readable connection/proxy/account summaries. `NO_COLOR` or `TERM=dumb` disables color; redirected preview is plain text.

The main menu exposes proxy selection/import/addition, explicit direct/block, Sign in, client registration, local-proxy settings/activation, local HTTP startup/port and diagnostics. Account/browser and workspace submenus expose the existing account status, browser configuration, actions, backup, logs, diagnostics export, cache inspection and HTTP settings. Destructive or authority-changing operations retain their existing owner-terminal consent and prerequisites. Starting HTTP occupies this terminal until stopped.

`run.ps1 -Preview` prints the menu without waiting for input; `-Status` prints JSON; `-SignIn` invokes the same gated native sign-in command. Existing native arguments are still accepted, but launcher switches cannot be combined. Windows PowerShell forwards once to installed PowerShell7; no dependency is installed automatically. Paths resolve relative to the installation, not the caller's directory. Enter selects proxy selection; `0`/`back` cancels a form, `quit`/`exit` closes the main menu and EOF exits without applying a default action.

Proxy import accepts the existing URL and `Label | URL` formats plus the reference `scheme://host:port:username:password:label` form (IPv6 in brackets). Prefer standard percent-encoded URLs when fields contain separators. Imports remain bounded to256KiB/100profiles, validate all lines and preserve the active route. Reimporting the same name/endpoint skips it; a conflicting name cancels the transaction. Proxy URLs typed in the menu are hidden; credentials never appear in summaries, command arguments or logs. Keep real lists private. Quoted relative paths and ordinary absolute Windows drive paths inside the installation are accepted; Windows canonical/verbatim spelling does not make an owned file external. Outside paths, dot/traversal components and linked/reparse paths remain refused.

**Sign in has two explicit session methods:** local private cookie-file import/selected-route verification and a user-gesture browser extension/native account-observation bridge. Option6 opens their import, verify, pair, manifest, status and forget actions. Cookie presence is not authentication, extension DOM is not independent authentication, and neither grants automation permission. Read [authentication](authentication.md) before supplying local credentials and [extension setup](../extension/README.md) before manual browser registration. Live Fiverr compatibility remains unverified; no official OAuth endpoint is invented.

## Stdio runtime

Configure an absolute executable command and args `["bridge"]`. No HTTP listener is required. `desktop-config` generates the real installation paths plus TEMP/TMP/TMPDIR/XDG_CACHE_HOME. The public Windows example has the owner's requested sample installation path; regenerate it for another directory. Unix clients use the corresponding native executable, not PowerShell or a Node shim.

`register <absolute-file>` and `unregister <absolute-file>` require local terminal consent and only alter this installation's Fiverr-Agent-mcp entry, preserving other entries with a backup. JSON validation is not proof of real host loading; review the generated entry before registration.

## HTTP runtime

Run `http` explicitly. It binds only 127.0.0.1. `http-port 1598` saves a new port; `http --port 1599` is one-shot. Precedence: CLI > FIVERR_AGENT_HTTP_PORT > saved >1597. Only ASCII digits 1..65535; no port0 or bind-to-next-free behavior.

`http-auth status` shows mode, never the token. `http-auth export <filename>` needs owner-terminal consent and writes a private authorization file under exports/. Configure a compatible client using the private header, never paste it into chat or a public template. `http-auth rotate` invalidates the old token for future requests. `http-auth set loopback-trusted` warns and requires owner consent; every local process can then use the listener, so token mode is preferred. Origin/Host checks still apply. No client plugin silently chooses trusted mode.

Modern2026-07-28 requests include per-request `_meta` and matching MCP headers. Legacy2025-11-25 uses initialize/initialized and SDK sessions. Do not impose modern stateless metadata on all legacy requests.

## Instruction plugin

The instruction ZIP contains one adapted existing fiverr-gig-optimizer skill, notices and manifests. Claude's .claude-plugin manifest, Codex fallback, Cursor fallback and Agent Plugins1.0 root manifest share one skills/ tree. Use the host's documented local/plugin install mechanism; no marketplace submission is performed by this project. Installation does not auto-start MCP, install hooks, enable networking or sign in.

## Upgrade/uninstall

Stop clients/listeners before upgrading. Preserve private state/browser profiles/exports and back them up privately. Replace only verified executable/public payload; never reset a schema-newer or corrupt store. `backup create <filename>` writes a private SQLite copy in exports/. `backup validate <filename>` checks bounded input (64 MiB), supported ordinary-table schema, database integrity and row bindings (at most 10,000 rows in each supported data table). Backup creation reports success only after the copy passes this recovery validation; oversized or incompatible copies are not reported as usable backups. `backup restore <filename>` requires interactive owner approval and an empty workspace; it never replaces existing records/history/journal. Restored routing remains blocked, pending approvals are cancelled, executing actions become outcome_unknown, and HTTP tokens/browser sessions are not restored. Preserve the backup privately; in-place rollback of a populated installation is deliberately refused. Each record retains its last 100 prior revisions. New record/import/action writes roll back if a data table exceeds 10,000 rows or aggregate record/history/action JSON exceeds 48 MiB; current records and action journals are never automatically deleted. Settings and read operations remain available for recovery. Unregister only the matching runtime entry if desired. Deleting an installation also deletes its private state; inspect it first and keep needed backups. Plugin uninstall does not remotely log out Fiverr.
