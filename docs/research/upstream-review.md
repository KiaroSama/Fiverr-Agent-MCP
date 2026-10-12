# Upstream review

Reviewed 2026-10-11/12. Exact reviewed commits are in references.lock.json; these sources were read, not executed against live accounts.

| Reference | Reviewed revision | Evidence read | Concepts used / rejected |
|---|---|---|---|
| KiaroSama/Upwork-Agent-Plugin | 00256cd7976f7639ab30c894ee3665de959a41e5 | Cargo manifest, route.rs, http_route.rs, config.rs, README, native CI | Explicit selected profile, no implicit proxies, route fencing, native binaries. Reject hosted Upwork endpoints and OAuth assumptions. |
| KiaroSama/Agensi-MCP | d431e70d66ed29135dc09d2320b6cc0b0c016ab6 | Cargo, local_http.rs, network.rs, network/local_http tests | Bounded local listener/stream, settings, proxy schemes. Reject unauthenticated default trust and inherited Agensi backend. |
| KyuRish/fiverr-mcp-server | 0fee60669607a43c03a0137e7fa78af059d5cd21 | tools, scraper, models, manifest, publish workflow, MIT license | Public data vocabulary; not official account API. Reject impersonation/challenge retries and zero-as-missing defaults. |
| marwankous/fiverr-mcp | cad7b0f33a54de626c1b5410b1ec34a013ee1f69 | server/schemas, extension, native bridge/client, manifest/license | Account/browser boundary inventory. Reject arbitrary JS, uncredentialed bridge and implicit DOM mutations. |
| eaglecheow/mcp-fiverr | a4f6020751a02e600fe700cba07c9a1b328783e0 | client/config/manifest/synthetic tests | Historical Briefs analysis only. Reject guessed routes/cookie export; no LICENSE grant file found, so no source incorporation. |
| Ahad690/fiverr-gig-optimizer | cf4d6005bd53647a8f9f7ea564bb2a92176c1e08 | skill, score/pricing/query helpers/config/tests, licenses | Selected existing skill and deterministic helpers. MIT code/CC-BY-4.0 Markdown/data; conflicting MIT frontmatter recorded. Exclude scraper, contribution/upload, unverified sample metrics. |
| waseemnasir2k26/fiverr-gig-optimizer | cd1bafc239062338f49310c18f1a5febf74c3752 | command/catalog/config/license | Presentation reference only. No duplicate skill; reject mental estimates and unsupported ranking/CTR assertions. |
| modelcontextprotocol/rust-sdk | 79437f291b2c44053d00dcd5db969fd0cca7c887 | published3.5.1 manifests/examples, discovery/stateless tests, docs/issues | Official transport/handler/model contracts; modern metadata and legacy initialize both. Tag object bc82eb8 points to this commit. |
| seanmonstar/reqwest | v0.13.5 | proxy/connect source and official API | Explicit Proxy::all/no_proxy, SOCKS4a/5h remote destination DNS, verified TLS. Wire tests required. |
| mattsse/chromiumoxide | v0.9.1 | launch/config/source example | Rust CDP lifecycle with explicit root/env/profile/sandbox. Reject hide/anti-detection, unmanaged/default profile and flag-only strict containment claims. |
| nextest-rs/nextest | cargo-nextest-0.9.148 | official runner docs/release | Bounded suites/JUnit/fail-on-zero discovery; no assumed execution from compilation. |

Original code uses GPL-3.0-only. Concepts are reimplemented where possible; any substantial adapted third-party material retains its own license/notice. No owner account/config/secrets or private working notes are incorporated.

## Other maintained usage

Jan (MIT) and Nushell (MIT) StreamableHttpService implementations were inspected at pinned source revisions. Their local-token/transport ideas inform the listener; unbounded spawning and permissive Origin defaults are rejected. Official SDK tests remain the API authority.

## Limits

Source review is not live readiness. Auxiliary tools, third-party DOM tests and wider partner arrangements were not fully audited; excluded material is not a supported capability. Official terms and account entitlement govern activation, not scraper license or GitHub popularity.
