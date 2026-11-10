const WORKFLOW: &str = include_str!("../../.github/workflows/ci.yml");

#[cfg(unix)]
#[test]
fn workflow_status_shell_refuses_cancelled_or_incomplete_acceptance() {
    let block = WORKFLOW
        .split("      - name: Publish actual aggregate result\n")
        .nth(1)
        .unwrap();
    let script = block
        .split("        run: |\n")
        .nth(1)
        .unwrap()
        .lines()
        .map(|line| line.strip_prefix("          ").unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    let directory = crate::root()
        .join(".ci-work")
        .join(format!("status-matrix-{}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    let job_names = [
        "native-x86_64-pc-windows-msvc",
        "native-x86_64-unknown-linux-gnu",
        "native-aarch64-apple-darwin",
        "native-x86_64-apple-darwin",
        "security / dependencies",
    ];
    let body = r#"gh() {
  if [ "$1" = api ] && [ "$2" = --paginate ]; then cat "$JOBS";
  else printf '%s\n' "$*" >> "$POSTS"; fi
}
"#;
    for (case, focused, start, native, security, selected, complete, passed) in [
        (
            "full", false, "success", "success", "success", "skipped", true, true,
        ),
        (
            "http", true, "success", "skipped", "skipped", "success", true, true,
        ),
        (
            "proxy_failure",
            true,
            "success",
            "skipped",
            "skipped",
            "failure",
            true,
            false,
        ),
        (
            "cancelled_start",
            false,
            "cancelled",
            "success",
            "success",
            "skipped",
            true,
            false,
        ),
        (
            "failed_native",
            false,
            "success",
            "failure",
            "success",
            "skipped",
            true,
            false,
        ),
        (
            "missing_job",
            false,
            "success",
            "success",
            "success",
            "skipped",
            false,
            false,
        ),
    ] {
        let context = if focused {
            if case == "http" {
                "CI / focused HTTP deadline"
            } else {
                "CI / focused proxy capacity"
            }
        } else {
            "CI / native acceptance"
        };
        let jobs = job_names
            .iter()
            .take(if complete { 5 } else { 4 })
            .map(|name| format!("{name}\tsuccess\thttps://example.invalid/job\n"))
            .collect::<String>();
        let jobs_path = directory.join(format!("{case}.jobs"));
        let posts = directory.join(format!("{case}.posts"));
        std::fs::write(&jobs_path, jobs).unwrap();
        let script = script
            .replace(
                "${{ inputs.http_deadline_only && inputs.proxy_capacity_only }}",
                "false",
            )
            .replace("${{ github.run_id }}", "1");
        let variables = [
            ("JOBS", jobs_path.to_str().unwrap().to_owned()),
            ("POSTS", posts.to_str().unwrap().to_owned()),
            ("GH_REPO", "fixture/repository".into()),
            ("SOURCE_SHA", "dummy".into()),
            ("RUN_URL", "https://example.invalid/run".into()),
            ("START_RESULT", start.into()),
            ("NATIVE_RESULT", native.into()),
            ("SECURITY_RESULT", security.into()),
            ("FOCUSED_RESULT", selected.into()),
            ("FOCUSED", focused.to_string()),
            ("STATUS_CONTEXT", context.into()),
        ]
        .into_iter()
        .map(|(key, value)| format!("{key}='{}'\n", value.replace('\'', "'\\''")))
        .collect::<String>();
        let result = crate::process::run(
            "/bin/bash",
            &[
                "-e".into(),
                "-o".into(),
                "pipefail".into(),
                "-c".into(),
                format!("{variables}{body}\n{script}"),
            ],
            &directory,
            10,
            5,
        )
        .unwrap();
        assert_eq!(result.status.success(), passed, "{case}");
        let posted = std::fs::read_to_string(&posts).unwrap_or_default();
        if !complete {
            assert!(
                posted.is_empty(),
                "incomplete inventory must never post success"
            );
        } else {
            assert!(
                posted.contains(&format!("context={context}")),
                "{case}: {posted}"
            );
            let aggregate = posted.lines().last().unwrap();
            assert!(
                aggregate.contains(if passed {
                    "state=success"
                } else {
                    "state=failure"
                }),
                "{case}: {aggregate}"
            );
            assert_eq!(posted.lines().count(), if focused { 1 } else { 6 });
            if focused {
                assert!(!posted.contains("CI / native acceptance"));
            }
        }
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[cfg(unix)]
#[test]
fn workflow_start_rejects_contradictory_selectors_before_status_write() {
    let script = WORKFLOW
        .split("      - name: Mark exact commit pending\n")
        .nth(1)
        .unwrap()
        .split("        run: |\n")
        .nth(1)
        .unwrap()
        .split("  http-deadline:\n")
        .next()
        .unwrap()
        .lines()
        .map(|line| line.strip_prefix("          ").unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    for (conflict, diagnostic, validation, focused, success, posts) in [
        (true, false, true, true, false, 0),
        (false, true, false, false, false, 0),
        (false, false, true, true, true, 1),
        (false, false, true, false, true, 6),
    ] {
        let script = script
            .replace(
                "${{ inputs.http_deadline_only && inputs.proxy_capacity_only }}",
                &conflict.to_string(),
            )
            .replace("${{ inputs.windows_diagnostics }}", &diagnostic.to_string())
            .replace("${{ inputs.validation_only }}", &validation.to_string())
            .replace(
                "${{ inputs.http_deadline_only || inputs.proxy_capacity_only }}",
                &focused.to_string(),
            );
        let result = crate::process::run(
            "/bin/bash",
            &[
                "-e".into(),
                "-o".into(),
                "pipefail".into(),
                "-c".into(),
                format!("gh() {{ printf 'status-write\\n'; }}\n{script}"),
            ],
            &crate::root(),
            10,
            5,
        )
        .unwrap();
        assert_eq!(result.status.success(), success);
        assert_eq!(
            String::from_utf8(result.stdout).unwrap().lines().count(),
            posts
        );
    }
}

#[test]
fn focused_status_never_uses_full_native_acceptance_context() {
    let expression = "${{ inputs.http_deadline_only && 'CI / focused HTTP deadline' || inputs.proxy_capacity_only && 'CI / focused proxy capacity' || 'CI / native acceptance' }}";
    assert_eq!(
        WORKFLOW.matches(expression).count(),
        2,
        "pending and completion must bind the identical workload-specific context"
    );
    assert!(
        WORKFLOW.contains("inputs.http_deadline_only && inputs.proxy_capacity_only"),
        "contradictory selectors must fail before status writes"
    );
    assert!(
        !WORKFLOW.contains("-f context='CI / native acceptance'"),
        "no unconditional full-acceptance write from a focused run"
    );
    assert!(WORKFLOW.contains("test \"$(wc -l < jobs.tsv)\" -eq 5"));
    assert_eq!(
        WORKFLOW.matches("-f context=\"$STATUS_CONTEXT\"").count(),
        2
    );
    assert!(!WORKFLOW.contains("-f context=\"CI / native acceptance\""));
    assert!(WORKFLOW.contains("if [ \"$START_RESULT\" = success ]"));
    assert!(WORKFLOW.contains("state=failure"));
}
