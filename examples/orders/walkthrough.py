#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Run with an absolute rowtide binary and a NEW workspace; stdlib only."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


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
    commands, immutable = [], {}

    def command(name, argv, success=True):
        result = subprocess.run([str(a) for a in argv], cwd=workspace,
                                capture_output=True, text=True)
        (logs / (name + ".stdout")).write_text(result.stdout)
        (logs / (name + ".stderr")).write_text(result.stderr)
        entry = {"name": name, "argv": [str(a) for a in argv], "cwd": str(workspace),
                 "returncode": result.returncode, "expected_success": success,
                 "stdout": str(logs / (name + ".stdout")), "stderr": str(logs / (name + ".stderr"))}
        commands.append(entry)
        (logs / (name + ".command.json")).write_text(json.dumps(entry, indent=2) + "\n")
        assert (result.returncode == 0) == success, entry
        return json.loads(result.stdout) if success else None

    def local(name, operation, *args):
        summary = command(name, [binary, operation, *args, "--out", workspace / name])
        receipt_path = workspace / name / "receipt.json"
        receipt = json.loads(receipt_path.read_bytes())
        for artifact in (workspace / name).iterdir():
            if artifact.is_file():
                immutable[str(artifact)] = sha(artifact)
        return receipt_path, receipt, summary

    def consume(name, handoff, success=True):
        command(name, [sys.executable, inputs / "consumer.py", handoff, workspace / name], success)
        if not success:
            assert not (workspace / name).exists(), "rejected consumer emitted output"
            return None
        for artifact in (workspace / name).iterdir():
            immutable[str(artifact)] = sha(artifact)
        return json.loads((workspace / name / "report.json").read_bytes())

    def expected(updated=False):
        return {"period": "2026-09", "amount_unit": "USD cents", "stores": [
            {"store": "north", "order_count": 2, "revenue_usd_cents": 195},
            {"store": "south", "order_count": 2 if updated else 1, "revenue_usd_cents": 150 if updated else 90},
            {"store": "west", "order_count": 1, "revenue_usd_cents": 150}],
            "total_order_count": 5 if updated else 4,
            "total_revenue_usd_cents": 495 if updated else 435}

    def matched(receipt, summary):
        assert len(receipt["consumption"]) == 2
        assert all(f["status"] == "matched" and f["origin"] == "source_declared"
                   for f in receipt["consumption"])
        assert summary["rechecked_checks"] == summary["reused_checks"] == 0
        assert summary["carried_checks"] == 5

    request = inputs / "report-request.json"
    absent_path, absent, _ = local("00-missing-file", "prepare", "--request", request)
    assert absent["bindings"]["input:orders"]["state"] != "bound"
    assert any(f["status"] == "unknown" for f in absent["findings"])
    consume("00-missing-file-consumer", absent_path, False)
    shutil.copyfile(fixtures / "orders.csv", inputs / "orders.csv")
    missing_path, missing, _ = local("01-missing-west", "refresh", "--previous", absent_path)
    coverage = next(f for f in missing["findings"] if f["id"] == "store_coverage")
    assert coverage["status"] == "mismatch"
    assert coverage["details"]["missing_distinct_key_count"] == 1
    assert coverage["details"]["missing_examples"] == [["west"]]
    assert missing["next_actions"]
    consume("01-missing-west-consumer", missing_path, False)
    shutil.copyfile(fixtures / "orders-complete.csv", inputs / "orders.csv")
    ready_path, ready, ready_counts = local("02-corrected", "refresh", "--previous", missing_path)
    assert all(f["status"] == "supported" for f in ready["findings"])
    assert (ready_counts["rechecked_checks"], ready_counts["reused_checks"]) == (3, 2)
    first_report = consume("03-consumed", ready_path)
    assert first_report == expected()
    _, first_record, first_record_counts = local("04-recorded", "record", "--previous", ready_path,
                                               "--actual", workspace / "03-consumed" / "mapping.json")
    matched(first_record, first_record_counts)
    shutil.copyfile(fixtures / "orders-updated.csv", inputs / "orders.csv")
    consume("05-stale-handoff-rejected", ready_path, False)
    assert "input differs from handoff: orders" in (logs / "05-stale-handoff-rejected.stderr").read_text()
    updated_path, updated, update_counts = local("06-updated", "refresh", "--previous", ready_path)
    assert (update_counts["rechecked_checks"], update_counts["reused_checks"]) == (3, 2)
    reuse = {f["id"]: f["details"]["reuse"] for f in updated["findings"]}
    assert {name for name, decision in reuse.items() if decision["decision"] == "reused"} == {"store_columns", "unique_stores"}
    assert all(reuse[name]["reason_code"] == "input_content_changed_or_unbound"
               for name in ("order_columns", "unique_orders", "store_coverage"))
    assert updated["freshness"]["input:orders"] == "changed"
    assert updated["freshness"]["input:stores"] == "unchanged"
    updated_report = consume("07-consumed", updated_path)
    assert updated_report == expected(True)
    _, updated_record, updated_record_counts = local("08-recorded", "record", "--previous", updated_path,
                                                   "--actual", workspace / "07-consumed" / "mapping.json")
    matched(updated_record, updated_record_counts)
    _, unchanged, unchanged_counts = local("09-unchanged", "refresh", "--previous", updated_path)
    assert (unchanged_counts["rechecked_checks"], unchanged_counts["reused_checks"]) == (0, 5)
    assert all(sha(Path(path)) == value for path, value in immutable.items()), "prior artifacts changed"
    evidence = {"status": "passed", "binary": str(binary), "binary_sha256": sha(binary),
                "engine_version": updated["engine_version"], "workspace": str(workspace),
                "missing_file_binding": absent["bindings"]["input:orders"],
                "missing_west_finding": coverage, "first_report": first_report,
                "updated_report": updated_report, "corrected_counts": ready_counts,
                "stale_handoff_rejected_without_output": True, "update_counts": update_counts,
                "update_reuse": reuse, "unchanged_counts": unchanged_counts,
                "first_consumption": first_record["consumption"],
                "updated_consumption": updated_record["consumption"],
                "immutable_artifact_sha256": immutable, "commands": commands,
                "limits": ["Native receipt statements remain source_declared; record compares fields and does not observe process compliance.",
                           "Tiny fictional static CSV exports; no transactional snapshot or domain-validity claim."]}
    target = workspace / "evidence.json"
    target.write_text(json.dumps(evidence, indent=2) + "\n")
    print(json.dumps({"status": "passed", "evidence": str(target), "report": updated_report}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--workspace", required=True, type=Path)
    args = parser.parse_args()
    run(args.binary, args.workspace)
