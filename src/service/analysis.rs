// SPDX-License-Identifier: GPL-3.0-only
// Deterministic helpers adapted from Ahad690/fiverr-gig-optimizer, revision
// cf4d6005bd53647a8f9f7ea564bb2a92176c1e08 (MIT source).
// Copyright (c) 2026 fiverr-gig-optimizer contributors.
// Original license and documentation attribution: third_party/fiverr-gig-optimizer/.

use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

const MAX_ROWS: usize = 2048;
const MAX_COUNT: u64 = 5_000_000;
const MAX_REVIEWS: u64 = 1_000_000;
const MAX_MINOR: u64 = 1_000_000_000_000;
const TIERS: [&str; 3] = ["basic", "standard", "premium"];

/// Analyze supplied observations without networking, file access, or state mutation.
/// The output preserves provenance and distinguishes missing data from observed zero.
pub fn analyze(input: &Value) -> Result<Value, String> {
    let input = object(input, "analysis")?;
    let kind = text(input.get("kind"), "kind", 16)?;
    let provenance = provenance(input.get("provenance"))?;
    let allowed: &[&str] = match kind {
        "keyword" => &["kind", "provenance", "keyword", "gig_count", "top_gigs"],
        "pricing" => &[
            "kind",
            "provenance",
            "currency",
            "scale",
            "experience",
            "prices_minor",
        ],
        "dataset" => &["kind", "provenance", "keyword", "rows", "top_n"],
        "metrics" => &["kind", "provenance", "impressions", "clicks", "orders"],
        _ => return Err("kind must be keyword, pricing, dataset, or metrics".into()),
    };
    keys(input, allowed, "analysis")?;
    let data = match kind {
        "keyword" => keyword(input)?,
        "pricing" => pricing(input)?,
        "dataset" => dataset(input, provenance["mode"].as_str().unwrap_or("imported"))?,
        "metrics" => metrics(input)?,
        _ => return Err("Unsupported analysis kind".into()),
    };
    let warnings = if provenance["mode"] == "sample" {
        vec!["Sample observations are not current Fiverr market measurements"]
    } else {
        Vec::new()
    };
    Ok(json!({"data": data, "warnings": warnings, "provenance": provenance}))
}

fn object<'a>(value: &'a Value, label: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{label} must be an object"))
}

fn keys(value: &Map<String, Value>, allowed: &[&str], label: &str) -> Result<(), String> {
    if value.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(format!("{label} contains an unsupported field"));
    }
    Ok(())
}

fn text<'a>(value: Option<&'a Value>, label: &str, max: usize) -> Result<&'a str, String> {
    let value = value
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{label} must be a string"))?;
    if value.trim().is_empty()
        || value.len() > max
        || value.contains("://")
        || value.chars().any(char::is_control)
    {
        return Err(format!(
            "{label} must contain 1..{max} bytes without URLs or control characters"
        ));
    }
    Ok(value)
}

fn unsigned(value: &Value, label: &str, max: u64) -> Result<u64, String> {
    value
        .as_u64()
        .filter(|number| *number <= max)
        .ok_or_else(|| format!("{label} must be an integer in 0..={max}"))
}

fn optional_count(value: Option<&Value>, label: &str, max: u64) -> Result<Option<u64>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => unsigned(value, label, max).map(Some),
    }
}

fn list<'a>(value: Option<&'a Value>, label: &str, max: usize) -> Result<&'a [Value], String> {
    let values = value
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{label} must be an array"))?;
    if values.len() > max {
        return Err(format!("{label} exceeds {max} entries"));
    }
    Ok(values)
}

fn provenance(value: Option<&Value>) -> Result<Value, String> {
    let value = value.ok_or("provenance is required")?;
    let fields = object(value, "provenance")?;
    keys(
        fields,
        &["mode", "observed_at", "rights_note", "record_id"],
        "provenance",
    )?;
    let mode = text(fields.get("mode"), "provenance.mode", 16)?;
    if !["manual", "imported", "sample"].contains(&mode) {
        return Err("provenance.mode must be manual, imported, or sample".into());
    }
    let date = text(fields.get("observed_at"), "provenance.observed_at", 40)?;
    chrono::DateTime::parse_from_rfc3339(date)
        .map_err(|_| "provenance.observed_at must be RFC3339 with an offset".to_string())?;
    text(fields.get("rights_note"), "provenance.rights_note", 256)?;
    if let Some(id) = fields.get("record_id") {
        let id = text(Some(id), "provenance.record_id", 96)?;
        if !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        {
            return Err("provenance.record_id must be a safe local identifier".into());
        }
    }
    Ok(value.clone())
}

