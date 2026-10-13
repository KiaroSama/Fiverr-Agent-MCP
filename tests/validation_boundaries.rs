use fiverr_agent_mcp::{
    core::{ErrorCode, Money, Provenance, bounded_text, validate_kind},
    server::validation::arguments,
    service::validation::validate,
};
use serde_json::{Value, json};

#[test]
fn public_errors_keep_stable_codes_and_actionable_display_text() {
    for (code, expected) in [
        (ErrorCode::AuthRequired, "AUTH_REQUIRED"),
        (ErrorCode::SessionExpired, "SESSION_EXPIRED"),
        (ErrorCode::FeatureUnavailable, "FEATURE_UNAVAILABLE"),
        (ErrorCode::AccountIneligible, "ACCOUNT_INELIGIBLE"),
        (ErrorCode::HumanActionRequired, "HUMAN_ACTION_REQUIRED"),
        (ErrorCode::ChallengeRequired, "CHALLENGE_REQUIRED"),
        (ErrorCode::RouteBlocked, "ROUTE_BLOCKED"),
        (ErrorCode::ProxyUnreachable, "PROXY_UNREACHABLE"),
        (ErrorCode::DnsPolicyUnverified, "DNS_POLICY_UNVERIFIED"),
        (ErrorCode::TlsValidationFailed, "TLS_VALIDATION_FAILED"),
        (ErrorCode::RateLimited, "RATE_LIMITED"),
        (ErrorCode::UpstreamChanged, "UPSTREAM_CHANGED"),
        (ErrorCode::InvalidInput, "INVALID_INPUT"),
        (ErrorCode::ResourceNotFound, "RESOURCE_NOT_FOUND"),
        (ErrorCode::PermissionDenied, "PERMISSION_DENIED"),
        (ErrorCode::ConfirmationRequired, "CONFIRMATION_REQUIRED"),
        (ErrorCode::ConfirmationExpired, "CONFIRMATION_EXPIRED"),
        (ErrorCode::ActionStateChanged, "ACTION_STATE_CHANGED"),
        (ErrorCode::ActionOutcomeUnknown, "ACTION_OUTCOME_UNKNOWN"),
        (ErrorCode::FilePolicyViolation, "FILE_POLICY_VIOLATION"),
        (ErrorCode::PortInUse, "PORT_IN_USE"),
        (ErrorCode::StorageUnavailable, "STORAGE_UNAVAILABLE"),
    ] {
        assert_eq!(code.as_str(), expected);
        assert_eq!(code.to_string(), expected);
        assert_eq!(serde_json::to_value(code).unwrap(), json!(expected));
    }
    let error = fiverr_agent_mcp::core::error(ErrorCode::InvalidInput, "Review the input.");
    assert_eq!(error.to_string(), "INVALID_INPUT: Review the input.");
}

#[test]
fn money_formats_fixed_extremes_and_rejects_invalid_currency_or_scale() {
    for (minor, scale, expected) in [
        (0, 0, "0"),
        (-1, 2, "-0.01"),
        (1, 6, "0.000001"),
        (i64::MIN, 0, "-9223372036854775808"),
        (i64::MIN, 6, "-9223372036854.775808"),
        (i64::MAX, 2, "92233720368547758.07"),
    ] {
        let money = Money {
            minor,
            currency: "USD".into(),
            scale,
        };
        assert_eq!(money.decimal().unwrap(), expected);
    }
    for (currency, scale) in [("usd", 2), ("US", 2), ("USDD", 2), ("U$D", 2), ("USD", 7)] {
        let money = Money {
            minor: 1,
            currency: currency.into(),
            scale,
        };
        assert_eq!(money.validate().unwrap_err().code, ErrorCode::InvalidInput);
        assert_eq!(money.decimal().unwrap_err().code, ErrorCode::InvalidInput);
    }
    assert!(
        serde_json::from_value::<Money>(json!({"minor":1,"currency":"USD","scale":2,"extra":true}))
            .is_err()
    );
    assert!(
        serde_json::from_value::<Money>(
            json!({"minor":9223372036854775808_u64,"currency":"USD","scale":2})
        )
        .is_err()
    );
}

