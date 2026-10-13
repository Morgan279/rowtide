# Fictional retail orders: a complete handoff

All data in this example is fictional. Fixtures and Python scripts are
redistributable under the repository's Apache-2.0 license. Python 3 standard library is sufficient for
the consumer and walkthrough; supply a built `rowtide` executable separately.

The optional [analysis metadata companion](ANALYSIS_METADATA.md) demonstrates
saved analysis declarations, mapped input comparisons and consumer-owned
expectations without changing this walkthrough.

Run from any directory, using absolute paths to the script and binary and a new
workspace (it must not already exist):

```sh
python3 /absolute/path/to/rowtide/examples/orders/walkthrough.py \
  --binary /absolute/path/to/rowtide \
  --workspace /tmp/orders-demo-new
```

The walkthrough retains each command, exit status, stdout and stderr under
`commands/`, plus every immutable prepare, refresh and record output. It first
prepares with a missing orders file and retains the consumer's failed attempt.
It then restores the original `orders.csv`: schema and keys pass, but coverage
diagnoses the missing west store and the consumer refuses to publish a report.
`orders-complete.csv` resolves this with the fictional west order. Refresh
rechecks three order-dependent checks and reuses the two independent store
checks. The unchanged original `request.json` remains a smaller negative example.

The external `consumer.py` requires all five exact, unscoped measured checks to
pass, validates its own declared reader digest and parameters, and hashes and
parses the same input bytes before calculating business results. Amounts are
integer **USD cents**, for the fictional **2026-09** export. It rejects malformed
rows, negative amounts and stores outside the explicit universe. Built-in
DuckDB checks do not execute this reader or certify these extra business rules.

The first report is north: 2 orders / 195 cents, south: 1 / 90, west: 1 / 150;
total: **4 orders / 435 cents ($4.35)**. Its `native.json` includes the business
result, actual input digests, ordered columns, roles, and reader identity.
`mapping.json` explicitly maps native fields with JSON Pointers; `record`
matches both input identities and carries five historical checks with zero new
measurements.

Next, `orders-updated.csv` adds one south order for 60 cents. The old handoff is
rejected by the consumer before output. Refresh rechecks the three dependent
checks and reuses both store checks; the new report is north: 2 / 195, south:
2 / 150, west: 1 / 150; total: **5 orders / 495 cents ($4.95)**. A final unchanged
refresh reuses all five checks.

`evidence.json` asserts exact results, matched consumption, recheck/reuse counts,
reasons, changed/unchanged bindings, stale rejection, and unchanged SHA-256
digests for all prior run artifacts. Input revisions remain under `inputs/`;
each historical receipt retains its binding to the earlier bytes.

Native consumption evidence remains **source_declared**: `record` compares
mapped statements with the selected handoff; it does not independently observe
or prove process compliance. This demo executes the consumer and records its
outputs, but offers no transactional snapshot guarantee. It is a small retail
business example; domain validity remains the consumer's responsibility.
