#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""External fictional retail report consumer; Python standard library only."""
import argparse
import csv
import hashlib
import io
import json
from pathlib import Path

PARAMETERS = {"period": "2026-09", "amount_unit": "USD cents"}
NEEDED_CHECKS = {
    "order_columns": ("orders", {"type": "schema", "required": {"order_id": "VARCHAR", "store": "VARCHAR", "amount": "BIGINT"}}),
    "unique_orders": ("orders", {"type": "key", "columns": ["order_id"]}),
    "store_coverage": ("orders", {"type": "coverage", "columns": ["store"], "expected_input": "stores"}),
    "store_columns": ("stores", {"type": "schema", "required": {"store": "VARCHAR"}}),
    "unique_stores": ("stores", {"type": "key", "columns": ["store"]}),
}
SPECS = {
    "orders": ("order_rows", ["order_id", "store", "amount"]),
    "stores": ("expected_store_universe", ["store"]),
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(content):
    return hashlib.sha256(content).hexdigest()


def consume(handoff, out):
    handoff_bytes = handoff.read_bytes()
    receipt = json.loads(handoff_bytes)
    request = receipt["request"]
    require(receipt["schema_version"] == 1, "unsupported receipt schema")
    require(request["task"] == "fictional-monthly-store-report", "wrong task")
    checks = {c["id"]: c for c in request["checks"]}
    findings = {f["id"]: f for f in receipt["findings"]}
    for name, (input_id, kind) in NEEDED_CHECKS.items():
        check = checks.get(name, {})
        require(check.get("input") == input_id and check.get("kind") == kind
                and check.get("scope") is None, "required check changed: " + name)
        finding = findings.get(name, {})
        require(finding.get("status") == "supported" and finding.get("origin") == "measured",
                "required check did not pass: " + name)
    specs = {spec["id"]: spec for spec in request["inputs"]}
    require(set(specs) == set(SPECS), "unexpected input set")
    reader_path = Path(__file__).resolve()
    reader_digest = digest(reader_path.read_bytes())
    actual, parsed = [], {}
    for input_id, (role, columns) in SPECS.items():
        spec = specs[input_id]
        require(spec["format"] == "csv" and spec["role"] == role and spec["columns"] == columns,
                "consumer input configuration changed: " + input_id)
        reader = spec.get("reader") or {}
        require(Path(reader.get("path", "")).resolve() == reader_path
                and reader.get("parameters") == PARAMETERS, "consumer reader configuration changed")
        binding = receipt["bindings"]["reader:" + input_id]
        require(binding["state"] == "bound" and binding["sha256"] == reader_digest,
                "consumer reader differs from handoff")
        path = Path(spec["path"]).resolve()
        content = path.read_bytes()
        sha256 = digest(content)
        binding = receipt["bindings"]["input:" + input_id]
        require(binding["state"] == "bound" and binding["sha256"] == sha256,
                "input differs from handoff: " + input_id)
        # Parse the exact immutable bytes just hashed, never reopen the input.
        reader = csv.DictReader(io.StringIO(content.decode("utf-8"), newline=""))
        require(reader.fieldnames == columns, "unexpected CSV columns")
        rows = list(reader)
        require(all(set(row) == set(columns) and all(row[c] is not None for c in columns)
                    for row in rows), "malformed CSV row")
        parsed[input_id] = rows
        actual.append({"id": input_id, "path": str(path), "sha256": sha256,
                       "columns": columns, "role": role, "row_count": len(rows),
                       "reader_sha256": reader_digest, "reader_parameters": PARAMETERS})
    stores = [row["store"] for row in parsed["stores"]]
    require(stores and all(stores) and len(stores) == len(set(stores)), "invalid store universe")
    totals = {store: {"store": store, "order_count": 0, "revenue_usd_cents": 0} for store in stores}
    order_ids = set()
    for row in parsed["orders"]:
        require(row["order_id"] and row["order_id"] not in order_ids, "invalid order key")
        order_ids.add(row["order_id"])
        require(row["store"] in totals, "order outside expected store universe")
        require(row["amount"].isascii() and row["amount"].isdigit(), "amount must be nonnegative integer USD cents")
        total = totals[row["store"]]
        total["order_count"] += 1
        total["revenue_usd_cents"] += int(row["amount"])
    require(all(total["order_count"] for total in totals.values()), "missing store orders")
    report = {"period": PARAMETERS["period"], "amount_unit": PARAMETERS["amount_unit"],
              "stores": [totals[store] for store in sorted(totals)],
              "total_order_count": len(order_ids),
              "total_revenue_usd_cents": sum(t["revenue_usd_cents"] for t in totals.values())}
    native = {"consumer": "fictional retail report / consumer.py", "handoff_sha256": digest(handoff_bytes),
              "inputs": actual, "report": report}
    mappings = [{"input": entry["id"], **{key + "_pointer": f"/inputs/{n}/{key}"
                 for key in ("path", "sha256", "columns", "role", "reader_sha256", "reader_parameters")}}
                for n, entry in enumerate(actual)]
    out.mkdir(parents=True, exist_ok=False)
    (out / "native.json").write_text(json.dumps(native, indent=2) + "\n")
    (out / "mapping.json").write_text(json.dumps({"source": "native.json", "mappings": mappings}, indent=2) + "\n")
    (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"report": str(out / "report.json"), "mapping": str(out / "mapping.json")}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("handoff", type=Path)
    parser.add_argument("out", type=Path)
    args = parser.parse_args()
    consume(args.handoff.resolve(), args.out.resolve())
