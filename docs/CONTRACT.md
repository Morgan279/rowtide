# Rowtide interface (receipt schema 1)

`src/model.rs` defines strict request types. Unknown request fields are rejected so misspelled conditions cannot silently disappear. Paths are relative to the request/mapping JSON directory, then stored as absolute paths. Inputs and requirement IDs must be unique, nonempty, and contain no colon. Only explicitly selected checks run.

## Request

- `task`, `purpose`: required text; `contract`: optional local authority file.
- `inputs`: `{id,path,role,format,columns?,reader?}`. Format `csv` or `parquet` for built-in checks; other formats may be described but cannot receive a built-in check PASS. `columns` is the intended consumer projection. `reader` contains local `path` and string map `parameters`; this entry is bound, never executed.
- `checks`: `{id,input,kind,scope?}`. Kind is `{"type":"schema","required":{"column":"TYPE"}}` (`*` permits any type), `{"type":"key","columns":[...]}`, or `{"type":"coverage","columns":[...],"expected_input":"id"}`. Exact physical key types must match for coverage. `scope` is `{column,min:null|string,max:null|string}`. Bounds use the physical type's DuckDB comparison conversion; no SQL expression accepted.
- `budgets`: defaults `hash_bytes=2147483648`, `timeout_seconds=60`, `memory_mb=512`, `temp_mb=512`, `max_examples=5`. Hash budget is shared across data/readers/evidence/contract. Timeout covers the check batch, not hashing and JSON serialization. Missing stable hashes yield unknown rather than reusable certification. Evidence/native JSON documents have an 8 MiB read limit. DuckDB errors/interruptions retain unknown rather than incomplete statistics as PASS.

## Source declarations

`evidence`: `{id,path,pointer?|excerpt?,origin,scope,input_sha256}`. Exactly one of JSON Pointer or literal excerpt is required. Origins: `source_declared` or `agent_proposed`. `input_sha256` maps input ID to full digest when the original evidence truly binds that version; do not fill it merely because a present-day file happens to exist.

`declarations`: `{id,input,description,evidence:[ids],expected:JSON}`. Exact extracted values are compared. A literal excerpt becomes a string value. All evidence must be found, strongly bound to the input and source-declared to support a condition. Conflicting values remain unknown. Agent proposals alone cannot support a declaration. The tool checks value identity and provenance binding, not the truth or interpretation of an author's claim. A changed source is read again; an old declaration is never copied blindly.

ODCS or external quality reports can be referenced with explicit pointers; no generic import, quality-rule conversion, automatic field mapping or full standards compatibility is claimed. Unmapped fields remain unconsumed. Markdown command snippets are text only.

## Results and reuse

Rowtide 0.2.1 keeps receipt/request schema 1 readable, including historical
Rowtide 0.2.0 and local-data receipts. Its package/engine fingerprint prevents reuse of old
measured findings on the first refresh (`implementation_changed`). Newly checked
receipts use `rowtide/0.2.1/checks-1/...`; a same-identity refresh can then reuse
them. A historical record operation retains the selected handoff's measurement
engine identity and does not rewrite or refresh the old receipt.

Schema version 1 contains normalized request, per-file bindings (`input:id`, `reader:id`, `evidence:id`, `contract`), scoped findings, freshness, next actions and limitations. A first observation has freshness `unbound` (no previous comparator) even if its binding state is `bound`; these are distinct dimensions. Refresh reports unchanged/changed/unavailable/unbound. Same bytes at a new data path can reuse, provided other dependencies match. Task, purpose or contract changes conservatively prevent reuse. Coverage depends on both actual and expected input.

`record` does not refresh input files: it compares actual historical consumer evidence with the chosen immutable handoff. Use `refresh` separately for current freshness. Actual receipt format: `{source,mappings,notes?}`; source is a local native JSON file. Each mapping has `input`, `path_pointer`, optional `sha256_pointer`, `columns_pointer`, `role_pointer`, `reader_sha256_pointer`, `reader_parameters_pointer`. Missing optional mappings are evidence gaps, not wildcards. A nonempty intended projection requires exact ordered columns. A declared reader requires its digest and parameters. Content digest allows data relocation; path alone cannot match a version. Native receipt statements are source-declared, not independently observed execution.

Caller notes become explicitly unverified regression candidates, never accepted facts. Use a new prepare/refresh request with explicit source evidence to adopt a correction.

`record` adds `next_actions` for each `mismatched` or `unknown` consumption finding. Actions identify the input, native receipt source and differing/missing fields, then explain how to reconcile the consumer/handoff or supply stable mapped evidence and record a new run. JSON and SUMMARY use the same actions. A completed comparison with differing declarations still has `execution=completed`; gaps still yield `partial`. Even `matched` remains `source_declared`, not proof of actual compliance. Recording from a prior record rebuilds guidance from carried handoff findings and the current comparison/notes, so resolved historical consumption actions do not persist; prior partial execution remains partial.

Output publication stages files in a sibling `.NAME.PID.tmp` and atomically renames after writing. A publication lock prevents concurrent writers to the same target. Stale staging directories/locks after interruption are incomplete artifacts, not valid receipts; retain for diagnosis and choose a fresh output name. No overwrite or global mutable catalog.

Refresh accepts an optional `--request`. Without it, the previous receipt's normalized request is validated and used to hash and inspect current files; it does not trust prior digests as current identity. Explicit `--request` fully replaces the request. Paths normalized by prepare remain independent of refresh's working directory.

Every requested measured check includes `details.reuse`: `decision` (`reused` or `rechecked`), `reason_code`, and `explanation`. Recheck reasons are `first_run`, `implementation_changed`, `task_or_contract_changed`, `check_changed`, `input_config_changed`, `input_content_changed_or_unbound`, `reader_content_changed_or_unbound`, and `no_prior_result`; a valid cache hit uses `dependencies_unchanged`. The first applicable dependency failure explains the decision. Input configuration includes reader parameters and reader path. The measured finding's `reason` stays separate from reuse diagnostics. Declarations are re-evaluated source evidence and do not receive measured-cache diagnostics. Stdout counts `reused_checks` and `rechecked_checks` over `request.checks` only; existing execution and exit semantics remain unchanged.

The human summary includes bounded key/coverage counts, explicit expected distinct-key and row denominators, reuse explanations, and `next_actions`. For schema findings it displays saved physical types observed by Rowtide's built-in reader, with requested types, for up to 12 projected/required columns. Missing type evidence is `not recorded`; the finding's status is unchanged. These are not necessarily the consumer's representations, and different types alone do not establish semantic incompatibility. See the [consumer-owned cross-format example](RESUMING.md#check-reader-representations). JSON remains the authoritative machine-readable result, including the full `details.physical_schema` when recorded.

On `record`, both per-run check counters are zero; `carried_checks` counts prior measured findings. The summary labels measurement and reuse diagnostics as historical to the previous handoff, not new measurements.
