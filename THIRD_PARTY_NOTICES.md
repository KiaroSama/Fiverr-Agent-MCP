# Third-party notices

Original Fiverr Agent MCP code is Copyright (C) 2026 KiaroSama, licensed GPL-3.0-only. See LICENSE. This is unofficial software, not endorsed by Fiverr or the referenced projects. No warranty is provided.

## Existing Fiverr optimizer

Native deterministic helpers in src/service/analysis.rs adapt Ahad690/fiverr-gig-optimizer at cf4d6005bd53647a8f9f7ea564bb2a92176c1e08. Copyright (c) 2026 fiverr-gig-optimizer contributors; source MIT grant retained in third_party/fiverr-gig-optimizer/LICENSE.

The existing instruction/copy-guidance adaptation is attributed to that same project. Its LICENSE-DATA explicitly applies CC-BY-4.0 to Markdown/data despite MIT metadata labels. The separate CC-BY attribution, changes dated 2026-10-12, original LICENSE-DATA and complete official CC-BY-4.0 text are retained in third_party/fiverr-gig-optimizer/. See its NOTICE.txt, docs/skills.md and skills.lock.json. No contributor endorsement is implied. No market dataset is bundled.

## Linked dependencies

Cargo.lock pins executable, test and packaging dependencies. Native packages generate licenses/dependencies.json and distribute actual license texts from the resolved normal/build dependency graph. Missing license texts fail packaging; an SPDX label alone is not a substitute. Audits and compatibility checks run in CI.

The application uses the official modelcontextprotocol/rust-sdk (Apache-2.0), reqwest (MIT OR Apache-2.0), chromiumoxide (MIT OR Apache-2.0), Tokio (MIT), rusqlite (MIT), serde (MIT OR Apache-2.0), Axum (MIT) and their locked dependencies. SQLite is bundled by libsqlite3-sys and retains the applicable upstream notices. Final package inventories, not this illustrative list, identify resolved versions/licenses.

## References versus redistributed material

Owner Upwork/Agensi projects and the other Fiverr projects listed in references.lock.json informed route, native packaging, schema and access-boundary design. They are not bundled account backends. Scrapers, arbitrary JavaScript bridges, anti-detection code, unsupported APIs and secret-bearing configurations were not copied.

## Corresponding source

Each native release must distribute the exact source archive and build/configuration scripts beside its executable archives, with the same source SHA and checksums. Source can also be obtained at https://github.com/KiaroSama/Fiverr-Agent-MCP. Binary publication is blocked until corresponding source, applicable third-party terms and native verification are present.
