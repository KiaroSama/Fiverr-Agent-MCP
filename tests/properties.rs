use fiverr_agent_mcp::{
    core::{Money, validate_id},
    network::profiles,
    storage::validate_relative,
};

#[test]
fn bounded_generated_inputs_preserve_path_id_money_and_proxy_contracts() {
    // Fixed seed and finite corpus: regression-friendly fuzz-style checks, not a coverage-guided fuzzer.
    let mut state = 0x53a7_019d_u64;
    let alphabet = b"aZ019_-./\\:\0\r\n?*<>|";
    for index in 0..4096 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let valid = format!("id-{index}_{}", state);
        assert!(validate_id(&valid).is_ok());
        assert!(validate_relative(&format!("folder/{valid}.txt")).is_ok());
        for unsafe_prefix in ["../", "/", "x:", "a\\", "CON/"] {
            let candidate = format!("{unsafe_prefix}{valid}");
            assert!(validate_id(&candidate).is_err());
            if unsafe_prefix != "CON/" {
                assert!(validate_relative(&candidate).is_err());
            }
        }
        let length = (state as usize % 260) + 1;
        let input: String = (0..length)
            .map(|position| {
                alphabet[(state.wrapping_add(position as u64) as usize) % alphabet.len()] as char
            })
            .collect();
        let _ = validate_id(&input);
        let _ = validate_relative(&input);
        let _ = profiles::normalize(&input);
        let minor = state as i64;
        for scale in [0, 2, 6] {
            let amount = Money {
                minor,
                currency: "USD".into(),
                scale,
            };
            let decimal = amount.decimal().unwrap();
            let normalized = decimal.replace('.', "");
            assert_eq!(normalized.parse::<i64>().unwrap(), minor);
        }
        let proxy =
            profiles::normalize(&format!("socks5://127.0.0.1:{}", 1 + state % 65535)).unwrap();
        assert_eq!(proxy.scheme(), "socks5h");
        assert_eq!(profiles::normalize(proxy.as_str()).unwrap(), proxy);
    }
    assert!(validate_id(&"a".repeat(129)).is_err());
    assert!(validate_relative(&"a".repeat(241)).is_err());
}
