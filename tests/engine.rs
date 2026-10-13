use rowtide::{engine::run_checks, model::*};
use std::collections::BTreeMap;
fn input(id: &str, path: &std::path::Path) -> Input {
    Input {
        id: id.into(),
        path: path.to_string_lossy().into(),
        role: "test".into(),
        format: "csv".into(),
        columns: vec![],
        reader: None,
    }
}
fn req(inputs: Vec<Input>, checks: Vec<Check>) -> Request {
    Request {
        task: "test".into(),
        purpose: "synthetic engine checks".into(),
        contract: None,
        inputs,
        checks,
        evidence: vec![],
        declarations: vec![],
        budgets: Budgets::default(),
    }
}
fn check(id: &str, kind: CheckKind) -> Check {
    Check {
        id: id.into(),
        input: "actual".into(),
        kind,
        scope: None,
    }
}
fn run(r: &Request) -> Vec<Finding> {
    run_checks(
        r,
        &r.checks.iter().map(|c| c.id.clone()).collect::<Vec<_>>(),
    )
}
#[test]
fn positive_schema_key_and_distinct_coverage() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.csv");
    let e = dir.path().join("e.csv");
    std::fs::write(&a, "id,value\n1,a\n2,b\n3,c\n").unwrap();
    std::fs::write(&e, "id\n1\n2\n2\n").unwrap();
    let r = req(
        vec![input("actual", &a), input("expected", &e)],
        vec![
            check(
                "schema",
                CheckKind::Schema {
                    required: BTreeMap::from([("id".into(), "BIGINT".into())]),
                },
            ),
            check(
                "key",
                CheckKind::Key {
                    columns: vec!["id".into()],
                },
            ),
            check(
                "coverage",
                CheckKind::Coverage {
                    columns: vec!["id".into()],
                    expected_input: "expected".into(),
                },
            ),
        ],
    );
    let f = run(&r);
    assert!(f.iter().all(|x| x.status == "supported"), "{f:?}");
    assert_eq!(f[2].details["expected_row_count"], 3);
    assert_eq!(f[2].details["expected_distinct_key_count"], 2);
    assert_eq!(f[2].details["covered_distinct_key_count"], 2);
}
#[test]
fn null_duplicates_scope_and_missing_coverage() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.csv");
    let e = dir.path().join("e.csv");
    std::fs::write(&a, "id,day\n1,1\n1,1\n,1\n2,2\n").unwrap();
    std::fs::write(&e, "id\n1\n2\n2\n3\n").unwrap();
    let mut key = check(
        "key",
        CheckKind::Key {
            columns: vec!["id".into()],
        },
    );
    key.scope = Some(Filter {
        column: "day".into(),
        min: Some("1".into()),
        max: Some("1".into()),
    });
    let mut coverage = check(
        "coverage",
        CheckKind::Coverage {
            columns: vec!["id".into()],
            expected_input: "expected".into(),
        },
    );
    coverage.scope = key.scope.clone();
    let f = run(&req(
        vec![input("actual", &a), input("expected", &e)],
        vec![key, coverage],
    ));
    assert_eq!(f[0].status, "mismatch", "{f:?}");
    assert_eq!(f[0].details["row_count"], 3);
    assert_eq!(f[0].details["null_key_rows"], 1);
    assert_eq!(f[0].details["duplicate_excess_rows"], 1);
    assert_eq!(f[0].details["distinct_key_count_including_null"], 2);
    assert_eq!(f[1].status, "mismatch");
    assert_eq!(f[1].details["missing_distinct_key_count"], 2);
    assert_eq!(f[1].details["expected_distinct_key_count"], 3);
}
#[test]
fn quoted_path_column_and_filter_do_not_execute_sql() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a'; DROP TABLE actual;--.csv");
    std::fs::write(&a, "\"odd\"\"name\",value\nx'; DROP TABLE actual;--,1\n").unwrap();
    let mut c = check(
        "quoted",
        CheckKind::Key {
            columns: vec!["odd\"name".into()],
        },
    );
    c.scope = Some(Filter {
        column: "odd\"name".into(),
        min: Some("x'; DROP TABLE actual;--".into()),
        max: Some("x'; DROP TABLE actual;--".into()),
    });
    let f = run(&req(vec![input("actual", &a)], vec![c]));
    assert_eq!(f[0].status, "supported", "{f:?}");
    assert_eq!(f[0].details["row_count"], 1);
}
#[test]
fn missing_fields_unsupported_formats_and_bad_budgets() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.csv");
    std::fs::write(&a, "id\n1\n").unwrap();
    let checks = vec![
        check(
            "schema",
            CheckKind::Schema {
                required: BTreeMap::from([("absent".into(), "*".into())]),
            },
        ),
        check(
            "key",
            CheckKind::Key {
                columns: vec!["absent".into()],
            },
        ),
    ];
    let mut r = req(vec![input("actual", &a)], checks);
    let f = run(&r);
    assert_eq!(f[0].status, "mismatch");
    assert_eq!(f[1].status, "unknown");
    r.inputs[0].format = "xlsx".into();
    assert!(run(&r).iter().all(|x| x.status == "unknown"));
    r.inputs[0].format = "csv".into();
    r.budgets.timeout_seconds = 0;
    assert!(run(&r).iter().all(|x| x.status == "unknown"));
}
#[test]
fn projection_missing_and_empty_expected_do_not_pass() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.csv");
    let e = dir.path().join("e.csv");
    std::fs::write(&a, "id\n1\n").unwrap();
    std::fs::write(&e, "id\n\n").unwrap();
    let mut actual = input("actual", &a);
    actual.columns = vec!["not_present".into()];
    let r = req(
        vec![actual, input("expected", &e)],
        vec![check(
            "schema",
            CheckKind::Schema {
                required: BTreeMap::new(),
            },
        )],
    );
    assert_eq!(run(&r)[0].status, "mismatch");
}
#[test]
fn parquet_uses_bundled_reader() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("test.parquet");
    let conn = duckdb::Connection::open_in_memory().unwrap();
    let path = a.to_string_lossy().replace('\'', "''");
    conn.execute_batch(&format!(
        "COPY (SELECT 1::BIGINT AS id UNION ALL SELECT 2) TO '{path}' (FORMAT PARQUET)"
    ))
    .unwrap();
    let mut i = input("actual", &a);
    i.format = "parquet".into();
    let f = run(&req(
        vec![i],
        vec![check(
            "key",
            CheckKind::Key {
                columns: vec!["id".into()],
            },
        )],
    ));
    assert_eq!(f[0].status, "supported", "{f:?}");
    assert_eq!(f[0].details["row_count"], 2);
}

