# Third-party notices

Original Fiverr Agent MCP code is Copyright (C) 2026 KiaroSama, licensed GPL-3.0-or-later. See LICENSE. This is unofficial software, not endorsed by Fiverr or the referenced projects. No warranty is provided.

## Existing Fiverr optimizer

Native deterministic helpers in src/service/analysis.rs adapt Ahad690/fiverr-gig-optimizer at cf4d6005bd53647a8f9f7ea564bb2a92176c1e08. Copyright (c) 2026 fiverr-gig-optimizer contributors; source MIT grant retained in third_party/fiverr-gig-optimizer/LICENSE.

The existing instruction/copy-guidance adaptation is attributed to that same project. Its LICENSE-DATA explicitly applies CC-BY-4.0 to Markdown/data despite MIT metadata labels. The separate CC-BY attribution, changes dated 2026-10-12, original LICENSE-DATA and complete official CC-BY-4.0 text are retained in third_party/fiverr-gig-optimizer/. See its NOTICE.txt, docs/skills.md and skills.lock.json. No contributor endorsement is implied. No market dataset is bundled.

## Linked dependencies

Cargo.lock pins executable, test and packaging dependencies. Native packages generate licenses/dependencies.json and distribute actual license texts from the resolved normal/build dependency graph. Missing license texts fail packaging; an SPDX label alone is not a substitute. Audits and compatibility checks run in CI.

The historical rmcp/rmcp-macros3.5.1 package lacked its workspace license text in the registry archive. The exact pinned upstream LICENSE, including the Apache-2.0/MIT transition statement, is retained in third_party/rust-sdk/ with a source/hash notice and included in native dependency notices. The fallback is restricted to those exact versions; other missing license texts still fail packaging.

The selectors0.38.0 and0.41.0 packages declare MPL-2.0 but omits the license text. The official unmodified Mozilla MPL2.0 text is retained in third_party/selectors/ with exact upstream source revision and attribution. Packaging accepts this fallback only for selectors0.38.0/0.41.0 and the pinned license hash; no selectors source was modified. Source remains available at the documented upstream revision/registry package.

The application uses the official modelcontextprotocol/rust-sdk (Apache-2.0 with retained upstream MIT transition notices), reqwest (MIT OR Apache-2.0), chromiumoxide (MIT OR Apache-2.0), Tokio (MIT), rusqlite (MIT), serde (MIT OR Apache-2.0), Axum (MIT) and their locked dependencies. SQLite is bundled by libsqlite3-sys and retains the applicable upstream notices. Final package inventories, not this illustrative list, identify resolved versions/licenses.

Release signature verification in `xtask` links `ssh-key` (Apache-2.0 OR MIT) at an exact pin, together with its locked `ed25519-dalek` and `curve25519-dalek` (BSD-3-Clause), `signature`, `ssh-encoding` and `sha2 0.10` (Apache-2.0 OR MIT) dependencies. These crates verify the SSHSIG archive signatures described in docs/verification-report.md; they are build/CI tooling and are not part of the shipped application binary. `ssh-key` pins `sha2 0.10` while `xtask` also pins `sha2 0.11` directly, so `cargo deny` reports a multiple-versions warning for that crate; both lines are on the allow list and no single crate mixes them.

`ssh-key` also declares `rsa` and, through `elliptic-curve`, `crypto-bigint` as optional dependencies. `xtask` selects `default-features = false, features = ["ed25519", "std"]`, so neither crate is in any resolved graph: `cargo tree -e all --target all` reports zero occurrences of `rsa`, `crypto-bigint` and `elliptic-curve`, and no binary, test or build script compiles their code. They are present in `Cargo.lock` only because Cargo records every optional dependency regardless of the enabled feature set. The RUSTSEC database lists timing side-channel advisories for both locked versions (`RUSTSEC-2023-0071` for `rsa 0.9.10`, `RUSTSEC-2026-0387` for `crypto-bigint 0.5.5`). Patched versions exist (`rsa >=0.10.0`, `crypto-bigint >=0.7.6`), but `ssh-key 0.6.7` requires the older lines, so the locked graph cannot reach a patch without leaving the stable pin; the next `ssh-key` release is a release candidate, which this release path avoids. Because `cargo-audit` scans the lockfile irrespective of features, both IDs are ignored in `.cargo/audit.toml` with this reasoning recorded beside them. `cargo-deny` resolves the real graph, detects no advisory for either crate, and therefore carries no ignore. The ignores cover only these two uncompiled, lockfile-present crates and only their timing advisories; they are removed and the audit re-run if the selected `ssh-key` features ever enable the `rsa` or `ecdsa` path.

## References versus redistributed material

Owner Upwork/Agensi projects and the other Fiverr projects listed in references.lock.json informed route, native packaging, schema and access-boundary design. They are not bundled account backends. Scrapers, arbitrary JavaScript bridges, anti-detection code, unsupported APIs and secret-bearing configurations were not copied.

## Test-only coverage-guided harness

The separate fuzz/Cargo.lock pins libfuzzer-sys0.4.13 and arbitrary1.5.0. libfuzzer-sys is licensed (MIT OR Apache-2.0) AND NCSA; its LLVM libFuzzer sources retain their NCSA terms. arbitrary is MIT OR Apache-2.0. These are CI-only test dependencies, not linked into shipped application/verifier binaries. The corresponding-source archive includes the harness and lockfile; upstream license texts remain in their registry packages and upstream repositories https://github.com/rust-fuzz/libfuzzer and https://github.com/rust-fuzz/arbitrary . The independent fuzz dependency policy explicitly covers NCSA without changing the production dependency policy.

## Corresponding source

Each native release must distribute the exact source archive and build/configuration scripts beside its executable archives, with the same source SHA and checksums. Source can also be obtained at https://github.com/KiaroSama/Fiverr-Agent-MCP. Binary publication is blocked until corresponding source, applicable third-party terms and native verification are present.
