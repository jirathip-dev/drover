use serde_json::Value;
use std::path::Path;

fn workflow(name: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../templates/ci")
        .join(name);
    let o = std::process::Command::new("ruby")
        .args([
            "-rjson",
            "-ryaml",
            "-e",
            "puts JSON.generate(YAML.safe_load(File.read(ARGV[0])))",
        ])
        .arg(&p)
        .output()
        .expect("ruby on PATH");
    assert!(o.status.success(), "{name}: {}", String::from_utf8_lossy(&o.stderr));
    serde_json::from_slice(&o.stdout).unwrap()
}

#[test]
fn ci_runs_on_labeled_with_a_run_unique_group_for_other_labels() {
    let w = workflow("ci.yml");
    let types = w["true"]["pull_request"]["types"].as_array().unwrap();
    assert!(types.iter().any(|t| t == "labeled"), "{types:?}");
    let group = w["concurrency"]["group"].as_str().unwrap();
    for part in [
        "github.event.action != 'labeled' || github.event.label.name == (vars.MERGE_LABEL || 'merge-ready')",
        "format('pr-{0}', github.event.pull_request.number)",
        "|| format('run-{0}', github.run_id) }}",
    ] {
        assert!(group.contains(part), "{part}: {group}");
    }
    let jobs = w["jobs"].as_object().unwrap();
    assert_eq!(jobs.keys().collect::<Vec<_>>(), ["changes", "ci", "leg", "reuse"]);
    assert_eq!(w["jobs"]["ci"]["if"], "always()");
    assert_eq!(w["jobs"]["ci"]["needs"], serde_json::json!(["reuse", "changes", "leg"]));
    assert!(w["jobs"]["changes"]["outputs"]["legs"].is_string());
}

#[test]
fn nightly_red_runs_the_full_ci_and_reports_its_result() {
    let w = workflow("nightly-red.yml");
    assert!(w["true"]["schedule"][0]["cron"].is_string(), "{w}");
    assert_eq!(w["jobs"]["full"]["uses"], "./.github/workflows/ci.yml");
    assert_eq!(w["jobs"]["report"]["needs"], "full");
    let run = w["jobs"]["report"]["steps"][0]["run"].as_str().unwrap();
    assert!(
        run.contains("--label \"nightly red\"") && run.contains("gh issue close"),
        "{run}"
    );
}
