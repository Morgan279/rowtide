# Resume a saved input handoff

Rowtide 0.2.1 uses the `rowtide` executable. No pre-rename `local-data` alias is
installed. For schema 1 receipts from Rowtide 0.2.0 or the earlier local-data implementation,
use Rowtide refresh normally: all prior findings are rechecked for
`implementation_changed` and the new receipt carries Rowtide's fingerprint.
Subsequent same-identity refresh can reuse unchanged results. Never edit the old
receipt's engine field to manufacture a cache hit. Record against an old receipt
retains the old historical measurement identity; it is not a new measurement.

Keep the latest chosen handoff's `receipt.json` path with the task's result or
run notes. Also retain the external consumer's native receipt and mapping.
Rowtide has no global task index; your existing task directory owns that
pointer. Choose the intended handoff, rather than guessing from directory order.

## Try resumption after the public walkthrough

With `ROWTIDE` and `DEMO` from the README walkthrough, the updated input is
still present and its chosen handoff is `06-updated/receipt.json`. These commands
use the existing bound consumer, without rerunning data preparation:

```sh
RESUME="$DEMO/resume-run"
"$ROWTIDE" refresh --previous "$DEMO/orders-run/06-updated/receipt.json" \
  --out "$RESUME/handoff"
python3 "$DEMO/orders-run/inputs/consumer.py" \
  "$RESUME/handoff/receipt.json" "$RESUME/consumer"
"$ROWTIDE" record --previous "$RESUME/handoff/receipt.json" \
  --actual "$RESUME/consumer/mapping.json" --out "$RESUME/record"
```

Expected: five reused checks, the same five-order / 495-cent report, and two
matched source-declared input identities. Use a new `RESUME` path for another run.
For another task, retain the interpreter, exact safe consumer command and
required outputs beside the receipt path; a reader binding does not execute or
infer that command. Distinguish a consume-only entry from one that regenerates
source data. The tool's key/coverage export can also differ from the value panel
your analysis reads; document which file each check actually covers.

## Continue the same task

Use absolute paths and a new directory for every operation. These variables
refer to your existing files and the output names you choose:

```sh
ROWTIDE=/absolute/path/to/rowtide
PREVIOUS_RECEIPT=/absolute/path/to/saved-run/receipt.json
NEW_HANDOFF_DIR=/absolute/path/to/new-handoff
"$ROWTIDE" refresh --previous "$PREVIOUS_RECEIPT" --out "$NEW_HANDOFF_DIR"
```

Refresh uses the saved normalized request, so it works from another working
directory. It rehashes current dependencies; previous digests are comparators,
not current observations. Read `findings`, `freshness`, `next_actions` and each
check's `details.reuse`. Reused findings retain both supported and mismatched
results. An implementation upgrade can produce `implementation_changed` and
recheck every measured finding even if file contents are unchanged.

Run your existing consumer when its required conditions are met. Pass it the
refreshed handoff using its own interface. Let it generate the native receipt
and mapping, then record against **that same handoff**:

```sh
NATIVE_MAPPING=/absolute/path/to/consumer-output/mapping.json
NEW_RECORD_DIR=/absolute/path/to/new-record
"$ROWTIDE" record --previous "$NEW_HANDOFF_DIR/receipt.json" \
  --actual "$NATIVE_MAPPING" --out "$NEW_RECORD_DIR"
```

The bundled example consumer already emits this mapping. A different consumer
needs explicit fields for content digest, ordered columns, role and any declared
reader digest/parameters. Missing fields remain unknown; do not rewrite a native
receipt merely to obtain a match.

Built-in checks cover the explicit exported inputs. If a preprocessing script
creates a key table and a separate values table, checks on the keys do not
independently validate the values or the script's business predicate. Bind the
files your consumer actually reads, and describe that relationship and its
limitations in your task notes.

A completed refresh can still contain a coverage mismatch. A matched record
means the mapped declarations agree with the selected handoff, and remains
source-declared evidence. Record does not rerun checks or observe today's file
freshness: its `rechecked_checks` and `reused_checks` counters are zero,
`carried_checks` counts prior measured findings, and their reuse details describe
the historical measurement. To resume from a record receipt, use refresh again
before a new consumption; historical consumption does not certify refreshed inputs.