fn keyword(input: &Map<String, Value>) -> Result<Value, String> {
    let keyword = text(input.get("keyword"), "keyword", 256)?;
    let count = optional_count(input.get("gig_count"), "gig_count", MAX_COUNT)?;
    let reviews = match input.get("top_gigs") {
        None => Vec::new(),
        Some(value) => {
            let rows = list(Some(value), "top_gigs", MAX_ROWS)?;
            let mut reviews = Vec::new();
            for (index, row) in rows.iter().enumerate() {
                let row = object(row, "top_gigs row")?;
                keys(row, &["review_count"], "top_gigs row")?;
                if let Some(review) =
                    optional_count(row.get("review_count"), "review_count", MAX_REVIEWS)?
                {
                    if index < 10 {
                        reviews.push(review as f64);
                    }
                }
            }
            reviews
        }
    };
    let Some(count) = count else {
        return Ok(json!({"keyword": keyword, "gig_count": null,
            "competition_score": null, "tier": "UNKNOWN", "demand_score": null,
            "opportunity_score": null, "flags": ["count_unavailable", "demand_unavailable"]}));
    };
    if count == 0 {
        return Ok(json!({"keyword": keyword, "gig_count": 0,
            "competition_score": null, "tier": "UNTESTED", "demand_score": null,
            "opportunity_score": null, "flags": ["no_results", "untested_niche"]}));
    }
    let competition = competition(count);
    let tier = if count < 200 {
        "LOW"
    } else if count < 2000 {
        "MEDIUM"
    } else {
        "HIGH"
    };
    let demand = if reviews.is_empty() {
        None
    } else {
        Some(
            (median(reviews) / 500.0 * 100.0)
                .clamp(0.0, 100.0)
                .round_ties_even() as u64,
        )
    };
    let opportunity = demand.map_or(competition, |demand| {
        (0.6 * competition as f64 + 0.4 * demand as f64).round_ties_even() as u64
    });
    let flags = if demand.is_none() {
        vec!["demand_unavailable"]
    } else {
        Vec::new()
    };
    Ok(
        json!({"keyword": keyword, "gig_count": count, "competition_score": competition,
        "tier": tier, "demand_score": demand, "opportunity_score": opportunity, "flags": flags}),
    )
}

fn competition(count: u64) -> u64 {
    let anchors: [(f64, f64); 4] = [(1.0, 100.0), (200.0, 70.0), (2000.0, 40.0), (20000.0, 0.0)];
    let x = (count as f64).log10();
    for pair in anchors.windows(2) {
        let (x0, score0) = (pair[0].0.log10(), pair[0].1);
        let (x1, score1) = (pair[1].0.log10(), pair[1].1);
        if x <= x1 {
            return (score0 + (x - x0) / (x1 - x0) * (score1 - score0))
                .round_ties_even()
                .clamp(0.0, 100.0) as u64;
        }
    }
    0
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    }
}

