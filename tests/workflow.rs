//! Synthetic public regressions with fixed expectations derived from these small inputs.
use rowtide::{model::*, workflow};
use serde_json::json;
use std::{fs, path::Path};
use tempfile::TempDir;

struct Case {
    dir: TempDir,
    request: Request,
}
impl Case {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("data.csv"),
            "date,entity,value\n2026-01-01,A,10\n2026-01-02,B,20\n",
        )
        .unwrap();
        let request: Request = serde_json::from_value(json!({
            "task":"synthetic-input-audit", "purpose":"Validate a declared broad daily input",
            "inputs":[{"id":"data","path":dir.path().join("data.csv"),"role":"broad_daily_universe","format":"csv","columns":["date","entity"]}],
            "checks":[{"id":"keys","input":"data","kind":{"type":"key","columns":["date","entity"]}}]
        })).unwrap();
        Self { dir, request }
    }
    fn file(&self, name: &str, contents: &str) -> String {
        let p = self.dir.path().join(name);
        fs::write(&p, contents).unwrap();
        p.to_string_lossy().into_owned()
    }
    fn prepare(&self) -> Receipt {
        workflow::validate(&self.request).unwrap();
        workflow::prepare(self.request.clone(), None)
    }
    fn refresh(&self, previous: &Receipt) -> Receipt {
        workflow::validate(&self.request).unwrap();
        workflow::prepare(
            self.request.clone(),
            Some((Path::new("previous.json"), previous)),
        )
    }
    fn add_declaration(&mut self, input_hash: &str, value: serde_json::Value) {
        let path = self.file("source.json", &json!({"scope":value}).to_string());
        self.request.evidence.push(Evidence {
            id: "source".into(),
            path,
            pointer: Some("/scope".into()),
            excerpt: None,
            origin: "source_declared".into(),
            scope: "input population".into(),
            input_sha256: [("data".into(), input_hash.into())].into(),
        });
        self.request.declarations.push(Declaration {
            id: "population".into(),
            input: "data".into(),
            description: "Declared population".into(),
            evidence: vec!["source".into()],
            expected: json!("broad"),
        });
    }
    fn record(
        &self,
        old: &Receipt,
        native: serde_json::Value,
        pointers: serde_json::Value,
    ) -> Receipt {
        let source = self.file("native.json", &native.to_string());
        let mapping = self.file(
            "mapping.json",
            &json!({"source":source,"mappings":[pointers]}).to_string(),
        );
        workflow::record(Path::new("previous.json"), old, Path::new(&mapping)).unwrap()
    }
}
fn finding<'a>(receipt: &'a Receipt, id: &str) -> &'a Finding {
    receipt.findings.iter().find(|f| f.id == id).unwrap()
}
fn mapped_fields() -> serde_json::Value {
    json!({"input":"data","path_pointer":"/path","sha256_pointer":"/sha256","columns_pointer":"/columns","role_pointer":"/role","reader_sha256_pointer":null,"reader_parameters_pointer":null})
}
fn native(old: &Receipt) -> serde_json::Value {
    json!({"path":old.request.inputs[0].path,"sha256":old.bindings["input:data"].sha256,
        "columns":["date","entity"],"role":"broad_daily_universe"})
}

#[test]
fn positive_prepare_refresh_and_content_preserving_relocation() {
    let mut c = Case::new();
    let old = c.prepare();
    assert_eq!(old.execution, "completed");
    assert_eq!(finding(&old, "keys").status, "supported");
    assert_eq!(finding(&old, "keys").details["row_count"], 2);
    assert_eq!(finding(&old, "keys").details["duplicate_excess_rows"], 0);
    let same = c.refresh(&old);
    assert!(finding(&same, "keys").reused);
    assert_eq!(same.freshness["input:data"], "unchanged");
    let moved = c.dir.path().join("relocated.csv");
    fs::rename(&c.request.inputs[0].path, &moved).unwrap();
    c.request.inputs[0].path = moved.to_string_lossy().into_owned();
    let relocated = c.refresh(&old);
    assert!(finding(&relocated, "keys").reused);
    assert_eq!(
        relocated.bindings["input:data"].sha256,
        old.bindings["input:data"].sha256
    );
    assert_eq!(
        relocated.bindings["input:data"].path,
        c.request.inputs[0].path
    );
}

