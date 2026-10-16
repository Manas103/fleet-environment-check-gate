#!/usr/bin/env python3
"""Independent cross-check: compares fleetcheck's JSON report against the ground-truth
seeded-fault manifest triple by triple, not just by comparing totals."""
import json
import os

SIM_DIR = os.path.dirname(os.path.abspath(__file__))
DIR = os.path.dirname(SIM_DIR)

with open(os.path.join(SIM_DIR, "seeded_faults.json")) as f:
    seeded = json.load(f)
with open(os.path.join(DIR, "docs", "faulted_report.json")) as f:
    report = json.load(f)

seeded_set = {(s["host_id"], s["category"], s["name"]) for s in seeded}

detected_fail = set()
false_positive = []
for host in report["hosts"]:
    for c in host["checks"]:
        if c["status"] != "Pass":
            key = (host["host_id"], c["category"], c["name"])
            detected_fail.add(key)
            if key not in seeded_set:
                false_positive.append(key)

missed = seeded_set - detected_fail
print(f"seeded faults: {len(seeded_set)}")
print(f"detected fails: {len(detected_fail)}")
print(f"missed (seeded but not caught): {len(missed)}")
print(f"false positives (caught but not seeded): {len(false_positive)}")
if missed:
    print("MISSED:", list(missed)[:10])
if false_positive:
    print("FALSE POSITIVES:", false_positive[:10])
