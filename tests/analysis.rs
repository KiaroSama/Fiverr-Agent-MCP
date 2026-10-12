use fiverr_agent_mcp::service::analysis::analyze;
use serde_json::{Value, json};

fn request(kind: &str, fields: Value) -> Value {
    let mut input = fields;
    input["kind"] = json!(kind);
    input["provenance"] = json!({
        "mode": "sample",
        "observed_at": "2026-10-12T00:00:00Z",
        "rights_note": "Synthetic test observations, not live Fiverr data"
    });
    input
}

#[test]
fn keyword_matches_reviewed_upstream_goldens() {
    for (count, expected, tier) in [
        (1, 100, "LOW"),
        (24, 82, "LOW"),
        (200, 70, "MEDIUM"),
        (2000, 40, "HIGH"),
        (10000, 12, "HIGH"),
        (20000, 0, "HIGH"),
    ] {
        let result = analyze(&request(
            "keyword",
            json!({
                "keyword": "ai chatbot n8n", "gig_count": count
            }),
        ))
        .expect("valid observed keyword");
        assert_eq!(result["data"]["competition_score"], json!(expected));
        assert_eq!(result["data"]["tier"], json!(tier));
        assert_eq!(result["data"]["opportunity_score"], json!(expected));
        assert_eq!(result["data"]["demand_score"], Value::Null);
        assert_eq!(result["data"]["flags"], json!(["demand_unavailable"]));
        assert_eq!(result["provenance"]["mode"], "sample");
    }
    let blended = analyze(&request(
        "keyword",
        json!({
            "keyword": "ai chatbot", "gig_count": 24,
            "top_gigs": [{"review_count": 500}, {"review_count": 500}, {"review_count": 500}]
        }),
    ))
    .expect("valid review observations");
    assert_eq!(blended["data"]["demand_score"], 100);
    assert_eq!(blended["data"]["opportunity_score"], 89);
}

#[test]
fn missing_count_is_not_an_observed_zero() {
    let missing = analyze(&request("keyword", json!({"keyword": "niche"})))
        .expect("missing data is represented explicitly");
    assert_eq!(missing["data"]["tier"], "UNKNOWN");
    assert_eq!(missing["data"]["gig_count"], Value::Null);
    let zero = analyze(&request(
        "keyword",
        json!({"keyword": "niche", "gig_count": 0}),
    ))
    .expect("zero observations");
    assert_eq!(zero["data"]["tier"], "UNTESTED");
    assert_eq!(zero["data"]["competition_score"], Value::Null);
    assert_eq!(
        zero["data"]["flags"],
        json!(["no_results", "untested_niche"])
    );
    for (count, tier) in [
        (199, "LOW"),
        (200, "MEDIUM"),
        (1999, "MEDIUM"),
        (2000, "HIGH"),
    ] {
        let result = analyze(&request(
            "keyword",
            json!({"keyword": "x", "gig_count": count}),
        ))
        .expect("tier boundary");
        assert_eq!(result["data"]["tier"], tier);
    }
}

#[test]
fn demand_rounding_is_ties_even_and_only_first_ten_rows_count() {
    let half = analyze(&request("keyword", json!({
        "keyword": "niche", "gig_count": 24, "top_gigs": [{"review_count": 2}, {"review_count": 3}]
    }))).expect("halfway demand");
    assert_eq!(half["data"]["demand_score"], 0);
    let mut rows = vec![json!({"review_count": 0}); 10];
    rows.push(json!({"review_count": 500}));
    let result = analyze(&request(
        "keyword",
        json!({
            "keyword": "niche", "gig_count": 24, "top_gigs": rows
        }),
    ))
    .expect("bounded top review sample");
    assert_eq!(result["data"]["demand_score"], 0);
    assert_eq!(result["data"]["opportunity_score"], 49);
}

#[test]
fn pricing_keeps_tier_samples_separate_and_money_exact() {
    let result = analyze(&request(
        "pricing",
        json!({
            "currency": "USD", "scale": 2, "experience": "L1",
            "prices_minor": {"basic": [1000, 2000, 3000], "standard": [4200], "premium": []}
        }),
    ))
    .expect("valid exact price observations");
    assert_eq!(
        result["data"]["recommended"],
        json!({
            "basic": {"minor": 1500, "currency": "USD", "scale": 2},
            "standard": {"minor": 4200, "currency": "USD", "scale": 2}, "premium": null
        })
    );
    assert_eq!(
        result["data"]["tiers"]["basic"]["median"],
        json!({
            "minor_numerator": 200000, "minor_denominator": 100, "currency": "USD", "scale": 2
        })
    );
    assert_eq!(result["data"]["tiers"]["basic"]["confidence"], "low");
    assert_eq!(
        result["data"]["flags"],
        json!([
            "basic_low_confidence",
            "standard_low_confidence",
            "premium_no_samples"
        ])
    );
    let fractional = analyze(&request(
        "pricing",
        json!({
            "currency": "EUR", "scale": 2, "prices_minor": {"basic": [1001, 1002]}
        }),
    ))
    .expect("exact fractional percentile");
    assert_eq!(
        fractional["data"]["tiers"]["basic"]["p25"]["minor_numerator"],
        100125
    );
    assert_eq!(fractional["data"]["recommended"]["basic"]["minor"], 1000);
}

