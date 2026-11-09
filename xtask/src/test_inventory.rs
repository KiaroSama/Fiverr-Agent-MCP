use crate::Result;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

// Pinned nextest 0.9.151 libtest-json-plus 0.1; schema drift fails closed.
pub fn reconcile(root: &Path, target: &str, bytes: &[u8]) -> Result<Value> {
    let mut suites = BTreeMap::<String, (u64, BTreeMap<String, String>, bool)>::new();
    let mut active = BTreeMap::new();
    let mut executed_targets = BTreeSet::new();
    for line in std::str::from_utf8(bytes)?
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        let event: Value = serde_json::from_str(line)?;
        let kind = event["type"].as_str().ok_or("test event type missing")?;
        let state = event["event"].as_str().ok_or("test event state missing")?;
        if kind == "suite" {
            let metadata = &event["nextest"];
            let name = format!(
                "{}::{}",
                metadata["crate"].as_str().ok_or("suite crate missing")?,
                metadata["test_binary"]
                    .as_str()
                    .ok_or("suite binary missing")?
            );
            if state == "started" {
                let count = event["test_count"].as_u64().ok_or("suite count missing")?;
                let suite_kind = metadata["kind"].as_str().unwrap_or("test");
                if metadata["crate"] == "fiverr-agent-mcp" && suite_kind == "test" && count > 0 {
                    executed_targets.insert(
                        metadata["test_binary"]
                            .as_str()
                            .ok_or("binary missing")?
                            .to_owned(),
                    );
                }
                let qualified = format!("{name}::{suite_kind}");
                if active.insert(name.clone(), qualified.clone()).is_some() {
                    return Err("duplicate active test suite".into());
                }
                if suites
                    .insert(qualified, (count, BTreeMap::new(), false))
                    .is_some()
                {
                    return Err("duplicate test suite".into());
                }
            } else {
                let qualified = active
                    .remove(&name)
                    .ok_or("suite completion before discovery")?;
                let suite = suites.get_mut(&qualified).ok_or("suite missing")?;
                if suite.2 || !["ok", "failed"].contains(&state) {
                    return Err("invalid suite completion".into());
                }
                if event["filtered_out"] != 0 || event["ignored"] != 0 || event["measured"] != 0 {
                    return Err("unexplained filtered or ignored tests".into());
                }
                let passed = event["passed"].as_u64().ok_or("suite passed missing")?;
                let failed = event["failed"].as_u64().ok_or("suite failed missing")?;
                if passed + failed != suite.0
                    || suite.1.len() as u64 != suite.0
                    || suite
                        .1
                        .values()
                        .filter(|state| state.as_str() == "ok")
                        .count() as u64
                        != passed
                {
                    return Err("suite result/discovery mismatch".into());
                }
                suite.2 = true;
            }
        } else if kind == "test" && state != "started" {
            let identity = event["name"].as_str().ok_or("test name missing")?;
            let (suite_name, name) = identity
                .split_once('$')
                .ok_or("test identity format changed")?;
            let qualified = active.get(suite_name).ok_or("test without active suite")?;
            let suite = suites
                .get_mut(qualified)
                .ok_or("test without discovered suite")?;
            if !["ok", "failed"].contains(&state)
                || suite.1.insert(name.into(), state.into()).is_some()
            {
                return Err("ignored, duplicate or unknown test result".into());
            }
        } else if kind != "test" {
            return Err("unknown test event".into());
        }
    }
    if suites.is_empty() || suites.values().any(|suite| !suite.2) {
        return Err("missing or incomplete test suites".into());
    }
    let mut inactive = Vec::new();
    let mut targets = BTreeSet::new();
    for entry in fs::read_dir(root.join("tests"))? {
        let path = entry?.path();
        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or("test target name")?;
        targets.insert(stem.to_owned());
        if executed_targets.contains(stem) {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        let gate = text.lines().next().unwrap_or("");
        let inactive_cfg = match gate {
            "#![cfg(windows)]" => !target.contains("windows"),
            "#![cfg(unix)]" => target.contains("windows"),
            "#![cfg(target_os = \"linux\")]" => !target.contains("linux"),
            _ => false,
        };
        if !inactive_cfg {
            return Err(format!("required test target not executed: {stem}").into());
        }
        inactive.push(json!({"target":stem,"reason":gate}));
    }
    let failed = suites
        .values()
        .flat_map(|suite| suite.1.values())
        .filter(|state| state.as_str() == "failed")
        .count();
    let cases = suites.values().map(|suite| suite.1.len()).sum::<usize>();
    Ok(
        json!({"target":target,"integration_targets":targets,"inactive_cfg_targets":inactive,"cases":cases,"failed":failed,"suites":suites.into_iter().map(|(name,(count,tests,_))|json!({"name":name,"discovered":count,"tests":tests})).collect::<Vec<_>>(),"full_product_acceptance":false}),
    )
}