## Check reader representations

Schema summaries show saved column types **observed by Rowtide's built-in
DuckDB reader**, for up to 12 projected/required columns. The full recorded map
is in `findings[].details.physical_schema`. `not recorded` means no type evidence
for that column in the finding; it does not change an unknown or historical
finding into a new measurement. A record summary carries the chosen handoff's
observations.

For the same calendar date, two formats and consumer readers can differ:

| Input | Example Rowtide observation | Example pandas consumer representation |
|---|---|---|
| CSV with an ISO date | `DATE` | `str` from `read_csv` without date parsing |
| Parquet with a nanosecond timestamp | `TIMESTAMP_NS` | `Timestamp` from `read_parquet` |

These are examples, not fixed types for every export. Two schema checks with
`"session_date": "*"` can both pass; independent key checks can also pass. Neither
establishes cross-format value equality. Different physical types alone do not
establish semantic incompatibility, and Rowtide does not normalize them.

For a consumer already using pandas, this small example isolates the reader
representation issue. Its explicit contract treats the values as calendar dates
with no time or timezone information:

```python
import pandas as pd

csv_dates = pd.Series(["2026-09-01"])  # representation after read_csv
parquet_dates = pd.Series([pd.Timestamp("2026-09-01")])  # after read_parquet
assert not csv_dates.equals(parquet_dates)
csv_iso = pd.to_datetime(csv_dates, errors="raise").dt.strftime("%Y-%m-%d")
parquet_iso = pd.to_datetime(parquet_dates, errors="raise").dt.strftime("%Y-%m-%d")
assert csv_iso.equals(parquet_iso)
```

The consumer chooses the conversion and checks its actual parsed keys/values.
Date-only conversion discards time; an instant/timezone contract needs its own
policy. This example does not add pandas as a Rowtide dependency or certify
equality of other columns. If the reader or its conversion contract changes,
refresh with the updated request/contract into a new directory before consuming.
Generate native declarations from the bytes, columns and reader settings the
consumer actually used; do not copy expected receipt fields to claim a match.

## Keep reading identity and analysis declarations explicit

Reader parameters describe the input-reading and data-meaning settings your
consumer actually uses. Keep population, parsing, window/filter, projection,
coverage and units in the appropriate reader/task identity. Do not quietly drop
a parameter because its name sounds like analysis or because it blocks reuse.

If a downstream mode only selects a report after the same reads, it can be
declared separately in the consumer's native JSON, with its actual parameters,
implementation identity and basis. A stable task contract must permit that
analysis; consumer-owned validation can enforce an expected mode. An existing
consumer that takes its mode from reader parameters requires an explicit
interface/contract migration before changing this layout.

`record` binds the complete native source, but compares only explicitly mapped
input fields. An unmapped analysis mode is saved metadata, not a validated
match to expected task intent. Changing it may leave input declarations matched;
even a declared consumer expectation/check result is still source-declared.

The [public companion example](../examples/orders/ANALYSIS_METADATA.md) shows
five reused data checks with different actual counts/revenue modes, matched
input declarations, an optional consumer expectation rejecting the wrong mode,
and a meaning-bearing unit change still rechecking all five checks and being
refused by the existing reader. It uses existing commands and keeps the original
walkthrough intact; it is not a cache optimization or savings claim.

## Change the task or relocate it

Saved paths are absolute. If inputs move, or roles, projections, reader settings
or requirements change, supply a complete replacement request:

```sh
"$ROWTIDE" refresh --previous "$PREVIOUS_RECEIPT" \
  --request /absolute/path/to/replacement-request.json --out "$NEW_HANDOFF_DIR"
```

Request paths are relative to that JSON file's directory, then normalized.
Explicit `--request` replaces the request; it is not a partial update. A data
file moved without byte changes can be reused if its other dependencies still
match. Reader path/configuration, purpose or contract changes may require
rechecks. Source evidence is evaluated again. See the [interface contract](CONTRACT.md)
for the exact rules.

Use the retained prior receipt to explain old results; use the new receipt to
describe the current observation. Neither a cache count nor a matching native
declaration decides whether an unresolved business requirement is acceptable.