#[test]
fn pricing_preserves_python_major_unit_rounding_and_confidence() {
    let ties = analyze(&request(
        "pricing",
        json!({
            "currency": "JPY", "scale": 0,
            "prices_minor": {"basic": [2, 4], "standard": [3, 4], "premium": [0]}
        }),
    ))
    .expect("bankers rounding");
    assert_eq!(ties["data"]["recommended"]["basic"]["minor"], 2);
    assert_eq!(ties["data"]["recommended"]["standard"]["minor"], 4);
    assert_eq!(ties["data"]["recommended"]["premium"]["minor"], 0);
    let samples = json!({"basic": [1000, 2000, 3000, 4000, 5000, 6000, 7000, 8000]});
    for (experience, expected) in [("New", 1700), ("L1", 2800)] {
        let result = analyze(&request(
            "pricing",
            json!({
                "currency": "USD", "scale": 2, "experience": experience, "prices_minor": samples
            }),
        ))
        .expect("sufficient observations");
        assert_eq!(result["data"]["tiers"]["basic"]["confidence"], "ok");
        assert_eq!(result["data"]["recommended"]["basic"]["minor"], expected);
    }
}

fn dataset_rows() -> Vec<Value> {
    vec![
        json!({"row_id": "a", "title": "AI chatbot n8n", "review_count": 5, "gig_count_in_search": 24}),
        json!({"row_id": "b", "tags": ["ai", "chatbot", "n8n"], "review_count": 9, "gig_count_in_search": 40}),
        json!({"row_id": "c", "subcategory": "AI chatbot n8n", "review_count": 9, "gig_count_in_search": 24}),
        json!({"row_id": "d", "title": "AI chatbot n8n", "review_count": 1, "gig_count_in_search": 40}),
        json!({"row_id": "e", "title": "AI chatbot n8n", "review_count": null, "gig_count_in_search": null}),
    ]
}

#[test]
fn dataset_uses_first_tied_mode_and_stable_review_order() {
    let result = analyze(&request(
        "dataset",
        json!({
            "keyword": "ai chatbot n8n", "rows": dataset_rows(), "top_n": 3
        }),
    ))
    .expect("five qualifying source rows");
    assert_eq!(result["data"]["gig_count"], 24);
    assert_eq!(result["data"]["match_confidence"], "HIGH");
    assert_eq!(result["data"]["matched_rows"], 5);
    assert_eq!(result["data"]["top_gigs"][0]["row_id"], "b");
    assert_eq!(result["data"]["top_gigs"][1]["row_id"], "c");
    assert_eq!(result["data"]["top_gigs"][2]["row_id"], "a");
    assert_eq!(result["data"]["source"], "sample_dataset");
    assert_eq!(result["data"]["flags"], json!([]));
}

#[test]
fn dataset_insufficient_rows_never_invent_a_count() {
    for (keyword, rows, expected_matches) in [
        ("ai chatbot n8n", vec![dataset_rows()[0].clone()], 1),
        ("underwater basket weaving zxqv", dataset_rows(), 0),
    ] {
        let result = analyze(&request(
            "dataset",
            json!({"keyword": keyword, "rows": rows}),
        ))
        .expect("low confidence response");
        assert_eq!(result["data"]["gig_count"], Value::Null);
        assert_eq!(result["data"]["matched_rows"], expected_matches);
        assert_eq!(result["data"]["flags"], json!(["no_match"]));
        assert_eq!(result["data"]["top_gigs"], json!([]));
    }
    let mut no_counts = dataset_rows();
    for row in &mut no_counts {
        row["gig_count_in_search"] = Value::Null;
    }
    let result = analyze(&request(
        "dataset",
        json!({"keyword": "ai chatbot n8n", "rows": no_counts}),
    ))
    .expect("counts absent from sufficient rows");
    assert_eq!(result["data"]["gig_count"], Value::Null);
}

