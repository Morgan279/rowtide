//! Independent comparator: explicit requests, direct DuckDB SQL, strong per-check cache.
//! This example deliberately does not use the product's workflow or engine.
use anyhow::{Context, Result, bail};
use duckdb::Connection;
use rowtide::model::{Check, CheckKind, Input, Request};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::Instant,
};

fn ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}
fn lit(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn resolve(base: &Path, s: &str) -> PathBuf {
    let p = Path::new(s);
    if p.is_absolute() {
        p.into()
    } else {
        base.join(p)
    }
}
fn hash_file(path: &Path) -> Result<String> {
    let before = fs::metadata(path)?;
    if !before.is_file() {
        bail!("not a regular file: {}", path.display());
    }
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0; 1024 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let after = fs::metadata(path)?;
    if before.len() != after.len() || before.modified()? != after.modified()? {
        bail!("file changed during hashing: {}", path.display());
    }
    Ok(format!("{:x}", hasher.finalize()))
}
fn source(i: &Input) -> Result<String> {
    if i.path.chars().any(|c| matches!(c, '*' | '?' | '[' | ']')) {
        bail!("glob paths unsupported");
    }
    if !Path::new(&i.path).is_file() {
        bail!("unavailable input {}", i.id);
    }
    match i.format.as_str() {
        "parquet" => Ok(format!("read_parquet({})", lit(&i.path))),
        "csv" => Ok(format!(
            "read_csv({}, header=true, auto_detect=true, strict_mode=true, encoding='utf-8')",
            lit(&i.path)
        )),
        _ => bail!("unsupported format {}", i.format),
    }
}
fn count(conn: &Connection, sql: &str) -> Result<u64> {
    Ok(conn
        .query_row(sql, [], |r| r.get::<_, i64>(0))?
        .try_into()?)
}
fn schema(conn: &Connection, src: &str) -> Result<BTreeMap<String, String>> {
    let mut stmt = conn.prepare(&format!("DESCRIBE SELECT * FROM {src}"))?;
    Ok(stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<std::result::Result<_, _>>()?)
}
fn columns_exist(s: &BTreeMap<String, String>, columns: &[String]) -> Result<()> {
    if columns.is_empty() {
        bail!("empty key");
    }
    if columns
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        != columns.len()
    {
        bail!("repeated key column");
    }
    for c in columns {
        if !s.contains_key(c) {
            bail!("missing column {c}");
        }
    }
    Ok(())
}
fn run_check(conn: &Connection, req: &Request, check: &Check) -> Result<Value> {
    let input = req
        .inputs
        .iter()
        .find(|i| i.id == check.input)
        .context("unknown input id")?;
    let src = source(input)?;
    let actual_schema = schema(conn, &src)?;
    let missing_projection: Vec<_> = input
        .columns
        .iter()
        .filter(|c| !actual_schema.contains_key(*c))
        .collect();
    let mut conditions = vec![];
    if let Some(scope) = &check.scope {
        let ty = actual_schema
            .get(&scope.column)
            .context("missing scope column")?;
        if scope.min.is_none() && scope.max.is_none() {
            bail!("empty scope");
        }
        if let (Some(min), Some(max)) = (&scope.min, &scope.max) {
            let reverse: bool = conn.query_row(
                &format!(
                    "SELECT CAST({} AS {ty}) > CAST({} AS {ty})",
                    lit(min),
                    lit(max)
                ),
                [],
                |r| r.get(0),
            )?;
            if reverse {
                bail!("reversed scope");
            }
        }
        if let Some(v) = &scope.min {
            conditions.push(format!("{} >= {}", ident(&scope.column), lit(v)));
        }
        if let Some(v) = &scope.max {
            conditions.push(format!("{} <= {}", ident(&scope.column), lit(v)));
        }
    }
    let clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };
    let actual = format!("(SELECT * FROM {src}{clause})");
    conn.prepare(&format!("SELECT * FROM {actual} LIMIT 0"))?;
    let (status, details) = match &check.kind {
        CheckKind::Schema { required } => {
            let missing: Vec<_> = required
                .keys()
                .filter(|c| !actual_schema.contains_key(*c))
                .collect();
            let wrong: Vec<_> = required
                .iter()
                .filter_map(|(c, t)| {
                    actual_schema
                        .get(c)
                        .filter(|a| t != "*" && !a.eq_ignore_ascii_case(t))
                        .map(|a| json!({"column":c,"expected":t,"actual":a}))
                })
                .collect();
            (
                if missing.is_empty() && wrong.is_empty() && missing_projection.is_empty() {
                    "supported"
                } else {
                    "mismatch"
                },
                json!({"physical_schema":actual_schema,"missing_columns":missing,"type_mismatches":wrong,"missing_projection":missing_projection}),
            )
        }
        CheckKind::Key { columns } => {
            columns_exist(&actual_schema, columns)?;
            let keys = columns
                .iter()
                .map(|c| ident(c))
                .collect::<Vec<_>>()
                .join(",");
            let nulls = columns
                .iter()
                .map(|c| format!("{} IS NULL", ident(c)))
                .collect::<Vec<_>>()
                .join(" OR ");
            let rows = count(conn, &format!("SELECT count(*) FROM {actual}"))?;
            let null_rows = count(
                conn,
                &format!("SELECT count(*) FROM {actual} WHERE {nulls}"),
            )?;
            let distinct = count(
                conn,
                &format!("SELECT count(*) FROM (SELECT DISTINCT {keys} FROM {actual})"),
            )?;
            // Duplicate excess = row count - distinct tuple count, including null tuples.
            let duplicates = rows - distinct;
            (
                if null_rows == 0 && duplicates == 0 && missing_projection.is_empty() {
                    "supported"
                } else {
                    "mismatch"
                },
                json!({"row_count":rows,"distinct_key_count_including_null":distinct,"duplicate_excess_rows":duplicates,"null_key_rows":null_rows,"missing_projection":missing_projection}),
            )
        }
        CheckKind::Coverage {
            columns,
            expected_input,
        } => {
            columns_exist(&actual_schema, columns)?;
            let expected = req
                .inputs
                .iter()
                .find(|i| i.id == *expected_input)
                .context("unknown expected input")?;
            let exp = source(expected)?;
            let expected_schema = schema(conn, &exp)?;
            columns_exist(&expected_schema, columns)?;
            for c in columns {
                if actual_schema[c] != expected_schema[c] {
                    bail!("coverage type mismatch for {c}");
                }
            }
            let keys = columns
                .iter()
                .map(|c| ident(c))
                .collect::<Vec<_>>()
                .join(",");
            let nulls = columns
                .iter()
                .map(|c| format!("{} IS NULL", ident(c)))
                .collect::<Vec<_>>()
                .join(" OR ");
            let expected_rows = count(conn, &format!("SELECT count(*) FROM {exp}"))?;
            let expected_nulls = count(conn, &format!("SELECT count(*) FROM {exp} WHERE {nulls}"))?;
            let expected_count = count(
                conn,
                &format!("SELECT count(*) FROM (SELECT DISTINCT {keys} FROM {exp})"),
            )?;
            let actual_rows = count(conn, &format!("SELECT count(*) FROM {actual}"))?;
            let actual_count = count(
                conn,
                &format!("SELECT count(*) FROM (SELECT DISTINCT {keys} FROM {actual})"),
            )?;
            let missing = count(
                conn,
                &format!(
                    "SELECT count(*) FROM (SELECT DISTINCT {keys} FROM {exp} EXCEPT SELECT DISTINCT {keys} FROM {actual})"
                ),
            )?;
            (
                if expected_rows == 0 || expected_nulls > 0 {
                    "unknown"
                } else if missing == 0 && missing_projection.is_empty() {
                    "supported"
                } else {
                    "mismatch"
                },
                json!({"expected_row_count":expected_rows,"expected_distinct_key_count":expected_count,"expected_null_key_rows":expected_nulls,"actual_row_count":actual_rows,"actual_distinct_key_count":actual_count,"missing_distinct_key_count":missing,"covered_distinct_key_count":expected_count-missing,"missing_projection":missing_projection}),
            )
        }
    };
    Ok(json!({"id":check.id,"status":status,"details":details,"reused":false}))
}
fn main() -> Result<()> {
    let wall = Instant::now();
    let mut args = std::env::args().skip(1);
    let request_path = fs::canonicalize(
        args.next()
            .context("usage: direct_duckdb_baseline REQUEST_JSON [--cache PREVIOUS_JSON]")?,
    )?;
    let cache_path = match args.next() {
        None => None,
        Some(s) if s == "--cache" => Some(args.next().context("--cache needs path")?),
        Some(_) => bail!("unexpected argument"),
    };
    if args.next().is_some() {
        bail!("unexpected trailing argument");
    }
    let previous: Option<Value> = cache_path
        .map(|p| -> Result<_> { Ok(serde_json::from_slice(&fs::read(p)?)?) })
        .transpose()?;
    let mut req: Request = serde_json::from_slice(&fs::read(&request_path)?)?;
    let base = request_path.parent().context("request parent")?;
    for input in &mut req.inputs {
        input.path = resolve(base, &input.path).to_string_lossy().into();
        if let Some(reader) = &mut input.reader {
            reader.path = resolve(base, &reader.path).to_string_lossy().into();
        }
    }
    let binding_started = Instant::now();
    let contract = req
        .contract
        .as_ref()
        .map(|p| hash_file(&resolve(base, p)))
        .transpose();
    let mut bindings: BTreeMap<String, Value> = BTreeMap::new();
    let mut metadata_before = BTreeMap::new();
    for input in &req.inputs {
        let hash = hash_file(Path::new(&input.path));
        let reader = input
            .reader
            .as_ref()
            .map(|r| hash_file(Path::new(&r.path)))
            .transpose();
        if let Ok(m) = fs::metadata(&input.path) {
            metadata_before.insert(input.id.clone(), (m.len(), m.modified().ok()));
        }
        match (hash, reader) {
            (Ok(data), Ok(reader)) => {
                bindings.insert(
                    input.id.clone(),
                    json!({"input":input,"sha256":data,"reader_sha256":reader}),
                );
            }
            (data, reader) => {
                bindings.insert(
                    input.id.clone(),
                    json!({"error":format!("data: {data:?}; reader: {reader:?}")}),
                );
            }
        }
    }
    let binding_ms = binding_started.elapsed().as_secs_f64() * 1000.;
    let conn_started = Instant::now();
    let conn = Connection::open_in_memory()?;
    conn.execute_batch(&format!(
        "SET memory_limit='{}MB'; SET max_temp_directory_size='{}MB'; SET threads=1",
        req.budgets.memory_mb, req.budgets.temp_mb
    ))?;
    let connection_ms = conn_started.elapsed().as_secs_f64() * 1000.;
    let mut findings = vec![];
    let mut sql_ms = 0.;
    let mut reused_count = 0;
    for check in &req.checks {
        let mut relevant = vec![check.input.clone()];
        if let CheckKind::Coverage { expected_input, .. } = &check.kind {
            relevant.push(expected_input.clone());
        }
        let values: Vec<_> = relevant.iter().map(|id| bindings.get(id)).collect();
        let valid_binding = contract.is_ok()
            && values
                .iter()
                .all(|v| v.is_some_and(|v| v.get("error").is_none()));
        let fingerprint = digest(&serde_json::to_vec(
            &json!({"baseline_version":digest(include_bytes!("direct_duckdb_baseline.rs")),"check":check,"bindings":values,"contract":contract.as_ref().ok()}),
        )?);
        let cached = previous
            .as_ref()
            .and_then(|p| p["findings"].as_array())
            .and_then(|rows| {
                rows.iter().find(|v| {
                    v["id"] == check.id
                        && v["fingerprint"] == fingerprint
                        && v["status"] != "unknown"
                })
            });
        let mut finding = if valid_binding {
            if let Some(value) = cached {
                let mut v = value.clone();
                v["reused"] = json!(true);
                reused_count += 1;
                v
            } else {
                let start = Instant::now();
                let result = run_check(&conn, &req, check);
                sql_ms += start.elapsed().as_secs_f64() * 1000.;
                result.unwrap_or_else(|e| json!({"id":check.id,"status":"unknown","reason":e.to_string(),"reused":false}))
            }
        } else {
            json!({"id":check.id,"status":"unknown","reason":"strong binding unavailable","reused":false})
        };
        finding["fingerprint"] = json!(fingerprint);
        findings.push(finding);
    }
    let changed: Vec<_> = req
        .inputs
        .iter()
        .filter(|i| {
            fs::metadata(&i.path)
                .ok()
                .map(|m| (m.len(), m.modified().ok()))
                != metadata_before.get(&i.id).cloned()
        })
        .map(|i| i.id.clone())
        .collect();
    if !changed.is_empty() {
        for finding in &mut findings {
            finding["status"] = json!("unknown");
            finding["reason"] = json!("input metadata changed during run");
            finding["reused"] = json!(false);
        }
        reused_count = 0;
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"comparator":"independent direct DuckDB SQL with strong per-check cache", "bindings":bindings,"findings":findings,"changed_during_run":changed,"timings_ms":{"binding":binding_ms,"connection":connection_ms,"sql":sql_ms,"total":wall.elapsed().as_secs_f64()*1000.},"reused_count":reused_count,"limitations":["ordinary files are not transactional snapshots","baseline does not evaluate evidence declarations or observed consumption","no performance claim about human effort"]})
        )?
    );
    Ok(())
}
