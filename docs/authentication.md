# Authentication and account scope

## Current state

Live authentication is not implemented/verified. Account/session tools report prerequisites and `live_authenticated:false`; they must not be interpreted as a successful login. `auth sign-in` refuses blocked routing, then missing browser, then unverified strict browser assurance. It does not launch an uncontrolled browser or collect passwords/cookies. `fiverr_auth_start` provides a clearly incomplete human handoff.

The owner confirmed an account-backed product with manual sign-in and narrow typed operations. Local functionality alone does not fulfill that requirement. Live readiness remains blocked on permitted current platform interfaces, verified routing/containment and an owner session; this limitation is not waived by a fixture test.

## Launcher Sign in

The main menu and `run.ps1 -SignIn` expose the same native `auth sign-in` path. The owner requested the saved, verified integration-session behavior of Agensi/Upwork, not merely a website link. Those reference services have distinct remote MCP/OAuth contracts; no equivalent first-party Fiverr contract was verified in the current research. Copying their callback/token code or changing their hostname would not produce valid Fiverr authentication. The entry therefore remains visibly gated and never claims a connected account. This requested integration is unfinished.

## Human workflow

Open the official Fiverr site in an owner-controlled browser through the intended route. Enter credentials, 2FA and challenges yourself. Verify the intended account and role before acting. Do not expose credential fields, sessions, tokens, CSRF data or browser debugging endpoints to an assistant. Identity verification may require a separate official manual process; the server must not silently bypass proxy policy.

## Dedicated infrastructure

Configured browser executable must be an existing absolute file. Profiles are installation-owned, not personal/system profiles. ProfileLease uses an OS file lock: concurrent ownership fails, process release unlocks it, stale-looking files are not blindly deleted. Typed AccountObservation decoding checks account ID, roles, observation time and source, refuses signed-out/expired/challenge/changed schemas, and never switches account automatically.

Controlled browser fixture extraction uses a fixed-purpose JSON marker, local loopback page and literal internal script. It is not a current Fiverr selector, permission grant, or production endpoint override. No arbitrary JS/CDP/cookie extraction is exposed through MCP.

## Logout distinctions

Remote logout is a manual official-site handoff. Local profile removal is separate and currently refused rather than recursively deleting user files. Drafts/imports are not authentication state. Do not erase drafts or unrelated profiles to fix an expired session.

## Remaining acceptance

Real managed manual login, safe account read, expiration/challenge behavior, role/entitlement evidence and strict browser network enforcement still require implementation and approved live validation. No real message, delivery or financial transaction is sent solely to make tests green.
