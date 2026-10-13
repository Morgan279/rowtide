# First use: Rowtide 0.2.1

Rowtide is an early preview for agents and scripts working with local CSV and
Parquet. This release provides source and fictional examples. Use a source
checkout or the matching source archive; binary downloads are not included.

## Install from source into a local directory

Tested build environment: macOS 27.0.1 arm64, Rust/Cargo 1.99.0 and Apple clang++
21.0.0. These tools must already be installed. Python 3.9+ standard library is
needed only for the example (3.9.6 tested). No separate DuckDB installation is
required. Older toolchains and other platforms have not been validated.

Check existing prerequisites; this guide does not install them:

```sh
uname -ms
"$HOME/.cargo/bin/rustc" --version
"$HOME/.cargo/bin/cargo" --version
clang++ --version
python3 --version
```

From a new directory, replace the source archive path:

```sh
FIRST_USE=$(mktemp -d /tmp/rowtide-first-use.XXXXXX)
tar -xzf /absolute/path/rowtide-0.2.1-source.tar.gz -C "$FIRST_USE"
SRC="$FIRST_USE/rowtide-0.2.1"
CARGO="$HOME/.cargo/bin/cargo"
BUILD_DIR="$FIRST_USE/build"
INSTALL_ROOT="$FIRST_USE/local"
"$CARGO" install --path "$SRC" --locked \
  --root "$INSTALL_ROOT" --target-dir "$BUILD_DIR"
ROWTIDE="$INSTALL_ROOT/bin/rowtide"
"$ROWTIDE" --version
"$ROWTIDE" --help
python3 "$SRC/examples/orders/walkthrough.py" \
  --binary "$ROWTIDE" --workspace "$FIRST_USE/orders-run"
```

Expected version: `rowtide 0.2.1`. The package, Rust crate and executable are all
named `rowtide`. Use the absolute executable path in later sessions; no global
install or shell profile edit is needed. If Rust is installed elsewhere, set
CARGO to that executable instead of the rustup path above.

Bundled DuckDB makes the first native compile slow. An uncached build needs
registry access for the locked sources. If those sources are already cached,
add `--offline` to the install command. You can set BUILD_DIR to an existing
private compiler cache; the installation still selects its resulting binary.
Current validation reused installed tools and cached dependency sources. A
fresh source/install directory is not proof of an unprovisioned machine.

From a checkout rather than an archive, use the same install command with
`SRC` set to the checkout's absolute path. To build without installing:

```sh
"$CARGO" build --manifest-path "$SRC/Cargo.toml" \
  --target-dir "$BUILD_DIR" --locked --release
ROWTIDE="$BUILD_DIR/release/rowtide"
```

## Read the result and continue

The example retains missing-file/missing-store diagnoses and rejected consumer
attempts, then reports 4 orders / 435 USD cents. A constructed update reports
5 / 495, rechecks three dependent checks and reuses two store checks. An
unchanged refresh reuses all five. These are fictional fixtures and technical
checks, not adoption or savings measurements.

Inspect `orders-run/evidence.json`, `07-consumed/report.json`,
`08-recorded/SUMMARY.md` and the command/stderr records. Use a new workspace for
another run. [Example details](../examples/orders/README.md) and
[task resumption](RESUMING.md) describe the consumer and saved receipt paths.

Exit 0 means a receipt was saved. Read findings independently of execution,
and consumption independently of both. Matched native fields are
source-declared evidence. Record carries historical checks rather than checking
current file freshness. Schema 1 receipts from pre-Rowtide local-data remain
readable, but the first Rowtide refresh rechecks them for the new implementation
identity. See [release notes](../RELEASE_NOTES.md) for the naming boundary.

Public Rust checks, with cached sources:

```sh
"$CARGO" test --manifest-path "$SRC/Cargo.toml" --locked --offline --release
```
