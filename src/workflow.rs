use crate::model::*;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{Read, Write},
    path::Path,
    time::UNIX_EPOCH,
};

fn absolute(base: &Path, path: &str) -> String {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    }
    .to_string_lossy()
    .into_owned()
}
fn document(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 8 * 1024 * 1024 {
        bail!("document exceeds 8 MiB limit")
    }
    Ok(bytes)
}
pub fn load_request(path: &Path) -> Result<Request> {
    let mut r: Request = serde_json::from_slice(&document(path)?).context("invalid request")?;
    let base = fs::canonicalize(path)?.parent().unwrap().to_path_buf();
    for i in &mut r.inputs {
        i.path = absolute(&base, &i.path);
        if let Some(reader) = &mut i.reader {
            reader.path = absolute(&base, &reader.path);
        }
    }
    for e in &mut r.evidence {
        e.path = absolute(&base, &e.path);
    }
    if let Some(c) = &mut r.contract {
        *c = absolute(&base, c);
    }
    validate(&r)?;
    Ok(r)
}
fn unique<'a>(items: impl Iterator<Item = &'a str>, label: &str) -> Result<()> {
    let mut ids = BTreeSet::new();
    for id in items {
        if id.is_empty() || id.contains(':') || !ids.insert(id) {
            bail!("{label}: empty, duplicate or colon-containing id {id:?}")
        }
    }
    Ok(())
}
pub fn validate(r: &Request) -> Result<()> {
    if r.task.is_empty() || r.purpose.is_empty() || r.inputs.is_empty() {
        bail!("task, purpose and at least one input required")
    }
    unique(r.inputs.iter().map(|x| x.id.as_str()), "input")?;
    unique(
        r.checks
            .iter()
            .map(|x| x.id.as_str())
            .chain(r.declarations.iter().map(|x| x.id.as_str())),
        "requirement",
    )?;
    unique(r.evidence.iter().map(|x| x.id.as_str()), "evidence")?;
    let inputs: BTreeSet<_> = r.inputs.iter().map(|x| &x.id).collect();
    let evidence: BTreeSet<_> = r.evidence.iter().map(|x| &x.id).collect();
    for i in &r.inputs {
        if i.role.is_empty() {
            bail!("input {} needs role", i.id)
        }
        unique(i.columns.iter().map(String::as_str), "columns")?;
    }
    for c in &r.checks {
        if !inputs.contains(&c.input) {
            bail!("check {} references missing input", c.id)
        }
        match &c.kind {
            CheckKind::Key { columns } | CheckKind::Coverage { columns, .. } => {
                if columns.is_empty() {
                    bail!("check {} needs keys", c.id)
                }
                unique(columns.iter().map(String::as_str), "check columns")?;
            }
            _ => {}
        }
        if let CheckKind::Coverage { expected_input, .. } = &c.kind {
            if !inputs.contains(expected_input) {
                bail!("missing expected input")
            }
        }
    }
    for e in &r.evidence {
        if !["source_declared", "agent_proposed"].contains(&e.origin.as_str()) {
            bail!("evidence origin must be source_declared or agent_proposed")
        }
        if e.pointer.is_some() == e.excerpt.is_some() {
            bail!("evidence {} needs exactly one pointer or excerpt", e.id)
        }
        if e.scope.is_empty() {
            bail!("evidence scope required")
        }
        if let Some(p) = &e.pointer {
            if !p.is_empty() && !p.starts_with('/') {
                bail!("invalid JSON pointer")
            }
        }
        for (id, hash) in &e.input_sha256 {
            if !inputs.contains(id)
                || hash.len() != 64
                || !hash.bytes().all(|b| b.is_ascii_hexdigit())
            {
                bail!("invalid evidence input binding")
            }
        }
    }
    for d in &r.declarations {
        if !inputs.contains(&d.input) || d.evidence.iter().any(|x| !evidence.contains(x)) {
            bail!("declaration {} invalid reference", d.id)
        }
    }
    if r.budgets.timeout_seconds == 0
        || r.budgets.timeout_seconds > 86400
        || r.budgets.memory_mb < 16
        || r.budgets.max_examples > 100
    {
        bail!("invalid budgets: timeout 1..86400s, memory >=16MB, examples <=100")
    }
    Ok(())
}
fn stat(path: &Path) -> Result<(u64, u128)> {
    let m = fs::metadata(path)?;
    if !m.is_file() {
        bail!("not a regular file")
    }
    Ok((
        m.len(),
        m.modified()?.duration_since(UNIX_EPOCH)?.as_nanos(),
    ))
}
pub fn bind(path: &str, budget: &mut u64) -> Binding {
    let mut b = Binding {
        path: path.into(),
        sha256: None,
        bytes: None,
        modified_ns: None,
        state: "unavailable".into(),
        reason: String::new(),
    };
    let result = (|| -> Result<()> {
        let before = stat(Path::new(path))?;
        b.bytes = Some(before.0);
        b.modified_ns = Some(before.1);
        if before.0 > *budget {
            b.state = "unbound".into();
            b.reason = "content hash budget exhausted; stat is only a hint".into();
            return Ok(());
        }
        let mut file = File::open(path)?;
        let mut hash = Sha256::new();
        let mut buf = [0u8; 65536];
        let mut total = 0u64;
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            total += n as u64;
            if n as u64 > *budget {
                *budget = 0;
                b.state = "unbound".into();
                b.reason = "file grew beyond hash budget".into();
                return Ok(());
            }
            *budget -= n as u64;
            hash.update(&buf[..n]);
        }
        if before != stat(Path::new(path))? || total != before.0 {
            b.state = "changed".into();
            b.reason = "file changed while hashing".into();
            return Ok(());
        }
        b.sha256 = Some(format!("{:x}", hash.finalize()));
        b.state = "bound".into();
        b.reason = "full SHA-256 of observed file; not a transactional snapshot".into();
        Ok(())
    })();
    if let Err(e) = result {
        b.reason = e.to_string();
    }
    b
}
fn bindings(r: &Request) -> BTreeMap<String, Binding> {
    let mut remaining = r.budgets.hash_bytes;
    let mut map = BTreeMap::new();
    for i in &r.inputs {
        map.insert(format!("input:{}", i.id), bind(&i.path, &mut remaining));
        if let Some(reader) = &i.reader {
            map.insert(
                format!("reader:{}", i.id),
                bind(&reader.path, &mut remaining),
            );
        }
    }
    for e in &r.evidence {
        map.insert(format!("evidence:{}", e.id), bind(&e.path, &mut remaining));
    }
    if let Some(c) = &r.contract {
        map.insert("contract".into(), bind(c, &mut remaining));
    }
    map
}
fn input_dependencies(c: &Check) -> Vec<String> {
    let mut ids = vec![c.input.clone()];
    if let CheckKind::Coverage { expected_input, .. } = &c.kind {
        ids.push(expected_input.clone());
    }
    ids
}
fn equal_content(a: Option<&Binding>, b: Option<&Binding>) -> bool {
    matches!((a,b),(Some(a),Some(b)) if a.sha256.is_some() && a.sha256==b.sha256 && a.state=="bound" && b.state=="bound")
}
fn reuse_reason(
    c: &Check,
    r: &Request,
    old: Option<&Receipt>,
    b: &BTreeMap<String, Binding>,
) -> (&'static str, &'static str) {
    let Some(old) = old else {
        return ("first_run", "No previous handoff; run the requested check.");
    };
    if old.schema_version != 1 || old.engine_version != ENGINE_VERSION {
        return (
            "implementation_changed",
            "Receipt schema or check implementation changed.",
        );
    }
    if old.request.task != r.task
        || old.request.purpose != r.purpose
        || old.request.contract != r.contract
        || (r.contract.is_some() && !equal_content(b.get("contract"), old.bindings.get("contract")))
    {
        return (
            "task_or_contract_changed",
            "Task, purpose or contract identity changed or is unbound.",
        );
    }
    if !old.request.checks.contains(c) {
        return ("check_changed", "Requested condition or scope changed.");
    }
    for id in input_dependencies(c) {
        let now = r.inputs.iter().find(|i| i.id == id).unwrap();
        let Some(before) = old.request.inputs.iter().find(|i| i.id == id) else {
            return (
                "input_config_changed",
                "Input identity or reader configuration changed.",
            );
        };
        let mut now = now.clone();
        now.path = before.path.clone();
        if &now != before {
            return (
                "input_config_changed",
                "Input role, projection, format or reader configuration changed.",
            );
        }
        if !equal_content(
            b.get(&format!("input:{id}")),
            old.bindings.get(&format!("input:{id}")),
        ) {
            return (
                "input_content_changed_or_unbound",
                "Input content changed or lacks stable content identity.",
            );
        }
        if now.reader.is_some()
            && !equal_content(
                b.get(&format!("reader:{id}")),
                old.bindings.get(&format!("reader:{id}")),
            )
        {
            return (
                "reader_content_changed_or_unbound",
                "Reader content changed or lacks stable content identity.",
            );
        }
    }
    if !old
        .findings
        .iter()
        .any(|f| f.id == c.id && f.status != "unknown" && f.origin == "measured")
    {
        return (
            "no_prior_result",
            "No completed measured finding is available for this check.",
        );
    }
    (
        "dependencies_unchanged",
        "Implementation, requested condition and strongly bound dependencies match.",
    )
}
fn set_reuse(f: &mut Finding, code: &str, explanation: &str) {
    if !f.details.is_object() {
        f.details = json!({});
    }
    f.details["reuse"] = json!({"decision":if f.reused {"reused"} else {"rechecked"},"reason_code":code,"explanation":explanation});
}
fn declaration(d: &Declaration, r: &Request, b: &BTreeMap<String, Binding>) -> Finding {
    let mut observations = Vec::new();
    let mut values = Vec::new();
    let mut all_valid = !d.evidence.is_empty();
    for id in &d.evidence {
        let e = r.evidence.iter().find(|x| &x.id == id).unwrap();
        let eb = &b[&format!("evidence:{id}")];
        let ib = &b[&format!("input:{}", d.input)];
        let mut valid = eb.state == "bound"
            && e.origin == "source_declared"
            && ib.sha256.is_some()
            && e.input_sha256.get(&d.input) == ib.sha256.as_ref();
        let value = (|| -> Result<Value> {
            // Avoid unbounded document reads even when the data hash budget is large.
            if eb.bytes.unwrap_or(u64::MAX) > 8 * 1024 * 1024 {
                bail!("evidence exceeds 8 MiB document limit")
            }
            let bytes = document(Path::new(&e.path))?;
            if let Some(p) = &e.pointer {
                let v: Value = serde_json::from_slice(&bytes)?;
                return v.pointer(p).cloned().context("pointer missing");
            }
            let text = String::from_utf8(bytes)?;
            let excerpt = e.excerpt.as_ref().unwrap();
            if excerpt.is_empty() || !text.contains(excerpt) {
                bail!("excerpt missing or empty")
            }
            Ok(Value::String(excerpt.clone()))
        })();
        match value {
            Ok(v) => {
                values.push(v.clone());
                observations.push(json!({"evidence":id,"scope":e.scope,"origin":e.origin,"value":v,"bound_to_input":valid}));
            }
            Err(err) => {
                valid = false;
                observations.push(json!({"evidence":id,"error":err.to_string()}));
            }
        }
        all_valid &= valid;
    }
    let conflict = values.windows(2).any(|v| v[0] != v[1]);
    let (status, reason) = if conflict {
        (
            "unknown",
            "source values conflict; no latest-wins resolution",
        )
    } else if !all_valid {
        (
            "unknown",
            "missing, proposed or unbound source evidence; historical description is not current certification",
        )
    } else if values.first() == Some(&d.expected) {
        (
            "supported",
            "explicit source value matches, bound to this input; source declaration, not independent measurement",
        )
    } else {
        (
            "mismatch",
            "bound source value differs from requested value",
        )
    };
    Finding {
        id: d.id.clone(),
        status: status.into(),
        origin: "source_declared".into(),
        reason: reason.into(),
        details: json!({"description":d.description,"expected":d.expected,"observations":observations}),
        reused: false,
    }
}
pub fn prepare(r: Request, previous: Option<(&Path, &Receipt)>) -> Receipt {
    let mut b = bindings(&r);
    let mut findings = Vec::new();
    let mut selected = Vec::new();
    let mut freshness = BTreeMap::new();
    for (id, current) in &b {
        let state = if current.state == "unavailable" {
            "unavailable"
        } else if current.state == "changed" {
            "changed"
        } else if current.sha256.is_none() {
            "unbound"
        } else if let Some((_, old)) = previous {
            if equal_content(Some(current), old.bindings.get(id)) {
                "unchanged"
            } else if old.bindings.get(id).is_some_and(|x| x.sha256.is_some()) {
                "changed"
            } else {
                "unbound"
            }
        } else {
            "unbound"
        };
        freshness.insert(id.clone(), state.into());
    }
    for c in &r.checks {
        if let Some((_, old)) = previous {
            if reuse_reason(c, &r, Some(old), &b).0 == "dependencies_unchanged" {
                let mut f = old.findings.iter().find(|x| x.id == c.id).unwrap().clone();
                f.reused = true;
                findings.push(f);
                continue;
            }
        }
        selected.push(c.id.clone());
    }
    findings.extend(crate::engine::run_checks(&r, &selected));
    for d in &r.declarations {
        findings.push(declaration(d, &r, &b));
    }
    // Changed or inaccessible sources invalidate dependent conclusions, even on a cache hit.
    let mut changed = BTreeSet::new();
    for (id, binding) in &mut b {
        if let (Some(size), Some(time)) = (binding.bytes, binding.modified_ns) {
            if stat(Path::new(&binding.path)).ok() != Some((size, time)) {
                binding.sha256 = None;
                binding.state = "changed".into();
                binding.reason = "file changed or disappeared during preparation".into();
                freshness.insert(id.clone(), "changed".into());
                changed.insert(id.clone());
            }
        }
    }
    for f in &mut findings {
        let mut deps = if let Some(c) = r.checks.iter().find(|x| x.id == f.id) {
            input_dependencies(c)
                .into_iter()
                .flat_map(|id| vec![format!("input:{id}"), format!("reader:{id}")])
                .collect::<Vec<_>>()
        } else {
            let d = r.declarations.iter().find(|x| x.id == f.id).unwrap();
            let mut deps = vec![format!("input:{}", d.input)];
            deps.extend(d.evidence.iter().map(|id| format!("evidence:{id}")));
            deps
        };
        if r.contract.is_some() {
            deps.push("contract".into());
        }
        if deps.iter().any(|id| changed.contains(id)) {
            f.status = "unknown".into();
            f.reason = "dependency changed during preparation".into();
            f.reused = false;
        }
        // A measured finding without strong identity remains an observation, never reusable certification.
        if f.origin == "measured"
            && deps
                .iter()
                .any(|id| b.get(id).is_some_and(|x| x.state != "bound"))
        {
            f.status = "unknown".into();
            f.reason = "dependency lacks stable content identity; measured details retained".into();
            f.reused = false;
        }
    }
    for f in &mut findings {
        if let Some(c) = r.checks.iter().find(|c| c.id == f.id) {
            let (code, explanation) = reuse_reason(c, &r, previous.map(|(_, old)| old), &b);
            set_reuse(f, code, explanation);
        }
    }
    findings.sort_by(|a, b| a.id.cmp(&b.id));
    let next_actions = finding_actions(&findings);
    let execution = if findings.iter().any(|f| f.status == "unknown")
        || b.values().any(|x| x.state != "bound")
    {
        "partial"
    } else {
        "completed"
    };
    Receipt{schema_version:1,engine_version:ENGINE_VERSION.into(),operation:if previous.is_some(){"refresh"}else{"prepare"}.into(),execution:execution.into(),request:r,bindings:b,findings,freshness,consumption:Vec::new(),next_actions,limitations:vec!["Static local exports only; pre/post metadata checks do not provide a transactional snapshot.".into(),"Supported findings apply only to named requirements and stated scope. Domain-specific correctness remains unverified.".into(),"Receipts describe intended inputs; external consumers are not controlled by this tool.".into()],previous:previous.map(|(p,_)|p.to_string_lossy().into())}
}
fn finding_actions(findings: &[Finding]) -> Vec<String> {
    findings
        .iter()
        .filter(|f| f.status == "unknown" || f.status == "mismatch")
        .map(|f| format!("Review {} ({}): {}", f.id, f.status, f.reason))
        .collect()
}
pub fn load_receipt(path: &Path) -> Result<Receipt> {
    let r: Receipt = serde_json::from_slice(&document(path)?)?;
    if r.schema_version != 1 {
        bail!("unsupported receipt version")
    }
    validate(&r.request)?;
    let mut needed = Vec::new();
    for i in &r.request.inputs {
        needed.push(format!("input:{}", i.id));
        if i.reader.is_some() {
            needed.push(format!("reader:{}", i.id));
        }
    }
    for e in &r.request.evidence {
        needed.push(format!("evidence:{}", e.id));
    }
    if r.request.contract.is_some() {
        needed.push("contract".into());
    }
    if needed.iter().any(|k| !r.bindings.contains_key(k)) {
        bail!("receipt is missing dependency bindings")
    }
    unique(r.findings.iter().map(|f| f.id.as_str()), "receipt findings")?;
    let ids: BTreeSet<_> = r
        .request
        .checks
        .iter()
        .map(|c| c.id.as_str())
        .chain(r.request.declarations.iter().map(|d| d.id.as_str()))
        .collect();
    if r.findings.iter().any(|f| {
        !ids.contains(f.id.as_str())
            || !["supported", "mismatch", "unknown"].contains(&f.status.as_str())
    }) {
        bail!("invalid receipt findings")
    }
    for b in r.bindings.values() {
        if b.state == "bound"
            && (b
                .sha256
                .as_ref()
                .is_none_or(|h| h.len() != 64 || !h.bytes().all(|c| c.is_ascii_hexdigit()))
                || b.bytes.is_none()
                || b.modified_ns.is_none())
        {
            bail!("invalid bound identity")
        }
    }
    Ok(r)
}
pub fn record(previous_path: &Path, old: &Receipt, mapping_path: &Path) -> Result<Receipt> {
    let actual: ActualReceipt = serde_json::from_slice(&document(mapping_path)?)?;
    unique(
        actual.mappings.iter().map(|m| m.input.as_str()),
        "consumption mapping",
    )?;
    if actual
        .mappings
        .iter()
        .any(|m| !old.request.inputs.iter().any(|i| i.id == m.input))
    {
        bail!("consumption mapping references unknown input")
    }
    let base = fs::canonicalize(mapping_path)?
        .parent()
        .unwrap()
        .to_path_buf();
    let source = absolute(&base, &actual.source);
    let mut budget = old.request.budgets.hash_bytes;
    let source_binding = bind(&source, &mut budget);
    if source_binding.bytes.unwrap_or(u64::MAX) > 8 * 1024 * 1024 {
        bail!("actual receipt missing or exceeds 8 MiB")
    }
    let source_json: Value = serde_json::from_slice(&document(Path::new(&source))?)
        .context("actual receipt must be JSON")?;
    let mut result = old.clone();
    result.operation = "record".into();
    result.previous = Some(previous_path.to_string_lossy().into());
    result.consumption.clear();
    result
        .bindings
        .insert("actual_receipt".into(), source_binding.clone());
    result.bindings.insert(
        "actual_mapping".into(),
        bind(mapping_path.to_string_lossy().as_ref(), &mut budget),
    );
    for input in &old.request.inputs {
        let mut missing = Vec::new();
        let mut mismatches = Vec::new();
        let mut observed = serde_json::Map::new();
        let expected = &old.bindings[&format!("input:{}", input.id)];
        if let Some(m) = actual.mappings.iter().find(|m| m.input == input.id) {
            let path = source_json.pointer(&m.path_pointer).and_then(Value::as_str);
            observed.insert("path".into(), json!(path));
            if path.is_none() {
                missing.push("path")
            } // Content identity permits relocation.
            let hash = m
                .sha256_pointer
                .as_ref()
                .and_then(|p| source_json.pointer(p))
                .and_then(Value::as_str);
            observed.insert("sha256".into(), json!(hash));
            match (expected.sha256.as_deref(), hash) {
                (Some(a), Some(b)) if a != b => mismatches.push("sha256"),
                (Some(_), Some(_)) => {}
                _ => missing.push("sha256"),
            }
            let columns = m
                .columns_pointer
                .as_ref()
                .and_then(|p| source_json.pointer(p));
            observed.insert("columns".into(), json!(columns));
            if !input.columns.is_empty() {
                match columns {
                    Some(c) if c == &json!(input.columns) => {}
                    Some(_) => mismatches.push("columns"),
                    None => missing.push("columns"),
                }
            }
            let role = m.role_pointer.as_ref().and_then(|p| source_json.pointer(p));
            match role {
                Some(v) if v == &json!(input.role) => {}
                Some(_) => mismatches.push("role"),
                None => missing.push("role"),
            }
            if let Some(reader) = &input.reader {
                let rh = m
                    .reader_sha256_pointer
                    .as_ref()
                    .and_then(|p| source_json.pointer(p))
                    .and_then(Value::as_str);
                let wanted = old
                    .bindings
                    .get(&format!("reader:{}", input.id))
                    .and_then(|b| b.sha256.as_deref());
                match (wanted, rh) {
                    (Some(a), Some(b)) if a != b => mismatches.push("reader_sha256"),
                    (Some(_), Some(_)) => {}
                    _ => missing.push("reader_sha256"),
                }
                match m
                    .reader_parameters_pointer
                    .as_ref()
                    .and_then(|p| source_json.pointer(p))
                {
                    Some(v) if v == &json!(reader.parameters) => {}
                    Some(_) => mismatches.push("reader_parameters"),
                    None => missing.push("reader_parameters"),
                }
            }
        } else {
            missing.push("mapping")
        }
        if source_binding.state != "bound" {
            missing.push("actual_receipt_binding")
        }
        let status = if !mismatches.is_empty() {
            "mismatched"
        } else if !missing.is_empty() {
            "unknown"
        } else {
            "matched"
        };
        result.consumption.push(Finding{id:input.id.clone(),status:status.into(),origin:"source_declared".into(),reason:format!("Native receipt comparison: missing={missing:?}; different={mismatches:?}"),details:json!({"source":source,"observed":observed,"missing":missing,"mismatches":mismatches,"mapping":actual.mappings.iter().find(|m|m.input==input.id)}),reused:false});
    }
    if stat(Path::new(&source)).ok() != source_binding.bytes.zip(source_binding.modified_ns) {
        for f in &mut result.consumption {
            f.status = "unknown".into();
            f.reason = "actual receipt changed during comparison".into();
        }
    }
    result.execution = if result.consumption.iter().any(|f| f.status == "unknown")
        || old.execution != "completed"
    {
        "partial"
    } else {
        "completed"
    }
    .into();
    // Rebuild current guidance: prior consumption and caller notes are historical,
    // whereas unresolved handoff findings still apply to this comparison.
    result.next_actions = finding_actions(&result.findings);
    for f in &result.consumption {
        let guidance = match f.status.as_str() {
            "mismatched" => {
                "Check the explicit mapping against the native receipt and the chosen handoff. Resolve differing declared fields in the consumer, or prepare/refresh a handoff for the intended inputs; rerun the consumer and record its new native receipt in a new output directory."
            }
            "unknown" => {
                "Obtain a stable native receipt and explicit mappings for the missing content identity, columns, role or declared reader fields. Record again in a new output directory; unavailable evidence remains unknown."
            }
            _ => continue,
        };
        result.next_actions.push(format!(
            "Review consumption {} ({}): {}. Source: {}. {}",
            f.id, f.status, f.reason, source, guidance
        ));
    }
    result.next_actions.extend(
        actual
            .notes
            .iter()
            .map(|n| format!("Unverified case candidate from caller: {n}")),
    );
    result.limitations.push("Consumption compares explicitly mapped native receipt fields; it does not independently observe an external process, nor revalidate present-day input files.".into());
    Ok(result)
}
// Keep literal type data on one line and inside an unambiguous code span.
fn markdown_literal(value: &str) -> String {
    let text = value.escape_debug().to_string();
    if text.is_empty() {
        return "`\"\"`".into();
    }
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let delimiter = "`".repeat(longest + 1);
    let needs_padding = text.starts_with('`')
        || text.ends_with('`')
        || (text.starts_with(' ') && text.ends_with(' ') && text.chars().any(|c| c != ' '));
    let padding = if needs_padding { " " } else { "" };
    format!("{delimiter}{padding}{text}{padding}{delimiter}")
}