#[test]
fn changing_values_without_changing_schema_or_keys_invalidates_old_measurements() {
    let c = Case::new();
    let old = c.prepare();
    fs::write(
        &c.request.inputs[0].path,
        "date,entity,value\n2026-01-01,A,11\n2026-01-02,B,20\n",
    )
    .unwrap();
    let changed = c.refresh(&old);
    assert_eq!(changed.freshness["input:data"], "changed");
    assert!(!finding(&changed, "keys").reused);
    assert_eq!(finding(&changed, "keys").status, "supported");
    assert_ne!(
        changed.bindings["input:data"].sha256,
        old.bindings["input:data"].sha256
    );
}

#[test]
fn projection_requirements_filter_and_role_are_consumer_identity() {
    let c = Case::new();
    let old = c.prepare();
    let mut columns = c.request.clone();
    columns.inputs[0].columns.push("value".into());
    let mut check = c.request.clone();
    check.checks[0].kind = CheckKind::Key {
        columns: vec!["entity".into()],
    };
    let mut filter = c.request.clone();
    filter.checks[0].scope = Some(Filter {
        column: "date".into(),
        min: Some("2026-01-02".into()),
        max: None,
    });
    let mut role = c.request.clone();
    role.inputs[0].role = "selected_monthly_subset".into();
    for request in [columns, check, filter, role] {
        let result = workflow::prepare(request, Some((Path::new("previous.json"), &old)));
        assert!(!finding(&result, "keys").reused);
        assert_eq!(finding(&result, "keys").status, "supported");
    }
}

#[test]
fn changed_reader_bytes_and_parameters_invalidate_reuse() {
    let mut c = Case::new();
    let reader = c.file("reader.txt", "reader-v1");
    c.request.inputs[0].reader = Some(Reader {
        path: reader.clone(),
        parameters: [("missing_slots".into(), "preserve".into())].into(),
    });
    let old = c.prepare();
    assert!(finding(&c.refresh(&old), "keys").reused);
    c.request.inputs[0]
        .reader
        .as_mut()
        .unwrap()
        .parameters
        .insert("missing_slots".into(), "drop".into());
    let parameter_change = c.refresh(&old);
    assert!(!finding(&parameter_change, "keys").reused);
    assert_eq!(
        finding(&parameter_change, "keys").details["reuse"]["reason_code"],
        "input_config_changed"
    );
    c.request = old.request.clone();
    fs::write(reader, "reader-v2").unwrap();
    let changed = c.refresh(&old);
    assert!(!finding(&changed, "keys").reused);
    assert_eq!(changed.freshness["reader:data"], "changed");
    assert_eq!(
        finding(&changed, "keys").details["reuse"]["reason_code"],
        "reader_content_changed_or_unbound"
    );
}

#[test]
fn bound_source_declaration_has_positive_and_negative_results_and_rechecks_changes() {
    let mut c = Case::new();
    let hash = c.prepare().bindings["input:data"].sha256.clone().unwrap();
    c.add_declaration(&hash, json!("broad"));
    let old = c.prepare();
    assert_eq!(finding(&old, "population").status, "supported");
    assert_eq!(finding(&old, "population").origin, "source_declared");
    c.file("source.json", "{\"scope\":\"selected\"}");
    let refreshed = c.refresh(&old);
    assert_eq!(finding(&refreshed, "population").status, "mismatch");
    assert_eq!(refreshed.freshness["evidence:source"], "changed");
    assert!(finding(&refreshed, "keys").reused); // Unaffected physical key fact remains reusable.
    fs::write(
        &c.request.inputs[0].path,
        "date,entity,value\n2026-01-01,A,99\n2026-01-02,B,20\n",
    )
    .unwrap();
    assert_eq!(finding(&c.refresh(&old), "population").status, "unknown");
}

