use super::*;
use crate::archives::{self, Files};
use serde_json::json;

const TARGET: &str = "x86_64-pc-windows-msvc";
const SHA: &str = "1111111111111111111111111111111111111111";

struct Fixture {
    directory: PathBuf,
    native: Files,
    source: Files,
    names: [String; 3],
    policy: Value,
}
impl Fixture {
    fn new() -> Self {
        let directory = crate::root().join(".ci-work").join(format!(
            "release-corpus-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        fs::create_dir_all(&directory).unwrap();
        let names = [
            format!("{}-{}-{TARGET}.zip", crate::PRODUCT, crate::VERSION),
            format!("{}-{}-source.tar.gz", crate::PRODUCT, crate::VERSION),
            format!("fiverr-gig-optimizer-{}-plugin.zip", crate::VERSION),
        ];
        let native = Files::from([
            (
                "bin/fiverr-agent-mcp.exe".into(),
                b"synthetic executable, never executed".to_vec(),
            ),
            (
                "bin/fiverr-verify.exe".into(),
                b"synthetic verifier, never executed".to_vec(),
            ),
            ("README.md".into(), b"Synthetic public fixture".to_vec()),
        ]);
        let source = Files::from([
            ("README.md".into(), b"Synthetic public fixture".to_vec()),
            ("Cargo.toml".into(), b"synthetic manifest".to_vec()),
            ("src/main.rs".into(), b"synthetic source".to_vec()),
        ]);
        let mut fixture = Self {
            directory,
            native,
            source,
            names,
            policy: json!({"certificate_sha256":"dummy-public-pin"}),
        };
        fixture.write_native();
        archives::write(&fixture.directory.join(&fixture.names[1]), &fixture.source).unwrap();
        archives::write(
            &fixture.directory.join(&fixture.names[2]),
            &crate::package::plugin_payload(&fixture.native),
        )
        .unwrap();
        fixture.sums();
        fixture.json(
            "windows-signing-receipt.json",
            &json!({
                "source_sha":SHA, "signature_valid":true,
                "certificate_sha256":"dummy-public-pin",
                "file_sha256":validate::digest(&fixture.native["bin/fiverr-agent-mcp.exe"]),
                "verifier":{"signature_valid":true,"certificate_sha256":"dummy-public-pin","file_sha256":validate::digest(&fixture.native["bin/fiverr-verify.exe"])},
            }),
        );
        fixture
    }
    fn json(&self, name: &str, value: &Value) {
        fs::write(
            self.directory.join(name),
            serde_json::to_vec(value).unwrap(),
        )
        .unwrap();
    }
    fn write_native(&mut self) {
        self.native.remove("release.json");
        let hashes: BTreeMap<_, _> = self
            .native
            .iter()
            .map(|(name, bytes)| (name.clone(), validate::digest(bytes)))
            .collect();
        let release = json!({"source_sha":SHA,"product":crate::PRODUCT,"version":crate::VERSION,"target":TARGET,"files":hashes});
        self.json(&format!("release-{TARGET}.json"), &release);
        self.native
            .insert("release.json".into(), serde_json::to_vec(&release).unwrap());
        let path = self.directory.join(&self.names[0]);
        if path.exists() {
            fs::remove_file(&path).unwrap();
        }
        archives::write(&path, &self.native).unwrap();
    }
    fn sums(&self) {
        let sums: String = self
            .names
            .iter()
            .map(|name| {
                format!(
                    "{}  {name}\n",
                    validate::digest(&fs::read(self.directory.join(name)).unwrap())
                )
            })
            .collect();
        fs::write(
            self.directory.join(format!("SHA256SUMS-{TARGET}.txt")),
            sums,
        )
        .unwrap();
    }
    fn validate(&self) -> Result<BTreeMap<String, PathBuf>> {
        validate_assets_against(&self.directory, SHA, &self.policy, &self.source)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.directory).unwrap();
    }
}

#[test]
fn release_consistent_fixture_returns_only_six_assets() {
    let fixture = Fixture::new();
    assert_eq!(fixture.validate().unwrap().len(), 6);
}

#[test]
fn release_assets_require_exact_names_and_unique_checksums() {
    for case in ["unrelated", "duplicate", "missing", "extra", "unsafe"] {
        let fixture = Fixture::new();
        let path = fixture.directory.join(format!("SHA256SUMS-{TARGET}.txt"));
        let mut sums = fs::read_to_string(&path).unwrap();
        match case {
            "unrelated" => {
                sums.clear();
                for name in ["one.zip", "two.zip", "three.zip"] {
                    fs::write(fixture.directory.join(name), b"dummy").unwrap();
                    sums.push_str(&format!("{}  {name}\n", validate::digest(b"dummy")));
                }
            }
            "duplicate" => sums.push_str(&format!("{}\n", sums.lines().next().unwrap())),
            "missing" => {
                sums = sums
                    .lines()
                    .take(2)
                    .map(|line| format!("{line}\n"))
                    .collect()
            }
            "extra" => {
                fs::write(fixture.directory.join("extra.zip"), b"dummy").unwrap();
                sums.push_str(&format!("{}  extra.zip\n", validate::digest(b"dummy")));
            }
            _ => sums.push_str(&format!("{}  ../outside.zip\n", validate::digest(b"dummy"))),
        }
        fs::write(path, sums).unwrap();
        assert!(fixture.validate().is_err(), "accepted {case}");
    }
}

#[test]
fn release_receipt_matches_packaged_executable() {
    let mut fixture = Fixture::new();
    fixture
        .native
        .get_mut("bin/fiverr-agent-mcp.exe")
        .unwrap()
        .push(b'!');
    fixture.write_native();
    fixture.sums();
    assert!(
        fixture.validate().is_err(),
        "accepted stale executable receipt after archive checksum update"
    );
}

fn reject_substitution(case: &str) {
    let mut fixture = Fixture::new();
    match case {
        "plugin_only_source" => {
            let path = fixture.directory.join(&fixture.names[1]);
            fs::remove_file(&path).unwrap();
            archives::write(&path, &crate::package::plugin_payload(&fixture.native)).unwrap();
        }
        "missing_verifier_receipt" => {
            let path = fixture.directory.join("windows-signing-receipt.json");
            let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            value.as_object_mut().unwrap().remove("verifier");
            fixture.json("windows-signing-receipt.json", &value);
        }
        "missing_verifier" => {
            fixture.native.remove("bin/fiverr-verify.exe");
            fixture.write_native();
        }
        _ => {
            fixture
                .native
                .get_mut("bin/fiverr-verify.exe")
                .unwrap()
                .push(b'!');
            fixture.write_native();
        }
    }
    fixture.sums();
    assert!(fixture.validate().is_err(), "accepted {case}");
}

#[test]
fn release_source_inventory_cannot_be_substituted() {
    reject_substitution("plugin_only_source");
}

#[test]
fn release_verifier_must_be_present() {
    reject_substitution("missing_verifier");
}

#[test]
fn release_verifier_receipt_must_match_bytes() {
    reject_substitution("changed_verifier");
}

#[test]
fn release_verifier_receipt_is_required() {
    reject_substitution("missing_verifier_receipt");
}

#[test]
fn release_metadata_and_member_inventory_are_bound() {
    for case in [
        "target",
        "product",
        "version",
        "source_sha",
        "external",
        "member",
        "missing_executable",
    ] {
        let mut fixture = Fixture::new();
        if case == "missing_executable" {
            fixture.native.remove("bin/fiverr-agent-mcp.exe");
            fixture.write_native();
            fixture.sums();
        } else {
            let mut release: Value =
                serde_json::from_slice(&fixture.native["release.json"]).unwrap();
            match case {
                "external" => release["extra"] = json!(true),
                "member" => release["files"]["README.md"] = json!(validate::digest(b"wrong")),
                _ => release[case] = json!("wrong"),
            }
            fixture.json(&format!("release-{TARGET}.json"), &release);
            if case != "external" {
                fixture
                    .native
                    .insert("release.json".into(), serde_json::to_vec(&release).unwrap());
                let path = fixture.directory.join(&fixture.names[0]);
                fs::remove_file(&path).unwrap();
                archives::write(&path, &fixture.native).unwrap();
                fixture.sums();
            }
        }
        assert!(fixture.validate().is_err(), "accepted {case}");
    }
}
