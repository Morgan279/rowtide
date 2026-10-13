# Implementation status

Rowtide 0.2.1 is an early preview with three local CLI operations and immutable
per-run receipts. No service, model account or Python runtime is required by the
CLI. Python standard library is optional for the bundled fictional examples.

The built-in checks cover declared CSV/Parquet schemas, keys and expected-key
coverage. Full content hashes bind inputs, readers and explicit evidence. Reuse
requires unchanged strongly bound dependencies and implementation identity;
unavailable data, resource interruption and incomplete evidence stay unknown.

Public regression fixtures are constructed independently from literal CSV and
Parquet data. They cover positive and negative checks, scoped population,
input/reader/contract changes, source declarations, missing native fields,
immutable output, historical record identity and literal summary rendering.
The fictional retail walkthrough and analysis companion retain their commands,
rejected attempts and prior artifact hashes in a new workspace.

Version metadata participates in fingerprinting. The first refresh after an
implementation upgrade rechecks; a later same-identity refresh can reuse.
Record compares explicitly mapped declarations and carries historical findings,
including unknown or mismatched evidence. Saved analysis metadata is not an
independently validated analysis result.

Tested scope is an equipped macOS arm64 environment using existing Rust/C++ and
cached locked sources. A fresh source/install directory does not prove a clean
dependency environment or another platform. No external adoption, performance
advantage or human-effort savings is claimed. Reproduce the public checks with
`cargo test --locked` and the examples with fresh output directories. See
[first use](FIRST_USE.md), [the contract](CONTRACT.md) and
[release notes](../RELEASE_NOTES.md).
