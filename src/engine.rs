//! Bounded physical data checks. No source document or request field is SQL.
use crate::model::{Check, CheckKind, Finding, Input, Request};
use anyhow::{Context, Result, bail};
use duckdb::Connection;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}
fn literal(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}
fn source(input: &Input) -> Result<String> {
    if input
        .path
        .chars()
        .any(|c| matches!(c, '*' | '?' | '[' | ']'))
    {
        bail!("glob metacharacters in paths are unsupported; use an explicit literal file alias");
    }
    if !std::path::Path::new(&input.path).is_file() {
        bail!("input is unavailable or not a regular file");
    }
    match input.format.as_str() {
        "csv" => Ok(format!(
            "read_csv({}, header=true, auto_detect=true, strict_mode=true, encoding='utf-8')",
            literal(&input.path)
        )),
        "parquet" => Ok(format!("read_parquet({})", literal(&input.path))),
        x => bail!("unsupported format {x}; only UTF-8 csv and parquet are supported"),
    }
}
fn schema(conn: &Connection, table: &str) -> Result<BTreeMap<String, String>> {
    let mut stmt = conn.prepare(&format!("DESCRIBE SELECT * FROM {table}"))?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}
fn require(schema: &BTreeMap<String, String>, columns: &[String]) -> Result<()> {
    if columns.is_empty() {
        bail!("check requires at least one key column");
    }
    let unique: std::collections::BTreeSet<_> = columns.iter().collect();
    if unique.len() != columns.len() {
        bail!("key columns must be distinct");
    }
    for c in columns {
        if !schema.contains_key(c) {
            bail!("missing check column {c}");
        }
    }
    Ok(())
}
fn count(conn: &Connection, sql: &str) -> Result<u64> {
    Ok(conn
        .query_row(sql, [], |r| r.get::<_, i64>(0))?
        .try_into()?)
}
fn examples(conn: &Connection, sql: &str, columns: &[String], cap: usize) -> Result<Value> {
    if cap == 0 {
        return Ok(json!([]));
    }
    let projection = columns
        .iter()
        .map(|c| format!("substring(CAST({} AS VARCHAR), 1, 513)", ident(c)))
        .collect::<Vec<_>>()
        .join(",");
    let mut stmt = conn.prepare(&format!(
        "SELECT {projection} FROM ({sql}) AS examples LIMIT {cap}"
    ))?;
    let rows = stmt.query_map([], |r| {
        let mut values = Vec::new();
        for i in 0..columns.len() {
            let value = r.get::<_, Option<String>>(i)?.map(|v| {
                if v.chars().count() > 512 {
                    format!(
                        "{}…[truncated at 512 characters]",
                        v.chars().take(512).collect::<String>()
                    )
                } else {
                    v
                }
            });
            values.push(value);
        }
        Ok(values)
    })?;
    Ok(serde_json::to_value(
        rows.collect::<std::result::Result<Vec<_>, _>>()?,
    )?)
}
fn check(conn: &Connection, req: &Request, c: &Check) -> Result<Finding> {
    let input = req
        .inputs
        .iter()
        .find(|i| i.id == c.input)
        .context("unknown input id")?;
    conn.execute_batch(
        "DROP VIEW IF EXISTS actual; DROP VIEW IF EXISTS expected; DROP VIEW IF EXISTS scoped;",
    )?;
    conn.execute_batch(&format!(
        "CREATE VIEW actual AS SELECT * FROM {}",
        source(input)?
    ))?;
    let actual_schema = schema(conn, "actual")?;
    let mut conditions = Vec::new();
    if let Some(scope) = &c.scope {
        if !actual_schema.contains_key(&scope.column) {
            bail!("missing scope column {}", scope.column);
        }
        if scope.min.is_none() && scope.max.is_none() {
            bail!("scope requires min or max");
        }
        if let (Some(min), Some(max)) = (&scope.min, &scope.max) {
            let ty = &actual_schema[&scope.column];
            let reversed: bool = conn.query_row(
                &format!(
                    "SELECT CAST({} AS {ty}) > CAST({} AS {ty})",
                    literal(min),
                    literal(max)
                ),
                [],
                |r| r.get(0),
            )?;
            if reversed {
                bail!("scope minimum exceeds maximum");
            }
        }
        if let Some(min) = &scope.min {
            conditions.push(format!("{} >= {}", ident(&scope.column), literal(min)));
        }
        if let Some(max) = &scope.max {
            conditions.push(format!("{} <= {}", ident(&scope.column), literal(max)));
        }
    }
    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };
    conn.execute_batch(&format!(
        "CREATE VIEW scoped AS SELECT * FROM actual{where_clause}"
    ))?;
    // Bind filters even for schema checks, without scanning the full table.
    conn.prepare("SELECT * FROM scoped LIMIT 0")?;
    let missing_projection: Vec<_> = input
        .columns
        .iter()
        .filter(|x| !actual_schema.contains_key(*x))
        .cloned()
        .collect();
    let (status, reason, mut details) = match &c.kind {
        CheckKind::Schema { required } => {
            let mut missing = Vec::new();
            let mut wrong_types = Vec::new();
            for (column, ty) in required {
                match actual_schema.get(column) {
                    None => missing.push(column.clone()),
                    Some(actual) if ty != "*" && !actual.eq_ignore_ascii_case(ty) => {
                        wrong_types.push(json!({"column":column,"expected":ty,"actual":actual}))
                    }
                    _ => (),
                }
            }
            let valid =
                missing.is_empty() && wrong_types.is_empty() && missing_projection.is_empty();
            (
                if valid { "supported" } else { "mismatch" },
                "physical schema and declared projection inspected",
                json!({"physical_schema":actual_schema,"required":required,"missing_columns":missing,"type_mismatches":wrong_types,"missing_projection":missing_projection}),
            )
        }
        CheckKind::Key { columns } => {
            require(&actual_schema, columns)?;
            let keys = columns
                .iter()
                .map(|x| ident(x))
                .collect::<Vec<_>>()
                .join(",");
            let null_condition = columns
                .iter()
                .map(|x| format!("{} IS NULL", ident(x)))
                .collect::<Vec<_>>()
                .join(" OR ");
            let rows = count(conn, "SELECT count(*) FROM scoped")?;
            let null_rows = count(
                conn,
                &format!("SELECT count(*) FROM scoped WHERE {null_condition}"),
            )?;
            let mut count_name = "__local_data_count".to_string();
            while actual_schema.contains_key(&count_name) {
                count_name.push('_');
            }
            let count_alias = ident(&count_name);
            let groups =
                format!("SELECT {keys}, count(*) AS {count_alias} FROM scoped GROUP BY {keys}");
            let distinct_keys = count(conn, &format!("SELECT count(*) FROM ({groups}) AS g"))?;
            let duplicate_keys = count(
                conn,
                &format!("SELECT count(*) FROM ({groups}) AS g WHERE {count_alias} > 1"),
            )?;
            let duplicate_rows = count(
                conn,
                &format!(
                    "SELECT coalesce(sum({count_alias} - 1),0)::BIGINT FROM ({groups}) AS g WHERE {count_alias} > 1"
                ),
            )?;
            let sample = examples(
                conn,
                &format!(
                    "SELECT {keys} FROM ({groups}) AS g WHERE {count_alias} > 1 ORDER BY {keys}"
                ),
                columns,
                req.budgets.max_examples,
            )?;
            (
                if null_rows == 0 && duplicate_rows == 0 && missing_projection.is_empty() {
                    "supported"
                } else {
                    "mismatch"
                },
                "full scoped key check",
                json!({"row_count":rows,"distinct_key_count_including_null":distinct_keys,"null_key_rows":null_rows,"duplicate_key_count":duplicate_keys,"duplicate_excess_rows":duplicate_rows,"duplicate_examples":sample,"key_columns":columns,"missing_projection":missing_projection}),
            )
        }
        CheckKind::Coverage {
            columns,
            expected_input,
        } => {
            require(&actual_schema, columns)?;
            let expected = req
                .inputs
                .iter()
                .find(|i| i.id == *expected_input)
                .context("unknown expected input id")?;
            conn.execute_batch(&format!(
                "CREATE VIEW expected AS SELECT * FROM {}",
                source(expected)?
            ))?;
            let expected_schema = schema(conn, "expected")?;
            require(&expected_schema, columns)?;
            for col in columns {
                if actual_schema[col] != expected_schema[col] {
                    bail!(
                        "coverage key type mismatch for {col}: {} versus {}; provide explicitly normalized inputs",
                        actual_schema[col],
                        expected_schema[col]
                    );
                }
            }
            let keys = columns
                .iter()
                .map(|x| ident(x))
                .collect::<Vec<_>>()
                .join(",");
            let nulls = columns
                .iter()
                .map(|x| format!("{} IS NULL", ident(x)))
                .collect::<Vec<_>>()
                .join(" OR ");
            let expected_rows = count(conn, "SELECT count(*) FROM expected")?;
            let expected_nulls = count(
                conn,
                &format!("SELECT count(*) FROM expected WHERE {nulls}"),
            )?;
            let expected_keys = count(
                conn,
                &format!("SELECT count(*) FROM (SELECT DISTINCT {keys} FROM expected) AS e"),
            )?;
            let actual_rows = count(conn, "SELECT count(*) FROM scoped")?;
            let actual_keys = count(
                conn,
                &format!("SELECT count(*) FROM (SELECT DISTINCT {keys} FROM scoped) AS a"),
            )?;
            let missing_sql = format!(
                "SELECT DISTINCT {keys} FROM expected EXCEPT SELECT DISTINCT {keys} FROM scoped"
            );
            let missing = count(conn, &format!("SELECT count(*) FROM ({missing_sql}) AS m"))?;
            let sample = examples(
                conn,
                &format!("SELECT * FROM ({missing_sql}) AS m ORDER BY {keys}"),
                columns,
                req.budgets.max_examples,
            )?;
            let status = if expected_rows == 0 || expected_nulls > 0 {
                "unknown"
            } else if missing == 0 && missing_projection.is_empty() {
                "supported"
            } else {
                "mismatch"
            };
            (
                status,
                if expected_rows == 0 {
                    "expected universe is empty"
                } else if expected_nulls > 0 {
                    "expected universe contains null keys"
                } else {
                    "coverage against explicit expected universe"
                },
                json!({"expected_input":expected_input,"expected_row_count":expected_rows,"expected_distinct_key_count":expected_keys,"expected_null_key_rows":expected_nulls,"actual_row_count":actual_rows,"actual_distinct_key_count":actual_keys,"missing_distinct_key_count":missing,"covered_distinct_key_count":expected_keys-missing,"missing_examples":sample,"key_columns":columns,"expected_scope":"entire explicit expected input; actual scope does not filter expected universe","missing_projection":missing_projection}),
            )
        }
    };
    details["example_value_limit_characters"] = json!(512);
    details["scope"] = json!({"filter":c.scope,"extent":"full scoped input; no row sampling","intended_projection":input.columns});
    Ok(Finding {
        id: c.id.clone(),
        status: status.into(),
        origin: "measured".into(),
        reason: reason.into(),
        details,
        reused: false,
    })
}