fn pricing(input: &Map<String, Value>) -> Result<Value, String> {
    let currency = text(input.get("currency"), "currency", 3)?;
    if currency.len() != 3 || !currency.bytes().all(|byte| byte.is_ascii_uppercase()) {
        return Err("currency must be three uppercase ASCII letters".into());
    }
    let scale = unsigned(input.get("scale").ok_or("scale is required")?, "scale", 6)?;
    let experience = match input.get("experience") {
        None => "L1",
        Some(value) => text(Some(value), "experience", 16)?,
    };
    if !["New", "L1", "L2", "Top Rated"].contains(&experience) {
        return Err("experience must be New, L1, L2, or Top Rated".into());
    }
    let pools = object(
        input
            .get("prices_minor")
            .ok_or("prices_minor is required")?,
        "prices_minor",
    )?;
    keys(pools, &TIERS, "prices_minor")?;
    let mut tiers = Map::new();
    let mut recommended = Map::new();
    let mut flags = Vec::new();
    let percentiles = if experience == "New" {
        [10, 25, 50]
    } else {
        [25, 50, 75]
    };
    let factor = 10_i128.pow(scale as u32);
    for (index, tier) in TIERS.iter().enumerate() {
        let mut values = Vec::new();
        if let Some(value) = pools.get(*tier) {
            for value in list(Some(value), "price samples", MAX_ROWS)? {
                values.push(unsigned(value, "price minor", MAX_MINOR)?);
            }
        }
        values.sort_unstable();
        let n = values.len();
        if n == 0 {
            tiers.insert(
                (*tier).into(),
                json!({"n": 0, "confidence": "low", "reason": "insufficient samples (n=0)"}),
            );
            recommended.insert((*tier).into(), Value::Null);
            flags.push(format!("{tier}_no_samples"));
            continue;
        }
        let mut block = json!({"n": n, "confidence": if n >= 8 { "ok" } else { "low" }});
        for (name, percentile) in [
            ("p10", 10),
            ("p25", 25),
            ("median", 50),
            ("p75", 75),
            ("p90", 90),
        ] {
            block[name] = json!({"minor_numerator": percentile_numerator(&values, percentile),
                "minor_denominator": 100, "currency": currency, "scale": scale});
        }
        if n < 8 {
            block["reason"] = json!(format!("insufficient samples (n={n})"));
            flags.push(format!("{tier}_low_confidence"));
        }
        // Round the exact percentile in MAJOR units, as upstream Python round does.
        let numerator = i128::from(percentile_numerator(&values, percentiles[index]));
        let denominator = 100 * factor;
        let whole = numerator / denominator;
        let remainder = numerator % denominator;
        let rounded = whole
            + i128::from(
                remainder * 2 > denominator || (remainder * 2 == denominator && whole % 2 == 1),
            );
        let minor = i64::try_from(rounded * factor)
            .map_err(|_| "Recommended price exceeds supported integer money range".to_string())?;
        recommended.insert(
            (*tier).into(),
            json!({"minor": minor, "currency": currency, "scale": scale}),
        );
        tiers.insert((*tier).into(), block);
    }
    Ok(
        json!({"experience": experience, "strategy": if experience == "New" { "new_seller" } else { "default" },
        "tiers": tiers, "recommended": recommended, "flags": flags}),
    )
}

fn percentile_numerator(values: &[u64], percentile: usize) -> u64 {
    let position = (values.len() - 1) * percentile;
    let index = position / 100;
    let fraction = (position % 100) as u64;
    let next = values[(index + 1).min(values.len() - 1)];
    values[index] * (100 - fraction) + next * fraction
}

fn tokens(value: &str) -> BTreeSet<String> {
    value
        .to_lowercase()
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect()
}

