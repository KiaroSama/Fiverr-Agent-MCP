# Capability inventory

capabilities.json is the canonical workflow disposition inventory. Server discovery exposes only implemented tools, not every dedicated Fiverr workflow name. A mapping to fiverr_record_get/save/list or action_prepare is a local alternative, never an equivalent live implementation.

Dimensions are independent: implementation (code exists or not), verification (not_verified until execution; fixture/live separately), availability (local available versus unsupported/prerequisite blocked), effect (read/local/remote/financial-contractual), mode (native/manual/unsupported), reason, adapter, account scope, evidence, observation time, required approval, tool and tests. An observed empty local list is not evidence of an empty account collection. Current account/session tools expose sanitized mode, account observation, freshness and prerequisite status. Cookie presence is not authentication; positive selected-route verification may report an authenticated observation, while extension DOM observations always keep `live_authenticated:false`. Neither grants automation permission. No credential import/pairing tool is exposed to MCP; those remain optional low-level owner CLI compatibility actions, not launcher choices. Live Fiverr compatibility remains unverified.

Groups covered: account/settings; marketplace/categories/gigs/sellers/reviews/search; profile/portfolio/availability; seller Gig lifecycle/history; inbox/messages/templates/attachments; account Mira opportunities and distinct historical Briefs; offers; orders/requirements/delivery/revision/extensions/cancellation/hourly/resolution/support; analytics/earnings/transactions/invoices/statements/ads/subscriptions; buyer collections/projects/briefs/requirements/offers/checkout/reviews; local notes/tasks/drafts/import/export/snapshot/reminders/monitor; shared action/runtime tools.

The existing helper supplies deterministic keyword/pricing/supplied-dataset/observed-ratio analysis only. Pairwise offers_compare, opportunity_match and metrics_compare are not hidden aliases to that helper. Unsupported remote operations require actual platform permission/interface, account identity and route evidence; generic draft storage does not implement them. Financial/contractual activity remains a clearly incomplete human handoff.

No unsupported tool returns an empty successful platform result. Current dedicated live operations are not implemented; reasons distinguish that code gap from independent external permission/route blockers. Full account-backed scope remains incomplete.

## Local reminder pagination

`fiverr_reminders_list` accepts `limit` (1-100, default 100) and an opaque `cursor`.
Eligible local draft tasks are filtered before the result limit: completed, cancelled,
undated and future tasks cannot hide a later due task. Imported records and other
accounts remain excluded. Follow `pagination.next_cursor` until null; top-level
`partial` indicates another eligible page, not an unrecoverable first-100 prefix.
The cursor binds the selected account and a fixed due-time cutoff across pages.
Task records can still change between calls; restart without a cursor to refresh.
Dates are parsed as RFC3339 with offsets. This produces no platform notification
and does not turn a local inferred deadline into a verified Fiverr deadline.
