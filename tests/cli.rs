use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
fn run(args: &[&str], cwd: &Path) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_rowtide"))
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
#[test]
fn rowtide_names_and_legacy_receipt_identity_boundary_are_explicit() {
    let binary = env!("CARGO_BIN_EXE_rowtide");
    let version = Command::new(binary).arg("--version").output().unwrap();
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        "rowtide 0.2.1"
    );
    let help = Command::new(binary).arg("--help").output().unwrap();
    let help = String::from_utf8(help.stdout).unwrap();
    assert!(help.contains("Usage: rowtide"));
    assert!(help.contains("An agent-native toolkit for local data"));

    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("data.csv"), "entity\nA\nA\n").unwrap();
    fs::write(dir.path().join("request.json"), json!({
        "task":"legacy-name-regression", "purpose":"Keep a known key mismatch across a naming upgrade",
        "inputs":[{"id":"data","path":"data.csv","role":"entities","format":"csv","columns":["entity"]}],
        "checks":[{"id":"keys","input":"data","kind":{"type":"key","columns":["entity"]}}]
    }).to_string()).unwrap();
    run(
        &["prepare", "--request", "request.json", "--out", "first"],
        dir.path(),
    );
    let mut legacy: Value =
        serde_json::from_slice(&fs::read(dir.path().join("first/receipt.json")).unwrap()).unwrap();
    // Constructed public regression: never edit an actual historical receipt.
    legacy["engine_version"] = json!("local-data/0.1.2/checks-1/historical-fixture");
    let legacy_bytes = legacy.to_string();
    fs::write(dir.path().join("legacy.json"), &legacy_bytes).unwrap();
    let upgraded = run(
        &["refresh", "--previous", "legacy.json", "--out", "upgraded"],
        dir.path(),
    );
    assert_eq!(upgraded["execution"], "completed");
    assert_eq!(upgraded["rechecked_checks"], 1);
    assert_eq!(upgraded["reused_checks"], 0);
    assert_eq!(upgraded["mismatches"], 1);
    let receipt: Value =
        serde_json::from_slice(&fs::read(dir.path().join("upgraded/receipt.json")).unwrap())
            .unwrap();
    assert_eq!(receipt["engine_version"], rowtide::model::ENGINE_VERSION);
    assert!(
        receipt["engine_version"]
            .as_str()
            .unwrap()
            .starts_with("rowtide/0.2.1/checks-1/")
    );
    assert_eq!(
        receipt["findings"][0]["details"]["reuse"]["reason_code"],
        "implementation_changed"
    );
    let same = run(
        &[
            "refresh",
            "--previous",
            "upgraded/receipt.json",
            "--out",
            "same",
        ],
        dir.path(),
    );
    assert_eq!(same["rechecked_checks"], 0);
    assert_eq!(same["reused_checks"], 1);
    assert_eq!(same["mismatches"], 1);
    fs::write(dir.path().join("native.json"), json!({
        "path":legacy["request"]["inputs"][0]["path"], "sha256":legacy["bindings"]["input:data"]["sha256"],
        "columns":["entity"], "role":"entities"
    }).to_string()).unwrap();
    fs::write(dir.path().join("mapping.json"), json!({"source":"native.json","mappings":[{
        "input":"data","path_pointer":"/path","sha256_pointer":"/sha256","columns_pointer":"/columns","role_pointer":"/role"
    }]}).to_string()).unwrap();
    let recorded = run(
        &[
            "record",
            "--previous",
            "legacy.json",
            "--actual",
            "mapping.json",
            "--out",
            "historical-record",
        ],
        dir.path(),
    );
    assert_eq!(recorded["carried_checks"], 1);
    assert_eq!(recorded["rechecked_checks"], 0);
    let historical: Value = serde_json::from_slice(
        &fs::read(dir.path().join("historical-record/receipt.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(historical["engine_version"], legacy["engine_version"]);
    assert_eq!(historical["consumption"][0]["status"], "matched");
    assert_eq!(historical["consumption"][0]["origin"], "source_declared");
    assert_eq!(
        fs::read_to_string(dir.path().join("legacy.json")).unwrap(),
        legacy_bytes
    );
}
#[test]
fn record_reports_mismatched_unknown_and_matched_declarations_with_consistent_actions() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("data.csv"), "entity\nA\n").unwrap();
    fs::write(dir.path().join("request.json"), json!({
        "task":"record-guidance", "purpose":"Compare declared consumer input",
        "inputs":[{"id":"data","path":"data.csv","role":"entities","format":"csv","columns":["entity"]}]
    }).to_string()).unwrap();
    run(
        &["prepare", "--request", "request.json", "--out", "prepare"],
        dir.path(),
    );
    let old: Value =
        serde_json::from_slice(&fs::read(dir.path().join("prepare/receipt.json")).unwrap())
            .unwrap();
    let exact = json!({"path":old["request"]["inputs"][0]["path"], "sha256":old["bindings"]["input:data"]["sha256"], "columns":["entity"], "role":"entities"});
    fs::write(dir.path().join("mapping.json"), json!({"source":"native.json","mappings":[{
        "input":"data","path_pointer":"/path","sha256_pointer":"/sha256","columns_pointer":"/columns","role_pointer":"/role"
    }]}).to_string()).unwrap();
    for (label, hash, execution, mismatched, unknown) in [
        ("mismatched", json!("0".repeat(64)), "completed", 1, 0),
        ("unknown", Value::Null, "partial", 0, 1),
        ("matched", exact["sha256"].clone(), "completed", 0, 0),
    ] {
        let mut native = exact.clone();
        native["sha256"] = hash;
        fs::write(dir.path().join("native.json"), native.to_string()).unwrap();
        let stdout = run(
            &[
                "record",
                "--previous",
                "prepare/receipt.json",
                "--actual",
                "mapping.json",
                "--out",
                label,
            ],
            dir.path(),
        );
        assert_eq!(stdout["execution"], execution);
        assert_eq!(stdout["consumption_mismatched"], mismatched);
        assert_eq!(stdout["consumption_unknown"], unknown);
        let receipt: Value =
            serde_json::from_slice(&fs::read(dir.path().join(label).join("receipt.json")).unwrap())
                .unwrap();
        let summary = fs::read_to_string(dir.path().join(label).join("SUMMARY.md")).unwrap();
        assert_eq!(receipt["consumption"][0]["origin"], "source_declared");
        let actions = receipt["next_actions"].as_array().unwrap();
        if label == "matched" {
            assert!(actions.is_empty());
            assert!(summary.contains("No unresolved evidence actions recorded."));
        } else {
            assert_eq!(actions.len(), 1);
            let action = actions[0].as_str().unwrap();
            assert!(action.contains(&format!("Review consumption data ({label})")));
            assert!(summary.contains(action));
            assert!(!summary.contains("No unresolved evidence actions recorded."));
        }
    }
}
#[test]
fn refresh_uses_normalized_request_across_cwd_and_still_rehashes_or_overrides() {
    let dir = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("data.csv"), "entity\nA\nB\n").unwrap();
    let request = dir.path().join("request.json");
    let mut value = json!({"task":"public-fixture","purpose":"check input keys","inputs":[{"id":"data","path":"data.csv","role":"entities","format":"csv"}],"checks":[{"id":"keys","input":"data","kind":{"type":"key","columns":["entity"]}}]});
    fs::write(&request, value.to_string()).unwrap();
    let first = dir.path().join("first");
    let result = run(
        &[
            "prepare",
            "--request",
            request.to_str().unwrap(),
            "--out",
            first.to_str().unwrap(),
        ],
        elsewhere.path(),
    );
    assert_eq!(result["rechecked_checks"], 1);
    let receipt = first.join("receipt.json");
    let same = dir.path().join("same");
    let result = run(
        &[
            "refresh",
            "--previous",
            receipt.to_str().unwrap(),
            "--out",
            same.to_str().unwrap(),
        ],
        elsewhere.path(),
    );
    assert_eq!(result["reused_checks"], 1);
    let native = dir.path().join("native.json");
    fs::write(&native, "{}").unwrap();
    let mapping = dir.path().join("mapping.json");
    fs::write(&mapping, json!({"source":native,"mappings":[]}).to_string()).unwrap();
    for (prior, label) in [
        (receipt.clone(), "record-prepare"),
        (same.join("receipt.json"), "record-refresh"),
    ] {
        let out = dir.path().join(label);
        let result = run(
            &[
                "record",
                "--previous",
                prior.to_str().unwrap(),
                "--actual",
                mapping.to_str().unwrap(),
                "--out",
                out.to_str().unwrap(),
            ],
            elsewhere.path(),
        );
        assert_eq!(result["reused_checks"], 0);
        assert_eq!(result["rechecked_checks"], 0);
        assert_eq!(result["carried_checks"], 1);
        assert_eq!(result["consumption_unknown"], 1);
        assert!(
            fs::read_to_string(out.join("SUMMARY.md"))
                .unwrap()
                .contains("record does not rerun checks")
        );
    }
    fs::write(dir.path().join("data.csv"), "entity\nA\nA\n").unwrap();
    let changed = dir.path().join("changed");
    let result = run(
        &[
            "refresh",
            "--previous",
            receipt.to_str().unwrap(),
            "--out",
            changed.to_str().unwrap(),
        ],
        elsewhere.path(),
    );
    assert_eq!(result["rechecked_checks"], 1);
    assert_eq!(result["mismatches"], 1);
    let actual: Value =
        serde_json::from_slice(&fs::read(changed.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(
        actual["findings"][0]["details"]["reuse"]["reason_code"],
        "input_content_changed_or_unbound"
    );
    value["task"] = json!("changed-task");
    fs::write(&request, value.to_string()).unwrap();
    let explicit = dir.path().join("explicit");
    run(
        &[
            "refresh",
            "--previous",
            receipt.to_str().unwrap(),
            "--request",
            request.to_str().unwrap(),
            "--out",
            explicit.to_str().unwrap(),
        ],
        elsewhere.path(),
    );
    let actual: Value =
        serde_json::from_slice(&fs::read(explicit.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(actual["request"]["task"], "changed-task");
    assert_eq!(
        actual["findings"][0]["details"]["reuse"]["reason_code"],
        "task_or_contract_changed"
    );
}