#[test]
fn dataset_medium_confidence_and_imported_provenance_remain_distinct() {
    let rows = vec![
        json!({
            "title": "ai chatbot", "tags": ["ai", "chatbot"],
            "review_count": 0, "gig_count_in_search": 200
        });
        5
    ];
    let mut input = request(
        "dataset",
        json!({"keyword": "ai chatbot n8n", "rows": rows}),
    );
    input["provenance"]["mode"] = json!("imported");
    input["provenance"]["record_id"] = json!("market-observation-1");
    let result = analyze(&input).expect("medium-confidence supplied dataset");
    assert_eq!(result["data"]["match_confidence"], "MEDIUM");
    assert_eq!(result["data"]["source"], "supplied_dataset");
    assert_eq!(result["data"]["gig_count"], 200);
    assert_eq!(result["provenance"]["record_id"], "market-observation-1");
    assert_eq!(result["warnings"], json!([]));
}

#[test]
fn metrics_return_observed_ratios_or_explicit_missing_flags() {
    let result = analyze(&request(
        "metrics",
        json!({"impressions": 100, "clicks": 10, "orders": 2}),
    ))
    .expect("observed counters");
    assert_eq!(result["data"]["click_through_percent"], 10.0);
    assert_eq!(result["data"]["click_to_order_percent"], 20.0);
    let zero = analyze(&request(
        "metrics",
        json!({"impressions": 0, "clicks": 0, "orders": null}),
    ))
    .expect("zero denominators");
    assert_eq!(zero["data"]["click_through_percent"], Value::Null);
    assert_eq!(
        zero["data"]["flags"],
        json!([
            "click_through_zero_denominator",
            "click_to_order_zero_denominator"
        ])
    );
    let absent = analyze(&request("metrics", json!({}))).expect("absent observations");
    assert_eq!(
        absent["data"]["flags"],
        json!(["click_through_unavailable", "click_to_order_unavailable"])
    );
}

#[test]
fn malformed_or_unbounded_external_inputs_are_rejected_without_echoing_values() {
    let invalid = [
        request("unknown", json!({})),
        request("keyword", json!({"keyword": "x", "gig_count": -1})),
        request("keyword", json!({"keyword": "x", "gig_count": true})),
        request("keyword", json!({"keyword": "x", "gig_count": 5000001})),
        request(
            "keyword",
            json!({"keyword": "https://user:secret@example.invalid", "gig_count": 1}),
        ),
        request(
            "keyword",
            json!({"keyword": "x", "top_gigs": [{"review_count": -1}]}),
        ),
        request(
            "pricing",
            json!({"currency": "usd", "scale": 2, "prices_minor": {}}),
        ),
        request(
            "pricing",
            json!({"currency": "USD", "scale": 7, "prices_minor": {}}),
        ),
        request(
            "pricing",
            json!({"currency": "USD", "scale": 2, "prices_minor": {"basic": [10.5]}}),
        ),
        request(
            "pricing",
            json!({"currency": "USD", "scale": 2, "prices_minor": {"basic": [1000000000001_u64]}}),
        ),
        request(
            "pricing",
            json!({"currency": "USD", "scale": 2, "prices_minor": {"basic": [null]}}),
        ),
        request(
            "pricing",
            json!({"currency": "USD", "scale": 2, "experience": "typo", "prices_minor": {}}),
        ),
        request("dataset", json!({"keyword": "x", "rows": [], "top_n": 0})),
        request(
            "dataset",
            json!({"keyword": "x", "rows": [{"title": "x", "url": "https://user:secret@example.invalid"}]}),
        ),
        request(
            "metrics",
            json!({"impressions": 1, "earnings_estimate": 99}),
        ),
    ];
    for input in invalid {
        let error = analyze(&input).expect_err("invalid boundary input");
        assert!(!error.contains("secret"));
    }
    let mut input = request("metrics", json!({}));
    input["provenance"]["observed_at"] = json!("not-a-date");
    assert!(analyze(&input).is_err());
    input["provenance"]["observed_at"] = json!("2026-10-12T00:00:00Z");
    input["provenance"]["uri"] = json!("https://user:secret@example.invalid");
    assert!(analyze(&input).is_err());
    let mut no_provenance = request("metrics", json!({}));
    no_provenance
        .as_object_mut()
        .expect("request object")
        .remove("provenance");
    assert!(analyze(&no_provenance).is_err());
    let oversized = request(
        "dataset",
        json!({"keyword": "x", "rows": vec![json!({}); 2049]}),
    );
    assert!(analyze(&oversized).is_err());
}
