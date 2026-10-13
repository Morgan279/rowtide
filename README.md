# Rowtide

An agent-native toolkit for local data.

An early preview for agents and scripts resuming recurring CSV and Parquet work.

When an agent or script resumes an analysis, it needs more than a file path:
which inputs, columns and roles were intended, what was checked, which limits
remain, what changed, and which version did the consumer declare? Rowtide
keeps those answers in an explicit, reusable handoff. Your existing scripts
still parse source formats, define the business population and produce results.

Written in Rust with bundled DuckDB. **Apache-2.0**, developed for self-use and
open-source reuse. Current CLI version: **0.2.1**. [First use](docs/FIRST_USE.md) covers a local source build; binary downloads are not included.

## Prepare, resume, record

| Operation | What it does | What to read next |
|---|---|---|
| `prepare` | Save an explicit request, content/reader bindings and named checks | `findings`, input roles, scope and `next_actions` |
| `refresh` | Rehash current dependencies against a saved receipt; explain reuse and rechecks | `freshness` and each check's `details.reuse` |
| `record` | Compare mapped native consumer fields with the chosen handoff | `consumption` and its evidence gaps or differences |

Each operation writes a new directory with `receipt.json` and `SUMMARY.md`.
Keep the receipt path with your task outputs so the next session has a starting
point. The JSON is the machine result; the summary is its reading view.

For the same task, resume from the saved receipt without reconstructing its
request or depending on the current working directory:

```sh
"$ROWTIDE" refresh --previous "$PREVIOUS_RECEIPT" --out "$NEW_HANDOFF_DIR"
```

Set `ROWTIDE` to the absolute executable path and use a new output directory.
Read the refreshed findings before running your existing consumer. Let that
consumer emit its native receipt and mapping, then compare its declarations:

```sh
"$ROWTIDE" record --previous "$NEW_HANDOFF_DIR/receipt.json" \
  --actual "$NATIVE_MAPPING" --out "$NEW_RECORD_DIR"
```

The example consumer already generates its mapping; reuse that pattern instead
of editing JSON Pointers each run. If paths, requirements or reader settings
change, use `refresh --request ...` with a complete replacement request.
[Resuming a task](docs/RESUMING.md) shows the handoff sequence and change cases.

## Try a complete example

The fictional retail example diagnoses a missing file and a missing store,
resolves the input, computes a report, records consumption and refreshes after
an update. It needs Python's standard library; Python is optional for the CLI.
From a source checkout with existing Rust/C++ tools:

```sh
"$HOME/.cargo/bin/cargo" build --locked --release
ROWTIDE="$PWD/target/release/rowtide"
DEMO=$(mktemp -d /tmp/rowtide-demo.XXXXXX)
python3 examples/orders/walkthrough.py \
  --binary "$ROWTIDE" --workspace "$DEMO/orders-run"
```

Use an absolute binary path and a workspace that does not exist yet.

The complete export produces **4 orders / 435 USD cents**. A constructed update
produces **5 / 495 cents**, rechecks three order-dependent checks and reuses two
store checks; an unchanged refresh reuses all five. The script asserts that
earlier receipts remain unchanged and retains every command and rejected
consumer attempt. See `orders-run/evidence.json` and `07-consumed/report.json`.
[Example details](examples/orders/README.md) explain the fixtures and reader.

The missing-store check illustrates three separate answers: the operation can
complete and save a receipt while a finding is `mismatch`; the example consumer
then refuses the incomplete report. Later, `record` can match declared input
identities without approving every data requirement. `unknown` means evidence
is missing or incomplete. A `matched` native receipt remains `source_declared`;
the tool compares its fields rather than monitoring the external process.

## Verification and limits

The bundled examples use fictional data and explicit consumer rules. They
exercise missing input, coverage gaps, stale consumption, content changes,
selective reuse and native declaration comparison. The tests use constructed
CSV/Parquet fixtures with fixed expectations. These engineering checks do not
establish external adoption, performance benefits or human effort saved.

The consumer selects the population, parses source values and interprets results.
Rowtide preserves explicit evidence and compares declarations; it does not
independently certify domain logic. [Implementation status](docs/IMPLEMENTATION_STATUS.md)
describes the supported behavior and validation boundary.

## When a simpler script is enough

Use Rowtide when repeated handoffs need a consistent record across sessions
or consumers. Plain DuckDB or a reliable existing script is often sufficient
for a one-off analysis, or when its manifests, checks and documentation already
provide the same record. For example, in an existing DuckDB session:

```sql
SELECT store, count(*) AS orders, sum(amount) AS total_cents
FROM read_csv_auto('examples/orders/orders-complete.csv')
GROUP BY store
ORDER BY store;
```

That computes the report totals; your script can own any required checks and
input history. The optional direct DuckDB example illustrates explicit checks
and content caching. No speed, token, cost or human-effort advantage is claimed
for Rowtide.

## Technical scope

Built-in checks cover UTF-8 headered CSV and Parquet schemas/physical types,
declared keys (nulls and duplicates), and coverage against an explicit expected
key table. CSV types are inferred by DuckDB. A scope filter applies to the actual
input; the expected coverage table remains the complete declared universe.
Business semantics and unsupported source conversion stay with your scripts.

Refresh reads full file contents for SHA-256 identity. Reuse also depends on
reader bytes/parameters, requirements, task/contract and implementation; a
version upgrade can recheck unchanged data. Reader entries are bound, not
executed. Source declarations keep their origin and scope. Budget exhaustion,
unavailable files and incomplete evidence yield `unknown`; ordinary files are
not transactional snapshots. `record` carries historical checks and does not
refresh current files. Exit 0 means a receipt was saved; invalid requests or
publication failures exit 2. Read the three result dimensions in the flow above.

The current validation boundary is macOS 27.0.1 arm64. Other platforms and a
pristine machine remain untested. There is no SQLite catalog, MCP service,
global search, automatic cleaning or arbitrary SQL interface. The core remains
three CLI operations and immutable per-run files.

[Interface contract](docs/CONTRACT.md) · [Regression guidance](docs/REGRESSIONS.md)
· [Release notes](RELEASE_NOTES.md) · [Locked dependency inventory](docs/DEPENDENCIES.md)
· [License](LICENSE)