fn unknown_check(c: &Check, reason: impl Into<String>) -> Finding {
    let mut finding = Finding::unknown(&c.id, reason);
    finding.details = json!({"input": c.input, "check": c.kind, "scope": {"filter": c.scope, "extent": "requested full scoped input; measurement incomplete"}});
    finding
}

pub fn run_checks(request: &Request, selected_ids: &[String]) -> Vec<Finding> {
    let selected: Vec<_> = request
        .checks
        .iter()
        .filter(|c| selected_ids.contains(&c.id))
        .collect();
    // A fully reusable handoff needs no database connection or temporary directory.
    if selected.is_empty() {
        return Vec::new();
    }
    let unknown_all = |reason: String| {
        selected
            .iter()
            .map(|c| unknown_check(c, reason.clone()))
            .collect()
    };
    if request.budgets.timeout_seconds == 0
        || request.budgets.memory_mb == 0
        || request.budgets.max_examples > 1000
    {
        return unknown_all(
            "invalid budget: positive timeout/memory and max_examples <= 1000 required".into(),
        );
    }
    let conn = match Connection::open_in_memory() {
        Ok(c) => c,
        Err(e) => return unknown_all(format!("DuckDB initialization failed: {e}")),
    };
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    let nonce = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let temp =
        std::env::temp_dir().join(format!("local-data-{}-{stamp}-{nonce}", std::process::id()));
    if let Err(e) = std::fs::create_dir(&temp) {
        return unknown_all(format!("temporary directory unavailable: {e}"));
    }
    let config = format!(
        "SET memory_limit='{}MB'; SET max_temp_directory_size='{}MB'; SET temp_directory={}; SET threads=2; SET autoinstall_known_extensions=false; SET autoload_known_extensions=false;",
        request.budgets.memory_mb,
        request.budgets.temp_mb,
        literal(&temp.to_string_lossy())
    );
    let result = match conn.execute_batch(&config) {
        Err(e) => unknown_all(format!("DuckDB configuration failed: {e}")),
        Ok(()) => {
            let timed_out = Arc::new(AtomicBool::new(false));
            let timeout_flag = timed_out.clone();
            let handle = conn.interrupt_handle();
            let (cancel_tx, cancel_rx) = mpsc::channel();
            let timeout = Duration::from_secs(request.budgets.timeout_seconds);
            let guard = std::thread::spawn(move || {
                if matches!(
                    cancel_rx.recv_timeout(timeout),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    timeout_flag.store(true, Ordering::SeqCst);
                    loop {
                        handle.interrupt();
                        if !matches!(
                            cancel_rx.recv_timeout(Duration::from_millis(10)),
                            Err(mpsc::RecvTimeoutError::Timeout)
                        ) {
                            break;
                        }
                    }
                }
            });
            let findings = selected
                .iter()
                .map(|c| {
                    if timed_out.load(Ordering::SeqCst) {
                        return unknown_check(c, "overall check time budget exceeded");
                    }
                    let measured = check(&conn, request, c);
                    if timed_out.load(Ordering::SeqCst) {
                        unknown_check(
                            c,
                            "overall check time budget exceeded; incomplete measurements discarded",
                        )
                    } else {
                        measured.unwrap_or_else(|e| {
                            unknown_check(c, format!("check could not be completed: {e:#}"))
                        })
                    }
                })
                .collect();
            let _ = cancel_tx.send(());
            let _ = guard.join();
            findings
        }
    };
    drop(conn);
    let _ = std::fs::remove_dir_all(temp);
    result
}