fn dataset(input: &Map<String, Value>, mode: &str) -> Result<Value, String> {
    let keyword = text(input.get("keyword"), "keyword", 256)?;
    let keyword_tokens = tokens(keyword);
    if keyword_tokens.is_empty() {
        return Err(
            "keyword needs at least one ASCII alphanumeric token for dataset matching".into(),
        );
    }
    let keyword_lower = keyword.to_lowercase().trim().to_string();
    let rows = list(input.get("rows"), "rows", MAX_ROWS)?;
    let top_n = match input.get("top_n") {
        None => 10,
        Some(value) => {
            let value = unsigned(value, "top_n", 100)?;
            if value == 0 {
                return Err("top_n must be in 1..=100".into());
            }
            value as usize
        }
    };
    let mut matches = Vec::new();
    let mut counts = Vec::new();
    let mut best: f64 = 0.0;
    for row in rows {
        let row_fields = object(row, "dataset row")?;
        keys(
            row_fields,
            &[
                "row_id",
                "title",
                "subcategory",
                "tags",
                "review_count",
                "gig_count_in_search",
            ],
            "dataset row",
        )?;
        for (field, bound) in [("title", 512), ("subcategory", 256), ("row_id", 96)] {
            if let Some(value) = row_fields.get(field) {
                if !value.is_null() {
                    let value = text(Some(value), field, bound)?;
                    if field == "row_id"
                        && !value
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
                    {
                        return Err("row_id must be a safe identifier".into());
                    }
                }
            }
        }
        let mut tags = BTreeSet::new();
        if let Some(value) = row_fields.get("tags") {
            if !value.is_null() {
                for tag in list(Some(value), "tags", 20)? {
                    tags.extend(tokens(text(Some(tag), "tag", 96)?));
                }
            }
        }
        let review = optional_count(row_fields.get("review_count"), "review_count", MAX_REVIEWS)?;
        let count = optional_count(
            row_fields.get("gig_count_in_search"),
            "gig_count_in_search",
            MAX_COUNT,
        )?;
        let title = tokens(
            row_fields
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or(""),
        );
        let subcategory = row_fields
            .get("subcategory")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_lowercase();
        let tag_score = if tags.is_empty() {
            0.0
        } else {
            keyword_tokens.intersection(&tags).count() as f64
                / keyword_tokens.union(&tags).count() as f64
        };
        let title_score =
            keyword_tokens.intersection(&title).count() as f64 / keyword_tokens.len() as f64;
        let subcategory_score: f64 = if subcategory.contains(&keyword_lower) {
            1.0
        } else {
            0.0
        };
        let score = tag_score.max(title_score).max(subcategory_score);
        if score >= 0.5 {
            best = best.max(score);
            if let Some(count) = count {
                counts.push(count);
            }
            matches.push((review.unwrap_or(0), row));
        }
    }
    let source = if mode == "sample" {
        "sample_dataset"
    } else {
        "supplied_dataset"
    };
    let matched_rows = matches.len();
    if matched_rows < 5 || counts.is_empty() {
        return Ok(
            json!({"keyword": keyword, "gig_count": null, "match_confidence": "LOW",
            "matched_rows": matched_rows, "top_gigs": [], "source": source, "flags": ["no_match"]}),
        );
    }
    let mut frequencies = BTreeMap::new();
    for count in &counts {
        *frequencies.entry(*count).or_insert(0_usize) += 1;
    }
    let highest = frequencies.values().copied().max().unwrap_or(0);
    // Python >=3.8 statistics.mode selects the FIRST observed tied mode, not median.
    let count = counts
        .iter()
        .find(|count| frequencies.get(*count) == Some(&highest))
        .copied();
    matches.sort_by(|left, right| right.0.cmp(&left.0));
    let top: Vec<&Value> = matches
        .into_iter()
        .take(top_n)
        .map(|(_, row)| row)
        .collect();
    Ok(json!({"keyword": keyword, "gig_count": count,
        "match_confidence": if best >= 0.8 { "HIGH" } else { "MEDIUM" },
        "matched_rows": matched_rows, "top_gigs": top, "source": source, "flags": []}))
}

fn metrics(input: &Map<String, Value>) -> Result<Value, String> {
    let impressions = optional_count(input.get("impressions"), "impressions", MAX_COUNT)?;
    let clicks = optional_count(input.get("clicks"), "clicks", MAX_COUNT)?;
    let orders = optional_count(input.get("orders"), "orders", MAX_COUNT)?;
    let mut flags = Vec::new();
    let mut ratio = |numerator: Option<u64>, denominator: Option<u64>, name: &str| match (
        numerator,
        denominator,
    ) {
        (_, Some(0)) => {
            flags.push(format!("{name}_zero_denominator"));
            None
        }
        (Some(n), Some(d)) => Some(n as f64 / d as f64 * 100.0),
        _ => {
            flags.push(format!("{name}_unavailable"));
            None
        }
    };
    let ctr = ratio(clicks, impressions, "click_through");
    let conversion = ratio(orders, clicks, "click_to_order");
    Ok(
        json!({"impressions": impressions, "clicks": clicks, "orders": orders,
        "click_through_percent": ctr, "click_to_order_percent": conversion,
        "flags": flags, "interpretation": "Ratios of supplied observations, not predictions"}),
    )
}
