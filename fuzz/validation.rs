#![no_main]
use fiverr_agent_mcp::{
    core::{KINDS, Money, Provenance, bounded_text, validate_id, validate_kind},
    storage::validate_relative,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    if bytes.len() > 512 {
        return;
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return;
    };
    let valid_id = !text.is_empty()
        && text.len() <= 128
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b));
    assert_eq!(validate_id(text).is_ok(), valid_id);
    assert_eq!(validate_kind(text).is_ok(), KINDS.contains(&text));
    let bounded = text.len() <= 128
        && !text
            .chars()
            .any(|c| c == '\0' || c == '\u{1b}' || (c.is_control() && !"\n\r\t".contains(c)));
    assert_eq!(bounded_text(text, 128).is_ok(), bounded);
    if let Ok(path) = validate_relative(text) {
        assert!(path.is_relative());
        assert!(
            path.components()
                .all(|c| matches!(c, std::path::Component::Normal(_)))
        );
        assert!(!text.contains([':', '\0']));
    }
    let scale = bytes.first().copied().unwrap_or(0) % 8;
    let money = Money {
        minor: i64::MIN,
        currency: text.into(),
        scale,
    };
    let valid_money = text.len() == 3 && text.bytes().all(|b| b.is_ascii_uppercase()) && scale <= 6;
    assert_eq!(money.validate().is_ok(), valid_money);
    if valid_money {
        assert!(money.decimal().unwrap().starts_with('-'));
    }
    let provenance = Provenance {
        source: text.into(),
        observed_at: "2026-11-11T00:00:00Z".into(),
        uri: None,
        partial: false,
    };
    assert_eq!(
        provenance.validate().is_ok(),
        [
            "user_import",
            "local_draft",
            "public_page",
            "browser",
            "fixture"
        ]
        .contains(&text)
    );
});
