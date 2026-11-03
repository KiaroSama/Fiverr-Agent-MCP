const WORKFLOW: &str = include_str!("../../.github/workflows/ci.yml");

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