#[test]
fn provenance_and_text_require_valid_timestamps_and_bounded_safe_utf8() {
    let baseline = Provenance {
        source: "user_import".into(),
        observed_at: "2026-01-02T03:04:05+02:00".into(),
        uri: Some("https://www.fiverr.com/example".into()),
        partial: true,
    };
    for source in [
        "user_import",
        "local_draft",
        "public_page",
        "browser",
        "fixture",
    ] {
        let mut provenance = baseline.clone();
        provenance.source = source.into();
        provenance.validate().unwrap();
    }
    for (field, value) in [
        ("source", "live_verified"),
        ("observed_at", "2026-02-30T00:00:00Z"),
        ("uri", "https://name@example.test/"),
        ("uri", "unsafe\0"),
    ] {
        let mut provenance = baseline.clone();
        match field {
            "source" => provenance.source = value.into(),
            "observed_at" => provenance.observed_at = value.into(),
            _ => provenance.uri = Some(value.into()),
        }
        assert_eq!(
            provenance.validate().unwrap_err().code,
            ErrorCode::InvalidInput,
            "{field}"
        );
    }
    let mut provenance = baseline;
    provenance.uri = Some("a".repeat(2049));
    assert_eq!(
        provenance.validate().unwrap_err().code,
        ErrorCode::InvalidInput
    );
    bounded_text("فارسی\n\r\t", 13).unwrap();
    assert_eq!(
        bounded_text("é", 1).unwrap_err().code,
        ErrorCode::InvalidInput
    );
    for text in ["\0", "\u{1b}", "\u{7}", "\u{7f}"] {
        assert_eq!(
            bounded_text(text, 10).unwrap_err().code,
            ErrorCode::InvalidInput
        );
    }
    assert_eq!(
        validate_kind("unknown").unwrap_err().code,
        ErrorCode::InvalidInput
    );
}

#[test]
fn each_structural_domain_reports_local_validation_not_platform_success() {
    for (kind, payload, warnings) in [
        ("gig", json!({"title":"Useful service","tags":["rust"]}), 1),
        (
            "message",
            json!({"recipient_id":"buyer1","body":"Hello"}),
            0,
        ),
        (
            "offer",
            json!({"recipient_id":"buyer1","scope":"Review","money":{"minor":1,"currency":"USD","scale":2},"delivery_days":365}),
            0,
        ),
        (
            "delivery",
            json!({"order_id":"order1","body":"Owner review"}),
            1,
        ),
        (
            "task",
            json!({"title":"Review","due_at":"2026-01-01T00:00:00Z","status":"done"}),
            0,
        ),
        ("note", json!({"body":"Note"}), 0),
        ("template", json!({"title":"Greeting","body":"Hello"}), 0),
        ("profile", json!({"bio":"Factual biography"}), 1),
        (
            "analytics",
            json!({"count":0,"missing":null,"enabled":false}),
            0,
        ),
    ] {
        let result = validate(kind, &payload).unwrap();
        assert_eq!(result["valid"], true, "{kind}");
        assert_eq!(result["validation_scope"], "local_structural");
        assert_eq!(result["remote_validated"], false);
        assert_eq!(result["warnings"].as_array().unwrap().len(), warnings);
    }
    for days in [1, 365] {
        validate("offer", &json!({"recipient_id":"buyer1","scope":"Review","money":{"minor":100,"currency":"USD","scale":2},"delivery_days":days})).unwrap();
    }
    for status in ["open", "done", "cancelled"] {
        validate("task", &json!({"title":"Review","status":status})).unwrap();
    }
}

