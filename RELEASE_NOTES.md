# Rowtide 0.2.1 — early preview

Source-only release for agents and scripts resuming local CSV/Parquet work.
Version 0.2.1 identifies this source snapshot; exact revisions and checksums
identify each prepared artifact.

## Usability improvements

- Schema summaries show saved types observed by Rowtide's built-in DuckDB reader
  for up to 12 projected/required columns, with requested types alongside.
  Controls and backticks render as literal text; raw JSON remains unchanged.
  Consumer representations may differ. Rowtide does not normalize values or
  infer equivalence from physical types.
- Record summaries label carried reasons as `Prior measurement:`. Record
  compares native declarations with a historical handoff and does not rerun
  input checks or refresh current files.
- A fictional retail companion separates reading configuration from downstream
  counts/revenue declarations, shows an optional consumer expectation check,
  and demonstrates that a meaning-bearing reader change still invalidates.
  Complete native JSON is bound, but only mapped input fields are compared;
  unmapped analysis intent/output is not validated by a match.

## Compatibility

Rust, bundled DuckDB, Apache-2.0 and prepare/refresh/record remain the foundation.
The check algorithms, locked dependency versions, request/receipt schema 1 and
native mapping fields are unchanged by the release-version step.

Package/lock metadata participates in the implementation fingerprint. A refresh
across implementations rechecks prior findings with `implementation_changed`,
even if data bytes are unchanged. A subsequent same-identity refresh can reuse,
while still full-hashing. Historical record retains the selected receipt's old
measurement identity. Never edit old receipts to manufacture reuse.

## Try it and read the boundaries

[First use](docs/FIRST_USE.md) shows a local source installation. The fictional
retail walkthrough diagnoses missing input and store coverage, reports 4 orders
/ 435 cents, records native inputs and updates to 5 / 495 with three rechecks and
two reused checks. The [analysis companion](examples/orders/ANALYSIS_METADATA.md)
and [resumption guide](docs/RESUMING.md) explain consumer ownership.

Validation is bounded to an equipped macOS arm64 host with existing Rust/C++
and cached dependencies. Python standard library is needed only for examples.
Other platforms and pristine dependency environments remain untested. Binaries
are excluded from this source release. Matched consumption is source-declared,
not process monitoring or domain certification. No external adoption, benchmark
advantage or savings claim is made.
