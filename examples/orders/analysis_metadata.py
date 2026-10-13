#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Companion example: reader identity and downstream analysis declarations.

Uses the existing retail reader and prepare/refresh/record, Python stdlib only.
Supply an absolute rowtide binary and a NEW workspace, as in walkthrough.py.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import time


def digest(content):
    return hashlib.sha256(content).hexdigest()


def save(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def analyse(reader_dir, out, mode, expected_mode=None):
    if mode not in ("revenue", "counts"):
        raise ValueError("unsupported analysis mode")
    # This optional expectation is enforced by the consumer, never by record.
    if expected_mode is not None and mode != expected_mode:
        raise ValueError("consumer expected analysis mode " + expected_mode + "; got " + mode)
    native_bytes = (reader_dir / "native.json").read_bytes()
    report_bytes = (reader_dir / "report.json").read_bytes()
    native, report = json.loads(native_bytes), json.loads(report_bytes)
    if native["report"] != report:
        raise ValueError("reader report differs from its native receipt")
    field = "revenue_usd_cents" if mode == "revenue" else "order_count"
    result = {"mode": mode, "stores": [{"store": s["store"], field: s[field]} for s in report["stores"]],
              "total": report["total_" + field]}
    # Retain actual declarations emitted by the reader that just ran. Never
    # manufacture actual hashes/columns/settings from an expected handoff.
    native["analysis_run"] = {
        "mode": mode, "parameters": {"measure": field}, "expected_mode": expected_mode,
        "expectation_check": "passed" if expected_mode is not None else "not requested",
        "implementation_sha256": digest(Path(__file__).read_bytes()),
        "basis": {"reader_native": str(reader_dir / "native.json"),
                  "reader_native_sha256": digest(native_bytes), "report_sha256": digest(report_bytes)},
        "result": result,
    }
    mapping = json.loads((reader_dir / "mapping.json").read_bytes())
    mapping["source"] = "native.json"  # Existing /inputs pointers; analysis_run is unmapped.
    out.mkdir(parents=True, exist_ok=False)
    save(out / "native.json", native)
    save(out / "mapping.json", mapping)
    save(out / "result.json", result)
    return result


def run(binary, workspace):
    if not binary.is_absolute() or not binary.is_file():
        raise ValueError("supply an absolute path to an existing rowtide binary")
    workspace = workspace.resolve()
    workspace.mkdir(parents=True, exist_ok=False)
    fixtures = Path(__file__).resolve().parent
    inputs, logs = workspace / "inputs", workspace / "commands"
    inputs.mkdir()
    logs.mkdir()
    for name in ("expected_stores.csv", "consumer.py", "report-request.json"):
        shutil.copyfile(fixtures / name, inputs / name)
    shutil.copyfile(fixtures / "orders-complete.csv", inputs / "orders.csv")
    reader, request = inputs / "consumer.py", inputs / "report-request.json"
    commands, immutable = [], {}

    def retain(directory):
        for path in directory.iterdir():
            if path.is_file():
                immutable[str(path)] = digest(path.read_bytes())

    def command(name, argv, success=True):
        argv = [str(a) for a in argv]
        started = time.perf_counter()
        result = subprocess.run(argv, cwd=workspace, capture_output=True, text=True)
        (logs / (name + ".stdout")).write_text(result.stdout)
        (logs / (name + ".stderr")).write_text(result.stderr)
        entry = {"argv": argv, "cwd": str(workspace), "returncode": result.returncode,
                 "expected_success": success, "elapsed_seconds": time.perf_counter() - started}
        commands.append(entry)
        save(logs / (name + ".command.json"), entry)
        assert (result.returncode == 0) == success, entry
        return result

    def local(name, operation, *args):
        result = command(name, [binary, operation, *args, "--out", workspace / name])
        path = workspace / name / "receipt.json"
        receipt = json.loads(path.read_bytes())
        retain(path.parent)
        return path, receipt, json.loads(result.stdout)

    def consume(name, handoff, mode, expected_mode=None):
        reader_dir, out = workspace / (name + "-reader"), workspace / name
        command(name + "-reader", [sys.executable, reader, handoff, reader_dir])
        retain(reader_dir)
        result = analyse(reader_dir, out, mode, expected_mode)
        retain(out)
        return result

    def record(name, handoff, actual):
        _, receipt, counts = local(name, "record", "--previous", handoff,
                                   "--actual", workspace / actual / "mapping.json")
        assert all(f["status"] == "matched" and f["origin"] == "source_declared"
                   for f in receipt["consumption"])
        assert len(receipt["consumption"]) == 2
        assert counts["carried_checks"] == 5 and counts["rechecked_checks"] == 0
        native_path = workspace / actual / "native.json"
        assert receipt["bindings"]["actual_receipt"]["sha256"] == digest(native_path.read_bytes())
        return receipt

    ready_path, ready, first = local("00-prepare", "prepare", "--request", request)
    assert all(f["status"] == "supported" for f in ready["findings"])
    revenue = consume("01-revenue", ready_path, "revenue", expected_mode="revenue")
    assert revenue["total"] == 435
    first_record = record("02-record-revenue", ready_path, "01-revenue")

    same_path, same, reused = local("03-refresh", "refresh", "--previous", ready_path)
    assert (reused["rechecked_checks"], reused["reused_checks"]) == (0, 5)
    assert same["request"] == ready["request"] and same["bindings"] == ready["bindings"]
    counts = consume("04-counts", same_path, "counts")
    assert counts["total"] == 4
    second_record = record("05-record-counts", same_path, "04-counts")
    revenue_native = json.loads((workspace / "01-revenue/native.json").read_bytes())
    counts_native = json.loads((workspace / "04-counts/native.json").read_bytes())
    assert revenue_native["inputs"] == counts_native["inputs"]
    assert counts_native["analysis_run"]["mode"] != revenue_native["analysis_run"]["mode"]
    assert counts_native["analysis_run"]["expectation_check"] == "not requested"
    assert first_record["bindings"]["actual_receipt"]["sha256"] != second_record["bindings"]["actual_receipt"]["sha256"]

    rejected = workspace / "expected-mode-rejected"
    try:
        analyse(workspace / "04-counts-reader", rejected, "counts", expected_mode="revenue")
    except ValueError as error:
        (logs / "expected-mode-rejection.stderr").write_text(str(error) + "\n")
    else:
        raise AssertionError("consumer expectation was not enforced")
    assert not rejected.exists()

    changed_request = json.loads(request.read_bytes())
    for spec in changed_request["inputs"]:
        spec["reader"]["parameters"]["amount_unit"] = "USD dollars"
    changed_path = inputs / "changed-reader-request.json"
    save(changed_path, changed_request)
    changed_handoff, changed, rechecked = local("06-reader-changed", "refresh", "--previous", same_path,
                                               "--request", changed_path)
    assert (rechecked["rechecked_checks"], rechecked["reused_checks"]) == (5, 0)
    assert changed["bindings"] == same["bindings"]
    assert all(f["details"]["reuse"]["reason_code"] == "input_config_changed" for f in changed["findings"])
    # The fixed retail reader supports cents only. A changed meaning is not
    # silently dropped to keep reuse or echoed as a setting actually used.
    refused = workspace / "07-reader-refused"
    result = command("07-reader-refused", [sys.executable, reader, changed_handoff, refused], False)
    assert "consumer reader configuration changed" in result.stderr and not refused.exists()
    assert all(digest(Path(path).read_bytes()) == expected for path, expected in immutable.items())
    evidence = {"status": "passed", "binary": str(binary), "binary_sha256": digest(binary.read_bytes()),
                "engine_version": ready["engine_version"], "analysis_modes": ["revenue", "counts"],
                "input_declarations_identical": True, "analysis_mode_compared_by_record": False,
                "unchanged_reused_checks": reused["reused_checks"], "changed_reader_rechecked_checks": rechecked["rechecked_checks"],
                "consumer_expected_mode_rejected": True, "unsupported_reader_setting_refused": True,
                "commands": commands, "preserved_artifacts": immutable,
                "limits": ["Native statements remain source_declared; no process monitoring.",
                           "Complete native bytes are bound; unmapped analysis mode is not compared with task intent.",
                           "Controlled fictional fixture example; no cache optimization or measured savings claim."]}
    save(workspace / "evidence.json", evidence)
    print(json.dumps({"status": "passed", "evidence": str(workspace / "evidence.json")}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--workspace", type=Path, required=True)
    args = parser.parse_args()
    run(args.binary, args.workspace)