#[test]
fn composite_keys_and_internal_alias_collision() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("composite.csv");
    std::fs::write(&a, "__local_data_count,n\n1,a\n1,a\n1,b\n2,a\n").unwrap();
    let f = run(&req(
        vec![input("actual", &a)],
        vec![check(
            "key",
            CheckKind::Key {
                columns: vec!["__local_data_count".into(), "n".into()],
            },
        )],
    ));
    assert_eq!(f[0].status, "mismatch", "{f:?}");
    assert_eq!(f[0].details["row_count"], 4);
    assert_eq!(f[0].details["distinct_key_count_including_null"], 3);
    assert_eq!(f[0].details["duplicate_excess_rows"], 1);
}

#[test]
fn null_expected_and_reversed_scope_are_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.csv");
    let e = dir.path().join("e.csv");
    std::fs::write(&a, "id\n1\n2\n").unwrap();
    std::fs::write(&e, "id,label\n1,a\n,b\n").unwrap();
    let mut key = check(
        "reversed",
        CheckKind::Key {
            columns: vec!["id".into()],
        },
    );
    key.scope = Some(Filter {
        column: "id".into(),
        min: Some("2".into()),
        max: Some("1".into()),
    });
    let r = req(
        vec![input("actual", &a), input("expected", &e)],
        vec![
            check(
                "null_universe",
                CheckKind::Coverage {
                    columns: vec!["id".into()],
                    expected_input: "expected".into(),
                },
            ),
            key,
        ],
    );
    let f = run(&r);
    assert_eq!(f[0].status, "unknown", "{f:?}");
    assert_eq!(f[0].details["expected_null_key_rows"], 1);
    assert_eq!(f[1].status, "unknown");
    assert!(f[1].reason.contains("minimum exceeds maximum"));
}

#[test]
fn example_values_are_bounded_and_marked() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("long.csv");
    let value = "长".repeat(2000);
    std::fs::write(&a, format!("id\n{value}\n{value}\n")).unwrap();
    let f = run(&req(
        vec![input("actual", &a)],
        vec![check(
            "key",
            CheckKind::Key {
                columns: vec!["id".into()],
            },
        )],
    ));
    assert_eq!(f[0].status, "mismatch");
    let example = f[0].details["duplicate_examples"][0][0].as_str().unwrap();
    assert!(example.ends_with("[truncated at 512 characters]"));
    assert!(example.chars().count() < 550);
}

#[test]
fn empty_expected_universe_is_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("actual.parquet");
    let e = dir.path().join("expected.parquet");
    let conn = duckdb::Connection::open_in_memory().unwrap();
    conn.execute_batch(&format!("COPY (SELECT 1::BIGINT AS id) TO '{}' (FORMAT PARQUET); COPY (SELECT 1::BIGINT AS id WHERE FALSE) TO '{}' (FORMAT PARQUET)", a.to_string_lossy(), e.to_string_lossy())).unwrap();
    let mut actual = input("actual", &a);
    actual.format = "parquet".into();
    let mut expected = input("expected", &e);
    expected.format = "parquet".into();
    let f = run(&req(
        vec![actual, expected],
        vec![check(
            "coverage",
            CheckKind::Coverage {
                columns: vec!["id".into()],
                expected_input: "expected".into(),
            },
        )],
    ));
    assert_eq!(f[0].status, "unknown");
    assert_eq!(f[0].details["expected_distinct_key_count"], 0);
    assert_eq!(f[0].reason, "expected universe is empty");
}
