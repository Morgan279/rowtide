//! Small public fixtures exercise the reading view without adding measurements.
use rowtide::{model::*, workflow};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

fn publish(receipt: &Receipt, out: &Path) -> String {
    let before = serde_json::to_value(receipt).unwrap();
    workflow::publish(receipt, out).unwrap();
    let saved: Value =
        serde_json::from_slice(&fs::read(out.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(
        saved, before,
        "the summary must not change receipt evidence"
    );
    fs::read_to_string(out.join("SUMMARY.md")).unwrap()
}

fn formats(dir: &Path) -> Request {
    let csv = dir.join("dates.csv");
    fs::write(
        &csv,
        "session_date,entity,units,unrelated\n2026-09-01,A,2,hidden\n",
    )
    .unwrap();
    let parquet = dir.join("dates.parquet");
    let conn = duckdb::Connection::open_in_memory().unwrap();
    conn.execute_batch(&format!(
        "COPY (SELECT '2026-09-01'::TIMESTAMP_NS AS session_date, 'A' AS entity, 2::BIGINT AS units, 'hidden' AS unrelated) TO '{}' (FORMAT PARQUET)",
        parquet.to_string_lossy().replace('\'', "''")
    )).unwrap();
    let request: Request = serde_json::from_value(json!({
        "task":"reader-types", "purpose":"Expose recorded reader observations",
        "inputs":[
            {"id":"csv","path":csv,"role":"daily_units","format":"csv","columns":["session_date","units"]},
            {"id":"parquet","path":parquet,"role":"daily_units","format":"parquet","columns":["session_date","units"]}
        ],
        "checks":[
            {"id":"csv_schema","input":"csv","kind":{"type":"schema","required":{"session_date":"*","entity":"VARCHAR"}}},
            {"id":"parquet_schema","input":"parquet","kind":{"type":"schema","required":{"session_date":"*","entity":"VARCHAR"}}}
        ]
    })).unwrap();
    workflow::validate(&request).unwrap();
    request
}

#[test]
fn summary_shows_reader_types_without_inferring_cross_format_equivalence() {
    let dir = tempfile::tempdir().unwrap();
    let mut request = formats(dir.path());
    let receipt = workflow::prepare(request.clone(), None);
    assert!(receipt.findings.iter().all(|f| f.status == "supported"));
    assert_eq!(
        receipt.findings[0].details["physical_schema"]["session_date"],
        "DATE"
    );
    assert_eq!(
        receipt.findings[1].details["physical_schema"]["session_date"],
        "TIMESTAMP_NS"
    );
    let summary = publish(&receipt, &dir.path().join("wildcard"));
    assert!(summary.contains(
        "Column types observed by Rowtide's built-in reader (consumer representations may differ)"
    ));
    assert!(summary.contains("`session_date`: `DATE` (required: `*`)"));
    assert!(summary.contains("`session_date`: `TIMESTAMP_NS` (required: `*`)"));
    assert!(summary.contains("`units`: `BIGINT`"));
    assert!(summary.contains("`entity`: `VARCHAR` (required: `VARCHAR`)"));
    assert!(!summary.contains("unrelated"));
    assert!(!summary.contains("incompatible"));

    // An explicit incompatible requirement still mismatches; the display never
    // substitutes the requested type for the observed type.
    if let CheckKind::Schema { required } = &mut request.checks[0].kind {
        required.insert("session_date".into(), "TIMESTAMP_NS".into());
    }
    let strict = workflow::prepare(request, None);
    assert_eq!(strict.findings[0].status, "mismatch");
    let summary = publish(&strict, &dir.path().join("strict"));
    assert!(summary.contains("`csv_schema`: **mismatch** (measured)"));
    assert!(summary.contains("`session_date`: `DATE` (required: `TIMESTAMP_NS`)"));
}

#[test]
fn summary_bounds_relevant_columns_and_keeps_full_schema_in_json() {
    let dir = tempfile::tempdir().unwrap();
    let columns = (0..40).map(|n| format!("column_{n}")).collect::<Vec<_>>();
    let csv = dir.path().join("wide.csv");
    fs::write(
        &csv,
        format!("{}\n{}\n", columns.join(","), vec!["1"; 40].join(",")),
    )
    .unwrap();
    let request: Request = serde_json::from_value(json!({
        "task":"wide-types", "purpose":"Bound the reading view",
        "inputs":[{"id":"wide","path":csv,"role":"units","format":"csv","columns":columns[..15]}],
        "checks":[{"id":"wide_schema","input":"wide","kind":{"type":"schema","required":{"column_30":"BIGINT"}}}]
    })).unwrap();
    let receipt = workflow::prepare(request, None);
    assert_eq!(receipt.findings[0].status, "supported");
    assert_eq!(
        receipt.findings[0].details["physical_schema"]
            .as_object()
            .unwrap()
            .len(),
        40
    );
    let summary = publish(&receipt, &dir.path().join("out"));
    assert_eq!(summary.matches(": `BIGINT`").count(), 12);
    assert!(summary.contains("`column_11`: `BIGINT`"));
    assert!(!summary.contains("`column_12`:"));
    assert!(!summary.contains("`column_30`:"));
    assert!(!summary.contains("column_39"));
    assert!(summary.contains("4 additional projected/required columns omitted; see receipt.json"));
}

#[test]
fn summary_does_not_fill_unknown_type_evidence_from_a_requirement() {
    let dir = tempfile::tempdir().unwrap();
    let request: Request = serde_json::from_value(json!({
        "task":"missing-types", "purpose":"Keep unknown evidence unknown",
        "inputs":[{"id":"absent","path":dir.path().join("missing.csv"),"role":"daily_units","format":"csv","columns":["session_date"]}],
        "checks":[{"id":"absent_schema","input":"absent","kind":{"type":"schema","required":{"session_date":"DATE"}}}]
    })).unwrap();
    let receipt = workflow::prepare(request, None);
    assert_eq!(receipt.findings[0].status, "unknown");
    let summary = publish(&receipt, &dir.path().join("out"));
    assert!(summary.contains("`absent_schema`: **unknown** (measured)"));
    assert!(summary.contains("`session_date`: not recorded (required: `DATE`)"));
    assert!(!summary.contains("`session_date`: `DATE`"));
}

#[test]
fn summary_keeps_carried_types_and_missing_historical_metadata_historical() {
    let dir = tempfile::tempdir().unwrap();
    let request = formats(dir.path());
    // Explicit native fields represent the fixture consumer's declared read.
    let actual = request
        .inputs
        .iter()
        .map(|input| {
            let bytes = fs::read(&input.path).unwrap();
            json!({"id":input.id,"path":input.path,"sha256":format!("{:x}",Sha256::digest(bytes)),
            "columns":["session_date","units"],"role":"daily_units"})
        })
        .collect::<Vec<_>>();
    let mut old = workflow::prepare(request, None);
    old.engine_version = "local-data/0.1.2/checks-1/historical-test".into();
    old.findings[1]
        .details
        .as_object_mut()
        .unwrap()
        .remove("physical_schema");
    publish(&old, &dir.path().join("historical"));
    let old_path = dir.path().join("historical/receipt.json");
    let old_bytes = fs::read(&old_path).unwrap();
    for input in &old.request.inputs {
        fs::remove_file(&input.path).unwrap();
    }
    fs::write(
        dir.path().join("native.json"),
        json!({"inputs":actual}).to_string(),
    )
    .unwrap();
    let mapping = dir.path().join("mapping.json");
    fs::write(&mapping, json!({"source":"native.json","mappings":[
        {"input":"csv","path_pointer":"/inputs/0/path","sha256_pointer":"/inputs/0/sha256","columns_pointer":"/inputs/0/columns","role_pointer":"/inputs/0/role"},
        {"input":"parquet","path_pointer":"/inputs/1/path","sha256_pointer":"/inputs/1/sha256","columns_pointer":"/inputs/1/columns","role_pointer":"/inputs/1/role"}
    ]}).to_string()).unwrap();
    let recorded = workflow::record(&old_path, &old, &mapping).unwrap();
    assert_eq!(recorded.engine_version, old.engine_version);
    assert_eq!(
        serde_json::to_value(&recorded.findings).unwrap(),
        serde_json::to_value(&old.findings).unwrap()
    );
    assert!(
        recorded
            .consumption
            .iter()
            .all(|f| f.status == "matched" && f.origin == "source_declared")
    );
    let summary = publish(&recorded, &dir.path().join("recorded"));
    assert!(summary.contains("carried from the previous handoff; record does not rerun checks"));
    assert!(summary.contains("`csv_schema`: **supported** (measured)"));
    assert!(summary.contains("`session_date`: `DATE` (required: `*`)"));
    assert!(summary.contains("`parquet_schema`: **supported** (measured)"));
    assert!(summary.contains("`session_date`: not recorded (required: `*`)"));
    assert_eq!(fs::read(old_path).unwrap(), old_bytes);
}

#[test]
fn summary_escapes_real_parquet_fields_and_observed_type_controls() {
    let dir = tempfile::tempdir().unwrap();
    let parquet = dir.path().join("literal.parquet");
    let field = "``date`\n\n### TYPE_RENDER_MARKER\n\n`tail``\t\r\u{2028}";
    let nested = "inner``\n\n### OBSERVED_TYPE_RENDER_MARKER\n\n`tail";
    let quote = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
    let conn = duckdb::Connection::open_in_memory().unwrap();
    conn.execute_batch(&format!(
        "COPY (SELECT '2026-09-01'::TIMESTAMP_NS AS {}, struct_pack({}:=1::BIGINT) AS nested) TO '{}' (FORMAT PARQUET)",
        quote(field), quote(nested), parquet.to_string_lossy().replace('\'', "''")
    )).unwrap();
    let required = std::collections::BTreeMap::from([(field, "*"), ("nested", "*")]);
    let request: Request = serde_json::from_value(json!({
        "task":"literal-types", "purpose":"Keep literal schema data inside the type reading view",
        "inputs":[{"id":"parquet","path":parquet,"role":"dates","format":"parquet","columns":[field,"nested"]}],
        "checks":[{"id":"schema","input":"parquet","kind":{"type":"schema","required":required}}]
    })).unwrap();
    let receipt = workflow::prepare(request, None);
    assert_eq!(receipt.findings[0].status, "supported");
    assert_eq!(
        receipt.findings[0].details["physical_schema"][field],
        "TIMESTAMP_NS"
    );
    assert!(
        receipt.findings[0].details["physical_schema"]["nested"]
            .as_str()
            .unwrap()
            .contains("OBSERVED_TYPE_RENDER_MARKER")
    );
    let summary = publish(&receipt, &dir.path().join("out"));
    assert!(
        !summary
            .lines()
            .any(|line| line.starts_with("### TYPE_RENDER_MARKER")
                || line.starts_with("### OBSERVED_TYPE_RENDER_MARKER"))
    );
    assert!(summary.contains("\\n\\n### TYPE_RENDER_MARKER\\n\\n"));
    assert!(summary.contains("\\t\\r\\u{2028}"));
    assert!(
        summary.contains("``` ``date`"),
        "backticks at the value boundary need safe delimiters/padding"
    );
    assert!(summary.contains("`tail``\\t"));
    assert_eq!(receipt.request.inputs[0].columns[0], field);
}

#[test]
fn summary_escapes_requested_type_without_rewriting_literal_json() {
    let dir = tempfile::tempdir().unwrap();
    let csv = dir.path().join("literal.csv");
    fs::write(&csv, "entity\nA\n").unwrap();
    let requested = "```VARCHAR`\n\n### REQUIRED_TYPE_RENDER_MARKER\n\n```\t";
    let request: Request = serde_json::from_value(json!({
        "task":"literal-request", "purpose":"Show an incompatible literal type requirement safely",
        "inputs":[{"id":"csv","path":csv,"role":"entities","format":"csv","columns":["entity"]}],
        "checks":[{"id":"schema","input":"csv","kind":{"type":"schema","required":{"entity":requested}}}]
    })).unwrap();
    let receipt = workflow::prepare(request, None);
    assert_eq!(receipt.findings[0].status, "mismatch");
    assert_eq!(receipt.findings[0].details["required"]["entity"], requested);
    assert_eq!(
        receipt.findings[0].details["physical_schema"]["entity"],
        "VARCHAR"
    );
    let summary = publish(&receipt, &dir.path().join("out"));
    assert!(
        !summary
            .lines()
            .any(|line| line.starts_with("### REQUIRED_TYPE_RENDER_MARKER"))
    );
    assert!(summary.contains("\\n\\n### REQUIRED_TYPE_RENDER_MARKER\\n\\n"));
    assert!(summary.contains("required: ```` ```VARCHAR`"));
    assert!(summary.contains("`entity`: `VARCHAR`"));
}

#[test]
fn record_summary_labels_prior_execution_reason_without_changing_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let csv = dir.path().join("invalid.csv");
    let bytes = b"entity\n\xff\n";
    fs::write(&csv, bytes).unwrap();
    let request: Request = serde_json::from_value(json!({
        "task":"prior-execution", "purpose":"Separate carried failure from record comparison",
        "inputs":[{"id":"csv","path":csv,"role":"entities","format":"csv","columns":["entity"]}],
        "checks":[{"id":"schema","input":"csv","kind":{"type":"schema","required":{"entity":"*"}}}]
    }))
    .unwrap();
    let old = workflow::prepare(request, None);
    assert_eq!(old.execution, "partial");
    assert_eq!(old.findings[0].status, "unknown");
    let original_reason = old.findings[0].reason.clone();
    let previous = dir.path().join("previous");
    publish(&old, &previous);
    let previous_path = previous.join("receipt.json");
    let previous_bytes = fs::read(&previous_path).unwrap();
    let source = dir.path().join("native.json");
    fs::write(&source, json!({"path":csv,"sha256":format!("{:x}",Sha256::digest(bytes)),"columns":["entity"],"role":"entities"}).to_string()).unwrap();
    let mapping = dir.path().join("mapping.json");
    fs::write(&mapping, json!({"source":source,"mappings":[{"input":"csv","path_pointer":"/path","sha256_pointer":"/sha256","columns_pointer":"/columns","role_pointer":"/role"}]}).to_string()).unwrap();
    let recorded = workflow::record(&previous_path, &old, &mapping).unwrap();
    assert_eq!(recorded.execution, "partial");
    assert_eq!(recorded.engine_version, old.engine_version);
    assert_eq!(recorded.consumption[0].status, "matched");
    assert_eq!(recorded.findings[0].reason, original_reason);
    let summary = publish(&recorded, &dir.path().join("record"));
    assert!(summary.contains(&format!("— Prior measurement: {original_reason}")));
    assert!(summary.contains("carried from the previous handoff; record does not rerun checks"));
    assert!(!summary.contains("(source_declared) — Prior measurement:"));
    assert_eq!(fs::read(previous_path).unwrap(), previous_bytes);
}