#[test]
fn proposed_conflicting_missing_and_unbound_evidence_do_not_certify_current_input() {
    let mut base = Case::new();
    let hash = base.prepare().bindings["input:data"]
        .sha256
        .clone()
        .unwrap();
    base.add_declaration(&hash, json!("broad"));
    let mut proposed = base.request.clone();
    proposed.evidence[0].origin = "agent_proposed".into();
    let mut unbound = base.request.clone();
    unbound.evidence[0].input_sha256.clear();
    let mut missing = base.request.clone();
    missing.evidence[0].pointer = Some("/absent".into());
    let mut conflict = base.request.clone();
    let mut other = conflict.evidence[0].clone();
    other.id = "correction".into();
    other.path = base.file("correction.json", "{\"scope\":\"selected\"}");
    conflict.evidence.push(other);
    conflict.declarations[0].evidence.push("correction".into());
    for request in [proposed, unbound, missing, conflict] {
        let result = workflow::prepare(request, None);
        assert_eq!(finding(&result, "population").status, "unknown");
        assert_eq!(result.execution, "partial");
    }
    fs::remove_file(&base.request.evidence[0].path).unwrap();
    let absent = base.prepare();
    assert_eq!(finding(&absent, "population").status, "unknown");
    assert_eq!(absent.bindings["evidence:source"].state, "unavailable");
}

#[test]
fn zero_hash_budget_and_unavailable_data_cannot_produce_reusable_certification() {
    let mut c = Case::new();
    let old = c.prepare();
    c.request.budgets.hash_bytes = 0;
    let unbound = c.refresh(&old);
    assert_eq!(unbound.execution, "partial");
    assert_eq!(unbound.freshness["input:data"], "unbound");
    assert_eq!(finding(&unbound, "keys").status, "unknown");
    assert!(!finding(&unbound, "keys").reused);
    fs::remove_file(&c.request.inputs[0].path).unwrap();
    let absent = c.refresh(&old);
    assert_eq!(absent.freshness["input:data"], "unavailable");
    assert_eq!(finding(&absent, "keys").status, "unknown");
    assert!(!finding(&absent, "keys").reused);
}

#[test]
fn native_consumption_mapping_requires_identity_projection_and_role() {
    let c = Case::new();
    let old = c.prepare();
    let path_only=c.record(&old,json!({"path":old.request.inputs[0].path}),json!({"input":"data","path_pointer":"/path","sha256_pointer":null,"columns_pointer":null,"role_pointer":null,"reader_sha256_pointer":null,"reader_parameters_pointer":null}));
    assert_eq!(path_only.consumption[0].status, "unknown");
    assert_eq!(path_only.execution, "partial");
    let exact = c.record(&old, native(&old), mapped_fields());
    assert_eq!(exact.consumption[0].status, "matched");
    assert_eq!(exact.execution, "completed");
    for (field, value) in [
        ("sha256", json!("0".repeat(64))),
        ("columns", json!(["date", "entity", "value"])),
        ("role", json!("selected_monthly_subset")),
    ] {
        let mut changed = native(&old);
        changed[field] = value;
        let result = c.record(&old, changed, mapped_fields());
        assert_eq!(result.consumption[0].status, "mismatched");
        assert!(
            result.consumption[0].details["mismatches"]
                .as_array()
                .unwrap()
                .contains(&json!(field))
        );
    }
    let mut relocated = native(&old);
    relocated["path"] = json!("elsewhere.csv");
    assert_eq!(
        c.record(&old, relocated, mapped_fields()).consumption[0].status,
        "matched"
    );
}