pub fn publish(r: &Receipt, out: &Path) -> Result<()> {
    if out.exists() {
        bail!("output exists; choose a new run directory")
    }
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let name = out
        .file_name()
        .context("output needs directory name")?
        .to_string_lossy();
    let temp = parent.join(format!(".{name}.{}.tmp", std::process::id()));
    fs::create_dir(&temp)
        .context("temporary run exists; retain it for interrupted-run recovery")?;
    let write_result = (|| -> Result<()> {
        let mut f = File::create(temp.join("receipt.json"))?;
        serde_json::to_writer_pretty(&mut f, r)?;
        f.write_all(b"\n")?;
        f.sync_all()?;
        let mut md = format!(
            "# {}: {}\n\nExecution: **{}**\n\n{}\n\n",
            r.operation, r.request.task, r.execution, r.request.purpose
        );
        if r.operation == "record" {
            md.push_str("Measured findings below are carried from the previous handoff; record does not rerun checks. Reuse diagnostics describe that previous measurement.\n\n");
        }
        for f in r.findings.iter().chain(&r.consumption) {
            let prior = if r.operation == "record" && f.origin == "measured" {
                "Prior measurement: "
            } else {
                ""
            };
            md.push_str(&format!(
                "- `{}`: **{}** ({}) — {prior}{}\n",
                f.id, f.status, f.origin, f.reason
            ));
            if let Some(reuse) = f.details.get("reuse") {
                md.push_str(&format!(
                    "  - {} (`{}`): {}\n",
                    reuse["decision"].as_str().unwrap_or(""),
                    reuse["reason_code"].as_str().unwrap_or(""),
                    reuse["explanation"].as_str().unwrap_or("")
                ));
            }
            if f.origin == "measured"
                && let Some(check) = r.request.checks.iter().find(|c| c.id == f.id)
                && let CheckKind::Schema { required } = &check.kind
                && let Some(input) = r.request.inputs.iter().find(|i| i.id == check.input)
            {
                let mut columns = input.columns.iter().collect::<Vec<_>>();
                for column in required.keys() {
                    if !columns.contains(&column) {
                        columns.push(column);
                    }
                }
                if !columns.is_empty() {
                    md.push_str("  - Column types observed by Rowtide's built-in reader (consumer representations may differ):\n");
                    for column in columns.iter().take(12) {
                        let observed = f.details["physical_schema"][*column]
                            .as_str()
                            .map(markdown_literal)
                            .unwrap_or_else(|| "not recorded".into());
                        let requirement = required
                            .get(*column)
                            .map(|ty| format!(" (required: {})", markdown_literal(ty)))
                            .unwrap_or_default();
                        md.push_str(&format!(
                            "    - {}: {observed}{requirement}\n",
                            markdown_literal(column)
                        ));
                    }
                    if columns.len() > 12 {
                        md.push_str(&format!("    - {} additional projected/required columns omitted; see receipt.json for full details.\n", columns.len() - 12));
                    }
                }
            }
            if f.details.get("expected_distinct_key_count").is_some() {
                md.push_str(&format!("  - Coverage: {} expected distinct keys; {} covered; {} missing. Expected rows: {}; actual rows: {}; actual distinct keys: {}; expected null-key rows: {}.\n", f.details["expected_distinct_key_count"], f.details["covered_distinct_key_count"], f.details["missing_distinct_key_count"], f.details["expected_row_count"], f.details["actual_row_count"], f.details["actual_distinct_key_count"], f.details["expected_null_key_rows"]));
            } else if f.details.get("row_count").is_some() {
                md.push_str(&format!("  - Keys: {} rows; {} distinct keys (including null); {} null-key rows; {} duplicate keys; {} duplicate excess rows.\n", f.details["row_count"], f.details["distinct_key_count_including_null"], f.details["null_key_rows"], f.details["duplicate_key_count"], f.details["duplicate_excess_rows"]));
            }
        }
        md.push_str("\n## Inputs\n\n");
        for i in &r.request.inputs {
            md.push_str(&format!(
                "- `{}`: {} — `{}`; columns: {:?}\n",
                i.id, i.role, i.path, i.columns
            ));
        }
        md.push_str("\n## Next actions\n\n");
        if r.next_actions.is_empty() {
            md.push_str("No unresolved evidence actions recorded.\n");
        }
        for action in &r.next_actions {
            md.push_str(&format!("- {action}\n"));
        }
        md.push_str("\n## Limits\n\n");
        for l in &r.limitations {
            md.push_str(&format!("- {l}\n"));
        }
        let mut f = File::create(temp.join("SUMMARY.md"))?;
        f.write_all(md.as_bytes())?;
        f.sync_all()?;
        // create_new reservation prevents concurrent writers claiming the same destination.
        let lock = parent.join(format!(".{name}.publish-lock"));
        let guard = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
            .context("another publisher reserved this output")?;
        let publication = (|| -> Result<()> {
            if out.exists() {
                bail!("output was created concurrently")
            };
            fs::rename(&temp, out)?;
            Ok(())
        })();
        drop(guard);
        let _ = fs::remove_file(lock);
        publication
    })();
    write_result.context(format!("incomplete run retained at {}", temp.display()))
}