#[test]
fn domain_errors_reject_wrong_types_missing_values_and_out_of_range_terms() {
    for (kind, payload) in [
        ("gig", json!({})),
        ("gig", json!({"title":1})),
        ("gig", json!({"title":" \t"})),
        ("gig", json!({"title":"x".repeat(201)})),
        ("gig", json!({"title":"Ok","tags":"rust"})),
        ("gig", json!({"title":"Ok","tags":vec!["x";21]})),
        ("message", json!({"body":"Hi"})),
        ("message", json!({"body":"Hi","recipient_id":"bad/id"})),
        (
            "message",
            json!({"body":"x".repeat(20001),"recipient_id":"buyer"}),
        ),
        ("offer", json!({"scope":"Review","recipient_id":"buyer"})),
        (
            "offer",
            json!({"scope":"Review","recipient_id":"buyer","money":false}),
        ),
        (
            "offer",
            json!({"scope":"Review","recipient_id":"buyer","money":{"minor":0,"currency":"USD","scale":2},"delivery_days":1}),
        ),
        (
            "offer",
            json!({"scope":"Review","recipient_id":"buyer","money":{"minor":-1,"currency":"USD","scale":2},"delivery_days":1}),
        ),
        (
            "offer",
            json!({"scope":"Review","recipient_id":"buyer","money":{"minor":1,"currency":"usd","scale":2},"delivery_days":1}),
        ),
        (
            "offer",
            json!({"scope":"Review","recipient_id":"buyer","money":{"minor":1,"currency":"USD","scale":2}}),
        ),
        (
            "offer",
            json!({"scope":"Review","recipient_id":"buyer","money":{"minor":1,"currency":"USD","scale":2},"delivery_days":0}),
        ),
        (
            "offer",
            json!({"scope":"Review","recipient_id":"buyer","money":{"minor":1,"currency":"USD","scale":2},"delivery_days":366}),
        ),
        (
            "offer",
            json!({"scope":"Review","recipient_id":"buyer","money":{"minor":1,"currency":"USD","scale":2},"delivery_days":1.5}),
        ),
        ("delivery", json!({"body":"Review"})),
        ("delivery", json!({"order_id":"order1","body":""})),
        ("task", json!({"title":"Review","due_at":12})),
        ("task", json!({"title":"Review","due_at":"tomorrow"})),
        ("task", json!({"title":"Review","status":"running"})),
        ("task", json!({"title":"Review","status":true})),
        ("template", json!({"title":"Greeting"})),
        ("profile", json!({"bio":false})),
        ("profile", json!({"bio":"x".repeat(10001)})),
        ("note", json!(null)),
        ("note", json!({"body":"x".repeat(256*1024)})),
        ("unknown", json!({})),
    ] {
        assert_eq!(
            validate(kind, &payload).unwrap_err().code,
            ErrorCode::InvalidInput,
            "{kind}"
        );
    }
}

#[test]
fn recursive_validation_accepts_exact_limits_and_rejects_the_next_element() {
    for length in [1000, 1001] {
        let result = validate("analytics", &json!({"rows":vec![Value::Null;length]}));
        if length == 1000 {
            assert_eq!(result.unwrap()["valid"], true);
        } else {
            assert_eq!(result.unwrap_err().code, ErrorCode::InvalidInput);
        }
    }
    for length in [200, 201] {
        let fields: serde_json::Map<String, Value> = (0..length)
            .map(|i| (format!("field{i}"), json!(i)))
            .collect();
        let result = validate("analytics", &Value::Object(fields));
        if length == 200 {
            assert_eq!(result.unwrap()["valid"], true);
        } else {
            assert_eq!(result.unwrap_err().code, ErrorCode::InvalidInput);
        }
    }
    for depth in [16, 17] {
        let mut payload = Value::Null;
        for _ in 0..depth {
            payload = json!({"child":payload});
        }
        let result = validate("analytics", &payload);
        if depth == 16 {
            assert_eq!(result.unwrap()["valid"], true);
        } else {
            assert_eq!(result.unwrap_err().code, ErrorCode::InvalidInput);
        }
    }
    for length in [64000, 64001] {
        let result = validate("analytics", &json!({"text":"x".repeat(length)}));
        if length == 64000 {
            assert_eq!(result.unwrap()["valid"], true);
        } else {
            assert_eq!(result.unwrap_err().code, ErrorCode::InvalidInput);
        }
    }
    for key in ["a".repeat(128), "a".repeat(129)] {
        let payload = Value::Object([(key.clone(), json!(null))].into_iter().collect());
        let result = validate("analytics", &payload);
        if key.len() == 128 {
            assert_eq!(result.unwrap()["valid"], true);
        } else {
            assert_eq!(result.unwrap_err().code, ErrorCode::InvalidInput);
        }
    }
    for field in [
        "PASSWORD",
        "cookie",
        "Authorization",
        "access_token",
        "csrf",
        "otp",
        "private_key",
    ] {
        let nested = Value::Object([(field.into(), json!("synthetic"))].into_iter().collect());
        assert_eq!(
            validate("analytics", &json!({"rows":[nested]}))
                .unwrap_err()
                .code,
            ErrorCode::InvalidInput,
            "{field}"
        );
    }
}

