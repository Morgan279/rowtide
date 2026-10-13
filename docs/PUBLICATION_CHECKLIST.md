# Source release checklist

Publish only the exact reviewed public source snapshot and its intended refs.
Use a clean history when private development history contains material that is
outside the public release. Do not upload an entire working directory.

## Intended contents

- Cargo.toml/Cargo.lock, build.rs and src/: CLI/library and locked build.
- tests/: constructed public correctness and receipt regressions.
- examples/orders/: fictional fixtures, reader, walkthrough and analysis metadata
  companion; examples/consume_csv.py is a small diagnostic consumer.
- examples/direct_duckdb_baseline.rs: generic direct-check reference, with no
  measured advantage or adoption claim.
- README.md, RELEASE_NOTES.md, docs/, LICENSE and NOTICE.

Exclude private datasets, operational outputs, local paths, credentials, private
case narratives, development handoffs, compiler caches and binary archives.
Inspect every intended file and reachable Git commit/tree/blob, including author
metadata and release text. Secret-pattern checks supplement semantic inspection;
they do not replace it. No private predecessor history should be reachable from
published refs. Exact source revision, tree, file list and artifact hashes belong
in the release manifest.

## Verification

- Version/help agree with the source package and release name.
- Fresh local source installation and fictional workflow run successfully.
- Implementation upgrades recheck; same-identity refresh can reuse; historical
  record retains its selected measurement identity.
- Content and links are complete, with only generic or fictional examples.
- Apache-2.0 project terms and declared dependency licenses remain intact.
- Independent privacy/release review accepts the exact revision and objects
  before upload. Verify remote refs and source contents after an approved upload.

The source-only preview is tested on an equipped macOS arm64 host. Other
platforms and pristine dependency environments are untested. Saved native
statements are source-declared; there is no process monitoring, domain validity,
external adoption or savings certification. Source dependency inventory is not
binary-distribution notice coverage; binaries are excluded from this release.
