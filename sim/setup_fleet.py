#!/usr/bin/env python3
"""Builds the simulated 60-host Ubuntu/RHEL fleet that fleetcheck is benchmarked against.

Each simulated host is a real SSH identity (its own ed25519 keypair) authenticated against one
real sshd instance running as the current user, with a per-key forced command in
authorized_keys that sets HOSTID and a stub PATH before running the client's actual script.
The SSH transport, authentication and remote shell execution are all real; only the leaf
commands the script calls (sysctl, dpkg-query, rpm, ss, systemctl) are stand-ins that print a
per-host synthetic fact file in the real tool's output format. This is disclosed in the README.

Usage: python3 setup_fleet.py <sim_dir> [--seed N]
"""
import argparse
import json
import os
import random
import stat
import subprocess
import sys

CONTRACT = {
    "kernel": {"net.ipv4.ip_forward": "0", "vm.swappiness": "10"},
    "packages": {
        "ubuntu": {
            "openssh-server": "1:8.9p1-3ubuntu0.17",
            "chrony": "4.2-2ubuntu2",
            "auditd": "1:3.0.6-2.1ubuntu1.1",
        },
        "rhel": {
            "openssh-server": "8.7p1-34.el9_3",
            "chrony": "4.3-1.el9",
            "audit": "3.1.2-2.el9",
        },
    },
    "ports": [22, 9100],
    "units": {"sshd": "active", "chronyd": "active", "node_exporter": "active"},
}

N_UBUNTU = 30
N_RHEL = 30


def check_items(os_family):
    items = []
    for k in CONTRACT["kernel"]:
        items.append(("kernel", k))
    for p in CONTRACT["packages"][os_family]:
        items.append(("package", p))
    for port in CONTRACT["ports"]:
        items.append(("port", str(port)))
    for u in CONTRACT["units"]:
        items.append(("systemd", u))
    return items


def clean_facts(os_family):
    return {
        "os_family": os_family,
        "kernel": dict(CONTRACT["kernel"]),
        "packages": dict(CONTRACT["packages"][os_family]),
        "ports": list(CONTRACT["ports"]),
        "units": dict(CONTRACT["units"]),
    }


def apply_fault(facts, category, name):
    """Mutates one (category, name) check into a detectable wrong value. Returns (old, new)."""
    if category == "kernel":
        old = facts["kernel"][name]
        new = "1" if old != "1" else "0"
        facts["kernel"][name] = new
        return old, new
    if category == "package":
        old = facts["packages"][name]
        new = old + "-stale1"
        facts["packages"][name] = new
        return old, new
    if category == "port":
        port = int(name)
        old = list(facts["ports"])
        facts["ports"] = [p for p in facts["ports"] if p != port]
        return str(old), str(facts["ports"])
    if category == "systemd":
        old = facts["units"][name]
        new = "inactive"
        facts["units"][name] = new
        return old, new
    raise ValueError(category)


def gen_hosts():
    hosts = []
    for i in range(1, N_UBUNTU + 1):
        hosts.append((f"host{i:03d}", "ubuntu"))
    for i in range(N_UBUNTU + 1, N_UBUNTU + N_RHEL + 1):
        hosts.append((f"host{i:03d}", "rhel"))
    return hosts


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("sim_dir")
    ap.add_argument("--seed", type=int, default=42)
    args = ap.parse_args()

    sim_dir = os.path.abspath(args.sim_dir)
    keys_dir = os.path.join(sim_dir, "keys")
    facts_clean_dir = os.path.join(sim_dir, "facts_clean")
    facts_faulted_dir = os.path.join(sim_dir, "facts_faulted")
    for d in (sim_dir, keys_dir, facts_clean_dir, facts_faulted_dir):
        os.makedirs(d, exist_ok=True)

    hosts = gen_hosts()
    auth_lines_clean = []
    auth_lines_faulted = []
    inventory_rows = ["host_id,os_family,ssh_host,ssh_port,ssh_user,identity"]
    seeded_faults = []

    ssh_user = subprocess.check_output(["id", "-un"]).decode().strip()
    stub_dir = os.path.join(sim_dir, "stubs")

    for host_id, os_family in hosts:
        priv = os.path.join(keys_dir, host_id)
        pub = priv + ".pub"
        if not os.path.exists(priv):
            subprocess.check_call(
                ["ssh-keygen", "-t", "ed25519", "-N", "", "-C", host_id, "-f", priv],
                stdout=subprocess.DEVNULL,
            )
        os.chmod(priv, stat.S_IRUSR | stat.S_IWUSR)
        pubkey_line = open(pub).read().strip()

        clean = clean_facts(os_family)
        with open(os.path.join(facts_clean_dir, f"{host_id}.json"), "w") as f:
            json.dump(clean, f, indent=2)

        faulted = clean_facts(os_family)
        rng = random.Random(args.seed + hash(host_id) % 100000)
        items = check_items(os_family)
        faults = rng.sample(items, 2)
        for category, name in faults:
            old, new = apply_fault(faulted, category, name)
            seeded_faults.append(
                {"host_id": host_id, "category": category, "name": name, "old": old, "new": new}
            )
        with open(os.path.join(facts_faulted_dir, f"{host_id}.json"), "w") as f:
            json.dump(faulted, f, indent=2)

        # Paths are single-quoted inside the forced command because the repo's own path (and
        # therefore FACTS_DIR / PATH) can contain spaces.
        cmd_clean = (
            f"command=\"HOSTID={host_id} FACTS_DIR='{facts_clean_dir}' "
            f"PATH='{stub_dir}':$PATH $SSH_ORIGINAL_COMMAND\",no-port-forwarding,"
            f"no-X11-forwarding,no-agent-forwarding,no-pty {pubkey_line}"
        )
        cmd_faulted = (
            f"command=\"HOSTID={host_id} FACTS_DIR='{facts_faulted_dir}' "
            f"PATH='{stub_dir}':$PATH $SSH_ORIGINAL_COMMAND\",no-port-forwarding,"
            f"no-X11-forwarding,no-agent-forwarding,no-pty {pubkey_line}"
        )
        auth_lines_clean.append(cmd_clean)
        auth_lines_faulted.append(cmd_faulted)
        # Identity path is relative to the repo root (fleetcheck is run from there); this keeps
        # hosts.csv portable across checkouts instead of baking in this machine's absolute path.
        identity_rel = os.path.relpath(priv, os.path.dirname(sim_dir))
        inventory_rows.append(f"{host_id},{os_family},127.0.0.1,2847,{ssh_user},{identity_rel}")

    with open(os.path.join(sim_dir, "authorized_keys_clean"), "w") as f:
        f.write("\n".join(auth_lines_clean) + "\n")
    with open(os.path.join(sim_dir, "authorized_keys_faulted"), "w") as f:
        f.write("\n".join(auth_lines_faulted) + "\n")
    with open(os.path.join(sim_dir, "hosts.csv"), "w") as f:
        f.write("\n".join(inventory_rows) + "\n")
    with open(os.path.join(sim_dir, "seeded_faults.json"), "w") as f:
        json.dump(seeded_faults, f, indent=2)

    print(f"{len(hosts)} hosts, {len(seeded_faults)} seeded faults, seed={args.seed}")


if __name__ == "__main__":
    main()