#[test]
fn advertised_argument_contract_rejects_unknown_required_type_enum_and_bounds() {
    for (name, value, message) in [
        ("not_a_tool", json!({}), "Unknown implemented tool."),
        (
            "fiverr_status",
            json!([]),
            "Tool arguments must be an object.",
        ),
        (
            "fiverr_record_get",
            json!({"kind":"note"}),
            "id is required.",
        ),
        (
            "fiverr_status",
            json!({"extra":1}),
            "Unexpected tool argument.",
        ),
        (
            "fiverr_record_save",
            json!({"kind":"note","payload":false}),
            "payload has an invalid type.",
        ),
        (
            "fiverr_record_get",
            json!({"kind":"note","id":1}),
            "id has an invalid type.",
        ),
        (
            "fiverr_import",
            json!({"records":false}),
            "records has an invalid type.",
        ),
        (
            "fiverr_records_list",
            json!({"kind":"note","limit":1.5}),
            "limit has an invalid type.",
        ),
        (
            "fiverr_records_list",
            json!({"kind":"unknown"}),
            "kind has an unsupported value.",
        ),
        (
            "fiverr_records_list",
            json!({"kind":"note","limit":-1}),
            "limit is outside its allowed range.",
        ),
        (
            "fiverr_records_list",
            json!({"kind":"note","limit":101}),
            "limit is outside its allowed range.",
        ),
        (
            "fiverr_record_save",
            json!({"kind":"note","payload":{},"expected_version":0}),
            "expected_version is outside its allowed range.",
        ),
        (
            "fiverr_records_search",
            json!({"kind":"note","query":""}),
            "query has an invalid length.",
        ),
        (
            "fiverr_records_search",
            json!({"kind":"note","query":"é".repeat(257)}),
            "query has an invalid length.",
        ),
        (
            "fiverr_import",
            json!({"records":vec![Value::Null;1001]}),
            "records has too many items.",
        ),
        (
            "fiverr_action_prepare",
            json!({"kind":"note","target_id":"a","payload":{},"files":vec!["a.txt";11]}),
            "files has too many items.",
        ),
    ] {
        let error = arguments(name, &value).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput, "{name}");
        assert_eq!(error.message, message, "{name}");
    }
    for (name, value) in [
        ("fiverr_records_list", json!({"kind":"note","limit":1})),
        ("fiverr_records_list", json!({"kind":"note","limit":100})),
        (
            "fiverr_records_search",
            json!({"kind":"note","query":"é".repeat(256)}),
        ),
        (
            "fiverr_record_save",
            json!({"kind":"note","payload":{},"expected_version":u64::MAX}),
        ),
        ("fiverr_import", json!({"records":vec![Value::Null;1000]})),
    ] {
        arguments(name, &value).unwrap();
    }
}
