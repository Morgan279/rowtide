# Reader configuration and analysis declarations

This optional companion uses the same fictional fixtures and existing
`consumer.py`. It leaves the original walkthrough unchanged. Python's standard
library and an existing Rowtide executable are sufficient; no new Rowtide
command, request field or mapping API is added.

```sh
python3 /absolute/path/to/rowtide/examples/orders/analysis_metadata.py \
  --binary /absolute/path/to/rowtide \
  --workspace /tmp/analysis-metadata-new
```

The workspace must not exist. Every CLI command and rejected consumer attempt
is retained. Inspect `evidence.json`, `01-revenue/native.json`,
`04-counts/native.json` and `06-reader-changed/receipt.json`.

The existing data reader hashes and parses the same CSV bytes, validates the
explicit store population, ordered columns and five checks, and enforces its
period and integer USD-cent meaning. Those settings remain in the input reader
configuration; the roles, projection and coverage universe are unchanged.
The downstream example selects revenue (435 cents) or counts (4 orders) from
the report the reader actually produced. It records the actual mode, measure,
implementation digest and source report/native digests under `analysis_run`.
Input declarations come from the reader's actual native output, never from
expected handoff hashes. Both reader and analysis outputs remain separate,
immutable artifacts in the workspace.

With the same request, reader bytes/parameters, input bytes and checks, refresh
reuses all five checks. Each mode is then recorded using the existing `/inputs`
mappings. Both sets of input declarations match and remain `source_declared`.
`record` binds **complete native JSON bytes**, including `analysis_run`, but
compares **only mapped input fields**. It does not compare the unmapped
`/analysis_run/mode` with expected task intent. Changed analysis metadata produces
a different native-source digest while input declarations can still match.
Matched inputs must not be reported as validated analysis selection or output.

The optional expectation is plainly consumer-owned, before publishing analysis:

```python
if expected_mode is not None and mode != expected_mode:
    raise ValueError("consumer expected analysis mode " + expected_mode + "; got " + mode)
```

The revenue run requests this check; the counts run does not. An additional
counts/revenue expectation mismatch is rejected without publishing analysis
output. Saving a declared expectation/check result in native metadata does not
make Rowtide independently validate it.

The contrast changes the declared `amount_unit` from cents to dollars while
retaining file bytes and checks. This is a meaning-bearing reader parameter:
all five checks recheck for `input_config_changed`, as before. Built-in schema,
key and coverage results still cannot approve a currency unit. The existing
consumer refuses the unsupported configuration instead of labeling cents as
dollars or echoing the new setting as actually used.

Separate an analysis mode only when it really leaves reading and data meaning
unchanged. Keep population, parse, window/filter, projection, coverage, units
and other meaning controls in the appropriate reader/task identity. A consumer
that takes its mode from reader parameters needs an explicit interface/contract
migration before using another layout; do not remove fields from existing
requests to manufacture reuse. Contract or reader changes retain their normal
invalidation behavior. This is controlled contract usage, not a cache
optimization, adoption or savings claim.