pub fn package_harness(root: &Path) -> Result<PathBuf> {
    let output = crate::ci::owned(
        120,
        60,
        "cargo",
        &[
            "nextest",
            "list",
            "--locked",
            "--package",
            "fiverr-agent-mcp",
            "--test",
            "package_roundtrip",
            "--message-format",
            "json",
        ],
    )?;
    let metadata: Value = serde_json::from_slice(&output.stdout)?;
    fs::write(
        root.join(".ci-work/package-test-discovery.json"),
        &output.stdout,
    )?;
    let suites = metadata["rust-suites"]
        .as_object()
        .ok_or("test discovery missing")?;
    let suite = suites
        .values()
        .find(|suite| {
            suite["package-name"] == "fiverr-agent-mcp"
                && suite["binary-name"] == "package_roundtrip"
        })
        .ok_or("package roundtrip harness not discovered")?;
    let target = std::env::var_os("CARGO_TARGET_DIR").ok_or("target directory missing")?;
    let path = PathBuf::from(
        suite["binary-path"]
            .as_str()
            .ok_or("harness binary path missing")?,
    )
    .canonicalize()?;
    if !path.starts_with(Path::new(&target).canonicalize()?) || !path.is_file() {
        return Err("harness outside owned target".into());
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn events() -> Vec<Value> {
        vec![
            json!({"type":"suite","event":"started","test_count":1,"nextest":{"crate":"fiverr-agent-mcp","test_binary":"sample"}}),
            json!({"type":"test","event":"ok","name":"fiverr-agent-mcp::sample$checks"}),
            json!({"type":"suite","event":"ok","passed":1,"failed":0,"ignored":0,"measured":0,"filtered_out":0,"nextest":{"crate":"fiverr-agent-mcp","test_binary":"sample"}}),
        ]
    }
    #[test]
    fn inventory_accepts_exact_case_count_and_rejects_missing_result() {
        let root = crate::root()
            .join(".ci-work")
            .join(format!("inventory-fixture-{}", std::process::id()));
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(root.join("tests/sample.rs"), b"// synthetic target").unwrap();
        let bytes = events()
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        let positive = reconcile(&root, "x86_64-unknown-linux-gnu", bytes.as_bytes());
        let changed = bytes
            .lines()
            .filter(|line| !line.contains("$checks"))
            .collect::<Vec<_>>()
            .join("\n");
        let negative = reconcile(&root, "x86_64-unknown-linux-gnu", changed.as_bytes());
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(positive.unwrap()["cases"], 1);
        assert!(negative.is_err());
    }
    #[test]
    fn inventory_refuses_missing_target_and_incomplete_or_duplicate_results() {
        let root = crate::root();
        let original = events();
        for mut values in [original.clone(), original[..2].to_vec(), Vec::new()] {
            let bytes = values
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n");
            assert!(reconcile(&root, "x86_64-unknown-linux-gnu", bytes.as_bytes()).is_err());
            values.insert(
                values.len().min(1),
                json!({"type":"test","event":"ok","name":"fiverr-agent-mcp::sample$checks"}),
            );
            let bytes = values
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n");
            assert!(reconcile(&root, "x86_64-unknown-linux-gnu", bytes.as_bytes()).is_err());
        }
    }
}
