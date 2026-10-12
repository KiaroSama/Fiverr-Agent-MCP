# Existing Fiverr optimizer adaptation

Use the packaged `fiverr-gig-optimizer` skill with `fiverr_market_analyze`.
It is an adaptation of Ahad690's existing skill, not a new Fiverr business skill.
It requires no Python, Node, shell command, scraper, uploaded dataset or API key.
Loading the instruction package does not install/register a runtime, enable
networking, sign in, create a hook, approve an action or publish a Gig.

## Upstream and license boundaries

Pinned upstream: [Ahad690/fiverr-gig-optimizer](https://github.com/Ahad690/fiverr-gig-optimizer/tree/cf4d6005bd53647a8f9f7ea564bb2a92176c1e08),
revision `cf4d6005bd53647a8f9f7ea564bb2a92176c1e08`.

- Original skill identity: `fiverr-gig-optimizer`, metadata1.1/plugin1.1.0.
  [Skill](https://github.com/Ahad690/fiverr-gig-optimizer/blob/cf4d6005bd53647a8f9f7ea564bb2a92176c1e08/skills/fiverr-gig-optimizer/SKILL.md)
  and [plugin](https://github.com/Ahad690/fiverr-gig-optimizer/blob/cf4d6005bd53647a8f9f7ea564bb2a92176c1e08/.claude-plugin/plugin.json).
- Source code: MIT, copyright2026 fiverr-gig-optimizer contributors. Native
  scoring/pricing/lookup adapts that source while new project code is GPL-3.0-only;
  original copyright and MIT grant remain with the adaptation.
- Documentation/data: upstream LICENSE-DATA expressly covers all Markdown/data
  as CC-BY-4.0 despite MIT skill/plugin labels. This package conservatively
  preserves CC-BY-4.0 attribution/change notices for the adapted instruction,
  copy guidance and related explanation; it does not relabel them GPL-only.
- Exact original `LICENSE`, original `LICENSE-DATA`, and official unmodified
  CC-BY-4.0 legal text are in `third_party/fiverr-gig-optimizer/`.
  [Official license](https://creativecommons.org/licenses/by/4.0/legalcode.txt).
  No endorsement by Ahad690, contributors or Fiverr is implied.

Changes dated2026-10-12: Python invocations replaced by typed native analysis;
local authorized observations/imports replace fetched profiles/scrapes; exact
minor money/provenance/input ceilings added; missing values remain unavailable;
upstream source/comment contradictions disclosed; contribution/HuggingFace token
steps, automatic dataset refresh, external actors, scheduled automerge, PDF/
canvas rendering, AI-image execution and unrelated assets removed. Offer-copy
and gather/candidate/score/price/draft/review sequence remain adapted upstream.

## Native public interface

```rust
pub fn analyze(input: &serde_json::Value) -> Result<serde_json::Value, String>
```

Runtime tool: `fiverr_market_analyze`. Input kinds: keyword/pricing/dataset/metrics.
Return envelope: `{data,warnings,provenance}`. No networking/file/state side effects.
Inputs <=2,048samples/rows, bounded text, integer counts and explicit provenance.
Unknown fields are rejected rather than echoed into output. Provenance allows
manual/imported/sample, RFC3339observed_at, plain rights_note and optional safe
record_id; it accepts no credential-bearing sourceURI. A rights statement isn't
independent verification. Full compact request contract lives in packaged SKILL.md.

### Keyword illustration

```json
{
  "kind": "keyword", "keyword": "ai chatbot n8n", "gig_count": 24,
  "top_gigs": [{"review_count": 500}],
  "provenance": {"mode":"sample","observed_at":"2026-10-12T00:00:00Z","rights_note":"Synthetic demonstration, not live Fiverr data"}
}
```

Expected synthetic data: competition82, LOW, demand100, opportunity89. Output
warns that sample observations are not current measurements. Missing count yields
UNKNOWN/null; observed0yieldsUNTESTED/null with no_results/untested_niche flags.

### Exact pricing illustration

```json
{
  "kind": "pricing", "currency": "USD", "scale": 2, "experience": "L1",
  "prices_minor": {"basic":[1000,2000,3000],"standard":[4200],"premium":[]},
  "provenance": {"mode":"sample","observed_at":"2026-10-12T00:00:00Z","rights_note":"Synthetic demonstration, not live Fiverr data"}
}
```

Recommendations:1500USDminor,4200USDminor,nullpremium; low-confidence/no-sample
flags remain visible. Percentile spreads retain exact fraction
`{minor_numerator:150000,minor_denominator:100,currency:"USD",scale:2}` forp25Basic
(1500minor=$15.00). Denominator100 is a rational divisor, distinct from scale.
Recommendations preserve upstream whole-major-unit ties-even rounding. A
percentile$12.50 rounds to$12.00, not$12.50; money remains integer/rational, never
float finance. Scale0..6is explicit; no ISO currency scale inference/FX conversion.

## Semantic parity and intentional validation changes

[Scoring source](https://github.com/Ahad690/fiverr-gig-optimizer/blob/cf4d6005bd53647a8f9f7ea564bb2a92176c1e08/skills/fiverr-gig-optimizer/scripts/score_keyword.py):
log10 interpolation anchors1/100,200/70,2000/40,20000/0; thresholds200/2000;
first10review rows, median/500clamped demand; opportunity60/40blend; combo bonusoff.
Ties-to-even uses Rust's standard method (stable1.77). Scores are heuristics,
not Fiverr ranking/demand measurements or guaranteed outcomes. log10precision is
platform-dependent at extreme near-rounding boundaries; native CI checks literal
goldens on each target, not universal bit-for-bit libm equivalence.

[Pricing source](https://github.com/Ahad690/fiverr-gig-optimizer/blob/cf4d6005bd53647a8f9f7ea564bb2a92176c1e08/skills/fiverr-gig-optimizer/scripts/analyze_pricing.py):
independent per-tier inclusive percentiles, n0null, n1..7lowconfidence, n>=8ok;
Newp10/p25/p50, defaultp25/p50/p75. Exact integer interpolation replaces floating
money while retaining whole-major recommendation rounding. Nonnegative integers,
valid experience enum and explicit currency/scale are stricter than upstream.

[Dataset source](https://github.com/Ahad690/fiverr-gig-optimizer/blob/cf4d6005bd53647a8f9f7ea564bb2a92176c1e08/skills/fiverr-gig-optimizer/scripts/query_dataset.py):
ASCII-token matching/tagJaccard/titleoverlap/subcategorysubstring; threshold0.5,
minimum5rows, best0.8HIGH otherwiseMEDIUM; review-stable sorting. Actual Python
>=3.8first-tied-mode behavior retained rather than misleading median fallback
comment. Caller-supplied canonical subset avoids raw URL/PII fields; other fields
are rejected. No sample dataset shipped or live retrieval claimed.

Upstream prose/sample1243count examples contain inconsistent scores/LOWtiers;
this adaptation uses source algorithms/tests, not those prose values. Source
assertions24->82/10000->12 and zero/tier/null rules are literal regression
expectations. Dataset row/count bounds reject negative/fractional/bool inputs
instead of permissive numeric coercion. `top_n:0` is rejected, not silently defaulted.

Metrics are ordinary observed click/impression and order/click ratios. Missing
observations/zero denominator->null/flags; no predictions, earnings estimates or
unsupported business analytics. Ratios may exceed100for incompatible source
cohorts; inputs are not silently altered to manufacture a plausible number.

## Asset/helper completeness and local workflow

Only one shipped skill tree, with its complete instruction/request/copy contract
in SKILL.md. No retained pointer invokes a missing Python helper or references
absent playbook/categories/sample/render template. Source-script dependencies
are replaced by the single native seam; removed optional features are named above.
`skills.lock.json` pins upstream paths/blobs and local skill/license digests.

Use discovered `fiverr_import_validate`/`fiverr_import` for owner-approved local
saving under selected account and `user_import` provenance. Read observations
with `fiverr_record_get`/`fiverr_records_list`, map needed narrow fields, preserve
time/rights/local record_id, then analyze. Use discovered local draft/export
workflow only when requested. Import/draft/export is not sending or publishing.
Fiverr account permission, manual authentication and human-approved account
operations remain separate gates; the skill cannot approve itself.

## Verification boundary

`tests/analysis.rs` tests only the public native interface with literal upstream
and independently specified expected values, missing/error/bounds/ties cases.
No subprocess, browser, network, sleeps, mutable fixture, or large dataset setup.
Run under the project's bounded native CI test bucket; local Rust compilation/
tests were explicitly not performed during this adaptation. Native CI and client
loading remain completion evidence to be supplied by the integrator; documentation
or an existing upstream test file is not an executed-green claim.