#[test]
fn unmapped_analysis_metadata_is_bound_but_does_not_validate_expected_analysis() {
    use sha2::{Digest, Sha256};
    let c = Case::new();
    let ready = c.prepare();
    let same = c.refresh(&ready);
    assert!(finding(&same, "keys").reused);
    let handoff = c.dir.path().join("handoff");
    workflow::publish(&same, &handoff).unwrap();
    let previous = handoff.join("receipt.json");
    let previous_bytes = fs::read(&previous).unwrap();
    let data_path = c.dir.path().join("data.csv");
    let data_digest = format!("{:x}", Sha256::digest(fs::read(&data_path).unwrap()));
    let mut sources = Vec::new();
    let expected_analysis = "counts";
    for mode in ["counts", "revenue"] {
        let declared = json!({"path":data_path,"sha256":data_digest,
            "columns":["date","entity"],"role":"broad_daily_universe",
            "analysis_run":{"mode":mode}});
        let source = c.file(&format!("native-{mode}.json"), &declared.to_string());
        let bytes = fs::read(&source).unwrap();
        let mapping = c.file(
            &format!("mapping-{mode}.json"),
            &json!({"source":source,"mappings":[mapped_fields()]}).to_string(),
        );
        let recorded = workflow::record(&previous, &same, Path::new(&mapping)).unwrap();
        assert_eq!(recorded.execution, "completed");
        assert_eq!(recorded.consumption[0].status, "matched");
        assert_eq!(recorded.consumption[0].origin, "source_declared");
        assert!(
            recorded.consumption[0].details["observed"]
                .get("analysis_run")
                .is_none()
        );
        assert_eq!(
            recorded.bindings["actual_receipt"].sha256,
            Some(format!("{:x}", Sha256::digest(&bytes)))
        );
        assert_eq!(
            serde_json::to_value(&recorded.findings).unwrap(),
            serde_json::to_value(&same.findings).unwrap()
        );
        if mode == "revenue" {
            assert_ne!(mode, expected_analysis); // Only the consumer can enforce this expectation.
        }
        workflow::publish(&recorded, &c.dir.path().join(format!("record-{mode}"))).unwrap();
        sources.push((source, bytes));
    }
    assert_ne!(Sha256::digest(&sources[0].1), Sha256::digest(&sources[1].1));
    for (path, bytes) in sources {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
    assert_eq!(fs::read(previous).unwrap(), previous_bytes);
}

#[test]
fn consumption_guidance_matches_json_and_summary_without_changing_execution_semantics() {
    let c = Case::new();
    let old = c.prepare();
    let mut changed = native(&old);
    changed["sha256"] = json!("0".repeat(64));
    let cases = [
        (
            "mismatched",
            "completed",
            changed,
            "sha256",
            "rerun the consumer",
        ),
        (
            "unknown",
            "partial",
            json!({"path": old.request.inputs[0].path}),
            "columns",
            "explicit mappings",
        ),
        ("matched", "completed", native(&old), "", ""),
    ];
    for (status, execution, actual, field, guidance) in cases {
        let result = c.record(&old, actual, mapped_fields());
        assert_eq!(result.execution, execution);
        assert_eq!(result.consumption[0].status, status);
        assert_eq!(result.consumption[0].origin, "source_declared");
        let out = c.dir.path().join(status);
        workflow::publish(&result, &out).unwrap();
        let published: serde_json::Value =
            serde_json::from_slice(&fs::read(out.join("receipt.json")).unwrap()).unwrap();
        let summary = fs::read_to_string(out.join("SUMMARY.md")).unwrap();
        if status == "matched" {
            assert!(result.next_actions.is_empty());
            assert!(summary.contains("No unresolved evidence actions recorded."));
        } else {
            assert_eq!(result.next_actions.len(), 1);
            let action = &result.next_actions[0];
            assert!(action.contains(&format!("Review consumption data ({status})")));
            assert!(action.contains(field));
            assert!(action.contains(guidance));
            assert!(action.contains("native.json"));
            assert_eq!(published["next_actions"][0], json!(action));
            assert!(summary.contains(action));
            assert!(!summary.contains("No unresolved evidence actions recorded."));
        }
    }
}

#[test]
fn rerecord_replaces_old_consumption_guidance_but_keeps_handoff_issues_and_current_notes() {
    let c = Case::new();
    c.file(
        "data.csv",
        "date,entity,value\n2026-01-01,A,10\n2026-01-01,A,20\n",
    );
    let old = c.prepare();
    assert_eq!(finding(&old, "keys").status, "mismatch");
    let unknown = c.record(&old, json!({}), mapped_fields());
    assert_eq!(unknown.next_actions.len(), 2);
    let mut prior = unknown.clone();
    prior
        .next_actions
        .push("Unverified case candidate from caller: historical note".into());
    let source = c.file("new-native.json", &native(&old).to_string());
    let mapping = c.file(
        "new-mapping.json",
        &json!({
            "source":source, "mappings":[mapped_fields()], "notes":["current note"]
        })
        .to_string(),
    );
    let corrected = workflow::record(
        Path::new("unknown/receipt.json"),
        &prior,
        Path::new(&mapping),
    )
    .unwrap();
    assert_eq!(corrected.consumption[0].status, "matched");
    assert_eq!(corrected.execution, "partial"); // Prior partial execution is retained.
    assert_eq!(
        corrected.next_actions,
        vec![
            old.next_actions[0].clone(),
            "Unverified case candidate from caller: current note".into()
        ]
    );
    assert_eq!(unknown.consumption[0].status, "unknown");
}

#[test]
fn consumption_reader_identity_and_configuration_are_required() {
    let mut c = Case::new();
    c.request.inputs[0].reader = Some(Reader {
        path: c.file("reader.txt", "v1"),
        parameters: [("slots".into(), "preserve".into())].into(),
    });
    let old = c.prepare();
    assert_eq!(
        c.record(&old, native(&old), mapped_fields()).consumption[0].status,
        "unknown"
    );
    let mut fields = mapped_fields();
    fields["reader_sha256_pointer"] = json!("/reader_hash");
    fields["reader_parameters_pointer"] = json!("/reader_parameters");
    let mut receipt = native(&old);
    receipt["reader_hash"] = json!(old.bindings["reader:data"].sha256);
    receipt["reader_parameters"] = json!({"slots":"preserve"});
    assert_eq!(
        c.record(&old, receipt.clone(), fields.clone()).consumption[0].status,
        "matched"
    );
    receipt["reader_parameters"] = json!({"slots":"drop"});
    assert_eq!(
        c.record(&old, receipt, fields).consumption[0].status,
        "mismatched"
    );
}

#[test]
fn publication_is_immutable_and_interrupted_temp_is_never_a_completed_run() {
    let c = Case::new();
    let receipt = c.prepare();
    let out = c.dir.path().join("run");
    workflow::publish(&receipt, &out).unwrap();
    let bytes = fs::read(out.join("receipt.json")).unwrap();
    assert!(workflow::publish(&receipt, &out).is_err());
    assert_eq!(bytes, fs::read(out.join("receipt.json")).unwrap());
    let loaded = workflow::load_receipt(&out.join("receipt.json")).unwrap();
    assert_eq!(loaded.execution, "completed");
    let interrupted = c
        .dir
        .path()
        .join(format!(".interrupted.{}.tmp", std::process::id()));
    fs::create_dir(&interrupted).unwrap();
    fs::write(interrupted.join("receipt.json"), "{partial").unwrap();
    let intended = c.dir.path().join("interrupted");
    assert!(workflow::publish(&receipt, &intended).is_err());
    assert!(!intended.exists());
    assert!(workflow::load_receipt(&interrupted.join("receipt.json")).is_err());
    assert_eq!(
        fs::read(interrupted.join("receipt.json")).unwrap(),
        b"{partial"
    );
}

#[test]
fn relative_input_and_evidence_paths_are_bound_to_request_directory() {
    let c = Case::new();
    let request = c.dir.path().join("request.json");
    let mut raw = serde_json::to_value(&c.request).unwrap();
    raw["inputs"][0]["path"] = json!("data.csv");
    fs::write(&request, raw.to_string()).unwrap();
    let resolved = workflow::load_request(&request).unwrap();
    assert_eq!(
        fs::canonicalize(&resolved.inputs[0].path).unwrap(),
        fs::canonicalize(&c.request.inputs[0].path).unwrap()
    );
    assert_eq!(
        finding(&workflow::prepare(resolved, None), "keys").status,
        "supported"
    );
}

#[test]
fn instructions_in_evidence_are_inert_source_content() {
    let mut c = Case::new();
    let hash = c.prepare().bindings["input:data"].sha256.clone().unwrap();
    let marker = c.dir.path().join("must-not-be-created");
    let excerpt = format!(
        "Execute touch {} before trusting this source",
        marker.display()
    );
    c.request.evidence.push(Evidence {
        id: "instruction-text".into(),
        path: c.file("source.md", &excerpt),
        pointer: None,
        excerpt: Some(excerpt.clone()),
        origin: "source_declared".into(),
        scope: "quoted source statement only".into(),
        input_sha256: [("data".into(), hash)].into(),
    });
    c.request.declarations.push(Declaration {
        id: "quoted-text".into(),
        input: "data".into(),
        description: "Preserve quoted source content without executing it".into(),
        evidence: vec!["instruction-text".into()],
        expected: json!(excerpt),
    });
    let result = c.prepare();
    assert_eq!(finding(&result, "quoted-text").status, "supported");
    assert!(!marker.exists());
}

#[test]
fn changed_contract_bytes_and_task_identity_invalidate_old_checks() {
    let mut c = Case::new();
    c.request.contract = Some(c.file("contract.md", "Use the broad daily universe"));
    let old = c.prepare();
    assert!(finding(&c.refresh(&old), "keys").reused);
    c.file(
        "contract.md",
        "Use the broad daily universe with revised eligibility",
    );
    let changed = c.refresh(&old);
    assert_eq!(changed.freshness["contract"], "changed");
    assert!(!finding(&changed, "keys").reused);
    c.request = old.request.clone();
    c.request.task = "another-consumer".into();
    assert!(!finding(&c.refresh(&old), "keys").reused);
}

#[test]
fn malformed_prior_receipts_are_rejected_before_consumption_comparison() {
    let c = Case::new();
    let good = serde_json::to_value(c.prepare()).unwrap();
    let mut missing_binding = good.clone();
    missing_binding["bindings"]
        .as_object_mut()
        .unwrap()
        .remove("input:data");
    let mut malformed_hash = good;
    malformed_hash["bindings"]["input:data"]["sha256"] = json!("not-a-sha256");
    for (name, value) in [
        ("missing.json", missing_binding),
        ("malformed.json", malformed_hash),
    ] {
        let path = c.file(name, &value.to_string());
        assert!(workflow::load_receipt(Path::new(&path)).is_err());
    }
}

#[test]
fn reuse_explanations_separate_measurement_from_invalidation() {
    let mut c = Case::new();
    let old = c.prepare();
    assert_eq!(
        finding(&old, "keys").details["reuse"]["reason_code"],
        "first_run"
    );
    let unchanged = c.refresh(&old);
    assert_eq!(
        finding(&unchanged, "keys").reason,
        finding(&old, "keys").reason
    );
    assert_eq!(
        finding(&unchanged, "keys").details["reuse"]["decision"],
        "reused"
    );
    let mut incompatible = c.prepare();
    incompatible.engine_version = "older-implementation".into();
    assert_eq!(
        finding(&c.refresh(&incompatible), "keys").details["reuse"]["reason_code"],
        "implementation_changed"
    );
    c.request.contract = Some(c.file("contract.md", "Declared identity v1"));
    let contract_old = c.prepare();
    c.file("contract.md", "Declared identity v2");
    assert_eq!(
        finding(&c.refresh(&contract_old), "keys").details["reuse"]["reason_code"],
        "task_or_contract_changed"
    );
    c.request = old.request.clone();
    c.request.inputs[0].columns.push("value".into());
    assert_eq!(
        finding(&c.refresh(&old), "keys").details["reuse"]["reason_code"],
        "input_config_changed"
    );
    c.request = old.request.clone();
    c.request.checks[0].kind = CheckKind::Key {
        columns: vec!["entity".into()],
    };
    assert_eq!(
        finding(&c.refresh(&old), "keys").details["reuse"]["reason_code"],
        "check_changed"
    );
    c.request = old.request.clone();
    let mut absent = c.prepare();
    absent.findings.clear();
    assert_eq!(
        finding(&c.refresh(&absent), "keys").details["reuse"]["reason_code"],
        "no_prior_result"
    );
}

#[test]
fn summary_has_coverage_denominators_key_counts_reuse_and_next_actions() {
    let mut c = Case::new();
    let expected = c.file(
        "expected.csv",
        "date,entity\n2026-01-01,A\n2026-01-02,B\n2026-01-03,C\n",
    );
    c.request.inputs.push(Input {
        id: "expected".into(),
        path: expected,
        role: "declared_expected_universe".into(),
        format: "csv".into(),
        columns: vec![],
        reader: None,
    });
    c.request.checks.push(Check {
        id: "coverage".into(),
        input: "data".into(),
        kind: CheckKind::Coverage {
            columns: vec!["date".into(), "entity".into()],
            expected_input: "expected".into(),
        },
        scope: None,
    });
    let receipt = c.prepare();
    let out = c.dir.path().join("published");
    workflow::publish(&receipt, &out).unwrap();
    let summary = fs::read_to_string(out.join("SUMMARY.md")).unwrap();
    assert!(summary.contains("3 expected distinct keys; 2 covered; 1 missing"));
    assert!(summary.contains("Expected rows: 3; actual rows: 2"));
    assert!(summary.contains("Keys: 2 rows; 2 distinct keys"));
    assert!(summary.contains("rechecked (`first_run`)"));
    assert!(summary.contains("Review coverage (mismatch):"));
}
