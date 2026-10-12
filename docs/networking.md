# Routing and proxy controls

First launch is blocked. `proxy list` exposes sanitized profiles only. Import UTF-8 text using label|URL, URL lines or supported legacy host:port:user:password lines; BOM/comments/blank lines are accepted. Parse the entire bounded file before persisting. Imports do not select a route. Credential values are supplied in a root-contained private file, not command arguments.

Supported schemes: HTTP, HTTPS, SOCKS5 and SOCKS5h. SOCKS5 is normalized to SOCKS5h for remote destination DNS. SOCKS4/4a are explicitly unsupported. A named proxy host can still require local bootstrap DNS; this is distinct from destination DNS and does not guarantee resolver geography. IPv6 endpoints require URL form. No credential-bearing URI is displayed/logged.

Select a stable profile ID explicitly. Editing/deleting/selecting relevant profiles bumps route generation. Removing an active profile blocks. `local-proxy settings --file` configures an endpoint; `local-proxy select` explicitly selects it. Default local endpoint1595 is configuration only, not an installed proxy service. `direct` requires interactive owner consent; MCP/plugins cannot enable it. `block` changes the persistent generation; native pending reads cooperatively cancel, not an OS-wide socket cutoff.

RouteClient disables environment/system proxy selection and redirects, creates only the explicit route, keeps bounded connect/read/total deadlines and body limits, validates exact approved HTTPS Fiverr authorities and generation before/after a read, and never retries through another profile/direct route. Local wire fixtures observe a SOCKS domain handshake and assert a dead proxy cannot reach a direct loopback origin. These tests are not proof against every OS DNS/browser behavior.

`proxy test` currently validates configuration only: it does not prove reachability or resolve public destinations. External diagnostic probing is not silently performed. HTTP CONNECT/TLS/SOCKS credential failures, rate limiting and changed pages need separate evidence. TLS verification is not disabled.

Browser routing is a separate assurance boundary. Launch flags/interception alone cannot prevent every DNS, WebRTC, extension/service-worker or background connection. Production browser automation/sign-in therefore stays blocked until process-wide route controls and tests exist; no fallback to direct or fingerprint/anti-detection workaround.
