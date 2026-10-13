use crate::{Result, root};
use serde_json::{Value, json};
use std::path::Path;

pub fn report(input: &str, output: &str) -> Result<()> {
    let root = root().canonicalize()?;
    let input = Path::new(input);
    let output = Path::new(output);
    if !input.canonicalize()?.starts_with(&root)
        || !output.is_absolute()
        || !output
            .parent()
            .ok_or("coverage parent missing")?
            .canonicalize()?
            .starts_with(&root)
    {
        return Err("coverage paths must be workspace-contained".into());
    }
    let bytes = std::fs::read(input)?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("coverage JSON exceeds limit".into());
    }
    let value: Value = serde_json::from_slice(&bytes)?;
    let files = value["data"]
        .as_array()
        .ok_or("coverage data missing")?
        .iter()
        .flat_map(|data| data["files"].as_array().into_iter().flatten());
    let mut application = (0_u64, 0_u64);
    let mut safety = (0_u64, 0_u64);
    let mut included = Vec::new();
    for file in files {
        let name = file["filename"]
            .as_str()
            .ok_or("coverage filename missing")?
            .replace('\\', "/");
        let root_prefix = format!("{}/", root.to_string_lossy().replace('\\', "/"));
        let source = name.strip_prefix(&root_prefix).unwrap_or(name.as_str());
        if !source.starts_with("src/") {
            continue;
        }
        let total = file["summary"]["lines"]["count"]
            .as_u64()
            .ok_or("coverage line count missing")?;
        let covered = file["summary"]["lines"]["covered"]
            .as_u64()
            .ok_or("coverage covered lines missing")?;
        if covered > total {
            return Err("coverage counts inconsistent".into());
        }
        application.0 += covered;
        application.1 += total;
        if [
            "src/core/",
            "src/storage/",
            "src/network/",
            "src/service/actions.rs",
            "src/service/bindings.rs",
            "src/server/http.rs",
            "src/server/framing.rs",
            "src/server/validation.rs",
        ]
        .iter()
        .any(|prefix| source.starts_with(prefix))
        {
            safety.0 += covered;
            safety.1 += total;
        }
        let missing: Vec<_> = file["segments"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|segment| {
                segment[3].as_bool() == Some(true)
                    && segment[2].as_u64() == Some(0)
                    && segment[4].as_bool() == Some(true)
            })
            .filter_map(|segment| segment[0].as_u64())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        included.push(json!({"file":source,"covered":covered,"lines":total,"uncovered_region_start_lines":missing}));
    }
    if application.1 == 0 || safety.1 == 0 {
        return Err("coverage scope has zero discovered lines".into());
    }
    let score = |(covered, total): (u64, u64)| covered as f64 * 100.0 / total as f64;
    let application_percent = score(application);
    let safety_percent = score(safety);
    let receipt = json!({"metric":"Linux source line coverage, native Windows code excluded by cfg","application":{"covered":application.0,"lines":application.1,"percent":application_percent,"target":90,"target_met":application_percent>=90.0},"safety":{"covered":safety.0,"lines":safety.1,"percent":safety_percent,"target":95,"target_met":safety_percent>=95.0},"files":included,"full_product_acceptance":false});
    std::fs::write(output, serde_json::to_vec_pretty(&receipt)?)?;
    println!(
        "Measured Linux line coverage: application={application_percent:.2}% (target90), safety={safety_percent:.2}% (target95). Targets are reported truthfully, not waived."
    );
    if application_percent < 90.0 || safety_percent < 95.0 {
        return Err("Required application90% / safety95% source line coverage not met; inspect preserved coverage evidence before packaging.".into());
    }
    Ok(())
}
