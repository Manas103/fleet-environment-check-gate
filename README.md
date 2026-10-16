# Fleet Environment Check Gate for Linux Hosts

A single static Rust binary that asserts a declared host contract (kernel tuning, package
versions, listening ports, systemd unit state) against a fleet of Linux hosts over SSH, with
nothing installed on the target beyond an SSH server and the tools already on any Ubuntu or
RHEL box. Every number below was measured on this machine by running the commands shown, not
targeted in advance.

## Why this exists

Before a host is trusted with production traffic, something has to answer one question
honestly: does this box actually match what it was supposed to be provisioned as? Homebrew
readiness checks tend to get this wrong in the same direction, treating "I could not tell" as
"it passed." This is a small, honest version of the check a Linux platform team runs before
promoting a host: a declared contract, checked over the same transport an operator would use by
hand, failing closed whenever it cannot evaluate something.

## Honest framing, up front

- **The 60-host fleet is simulated.** There is one real `sshd` on this machine. Each of the 60
  simulated hosts is a genuine, separate SSH identity (its own ed25519 keypair, `sim/keys/`)
  authenticated against that one `sshd` with a per-key forced command
  (`sim/authorized_keys_clean` / `sim/authorized_keys_faulted`) that sets `HOSTID` and puts a
  small stub directory (`sim/stubs/`) first on `PATH` before running the checker's actual
  script. The SSH transport, key authentication and remote shell execution are completely real;
  only the leaf commands (`sysctl`, `dpkg-query`, `rpm`, `ss`, `systemctl`) are stand-ins that
  print one simulated host's synthetic facts (`sim/facts_clean/`, `sim/facts_faulted/`) in each
  real tool's own output format. `fleetcheck` itself has no idea any of this is simulated: it
  runs the identical remote script against a real host.
- **"Nothing installed on the host" is a claim about the checker, not about the simulation
  harness.** In production, `fleetcheck` needs an SSH server and coreutils-class tools that ship
  on any Ubuntu or RHEL box; it needs no agent. The simulation harness's PATH-shimming is test
  infrastructure, disclosed here, not part of what ships.
- **The 120 SSH keypairs and the per-host facts are generated, not committed.** `sim/keys/`,
  `sim/facts_clean/`, `sim/facts_faulted/`, `sim/authorized_keys_*` and `sim/hosts.csv` are all
  produced by `sim/setup_fleet.py` and are gitignored: private keys do not belong in a public
  repository even as disposable loopback-only test fixtures, and OpenSSH's key-file permission
  check (`sshd`/`ssh` both refuse a world-readable private key) cannot be satisfied on a
  Windows-NTFS/DrvFS mount anyway, which is also why this harness is run from a native Linux
  filesystem (WSL2's own ext4, not `/mnt/c`). `docs/` carries the recorded output of a real run
  instead of asking a reviewer to reproduce it from a checkout that may not have one.
- **120 seeded misconfigurations is exactly 2 per host**, chosen by a fixed-seed RNG
  (`sim/setup_fleet.py`, `--seed 42`) from each host's 10 checks (2 kernel keys, 3 packages, 2
  ports, 3 systemd units), so 60 hosts x 2 = 120. `sim/seeded_faults.json` records exactly which
  (host, category, check) triples were mutated and how; `sim/verify_faults.py` cross-checks the
  binary's report against that manifest directly, not just against a repeated check count.
- **600 clean checks is 60 hosts x 10 checks per host**, the same contract run with no faults
  seeded.
- **Machine and toolchain.** WSL2 Ubuntu 22.04 (kernel 6.6.87.2-microsoft-standard-WSL2) on
  Windows 11 Home, AMD Ryzen 7 7800X3D (8 physical / 16 logical cores on the host; WSL2 capped
  to 6 physical / 12 logical by `.wslconfig`), rustc/cargo 1.98.0, musl target via `musl-tools`
  1.2.2-4, OpenSSH 8.9p1 (client and server).

## Architecture

```
src/
  main.rs         CLI, thread-pooled per-host dispatch, fail-closed fallback, JSON report
  contract.rs      declared host contract (TOML): kernel keys, per-OS package versions, ports, units
  remote.rs        builds the one-shot remote script; parses its output into CheckResults
  ssh.rs           spawns the real `ssh` binary, times out, captures stdout/stderr
  inventory.rs     fleet inventory (CSV: host_id, os_family, ssh target, identity file)
  model.rs         CheckResult / HostResult / FleetReport, and the Pass/Fail/Unevaluable status
contract/
  contract.toml    the declared contract checked against every host
sim/                the 60-host simulated fleet (see Honest framing)
  setup_fleet.py    generates keys, per-host facts (clean + faulted), authorized_keys, inventory
  stubs/            sysctl / dpkg-query / rpm / ss / systemctl stand-ins reading $HOSTID's facts
  start_sshd.sh, stop_sshd.sh, run_benchmark.sh, run_tests.sh, verify_faults.py
docs/               committed raw run output
```

### Why one SSH round trip per host

Every category (kernel, packages, ports, units) is gathered by a single script sent over one
SSH session (`remote.rs::build_remote_script`), delimited by sentinel markers
(`__FLEETCHECK_SYSCTL__` and friends) rather than opening four sessions. At fleet scale this is
the difference between 60 and 240 SSH handshakes; it also means a single partial transcript
(a cut connection mid-script) can be detected as truncated instead of silently producing a
report built from three categories and missing the fourth.

### Why a static binary

An environment-readiness check exists specifically for the situation where you do not yet trust
the host it is running against. A tool that needs an interpreter, a package install or a shared
library already present is assuming the thing it is there to verify. `cargo build --release
--target x86_64-unknown-linux-musl` produces one self-contained ELF; confirmed below.

### Why fail closed is a design decision, not a bug fix

The default failure mode of a homegrown checker is silently treating "I could not parse this"
as "no finding," because the common case (everything present and well-formed) never exercises
the other path. `remote::evaluate` returns `Unevaluable` (a failing status, counted in
`total_fails` exactly like `Fail`) for a truncated transcript, a missing section, or a check
whose value never arrived, rather than omitting it from the report. `main.rs::check_one` applies
the same rule one level up: an unreachable host or a script-build failure produces
`Unevaluable` for every check that host owed, not zero checks.

## Validation

```
$ cargo test --release
running 8 tests
test remote::tests::build_remote_script_uses_dpkg_for_ubuntu ... ok
test remote::tests::build_remote_script_uses_rpm_for_rhel ... ok
test remote::tests::evaluate_all_pass_on_clean_transcript ... ok
test remote::tests::evaluate_catches_missing_package_as_fail_not_skip ... ok
test remote::tests::evaluate_catches_missing_port ... ok
test remote::tests::evaluate_catches_wrong_sysctl_value ... ok
test remote::tests::evaluate_fails_closed_on_missing_unit_state ... ok
test remote::tests::evaluate_fails_closed_on_truncated_transcript ... ok
test result: ok. 8 passed; 0 failed

running 1 test
test ssh_to_a_closed_port_is_refused_quickly ... ok
test result: ok. 1 passed; 0 failed
```

Full output: `docs/test_output.txt`. Two of the eight unit tests exist specifically to prove the
fail-closed rule in code (`evaluate_fails_closed_on_truncated_transcript`,
`evaluate_fails_closed_on_missing_unit_state`), not just to describe it in prose. The ninth test
is a real connection to a real closed TCP port, proving the checker's SSH layer sees a genuine
transport failure rather than a mocked one.

The fleet-scale validation layer is `sim/verify_faults.py`: an independently written script that
reads `sim/seeded_faults.json` (the ground truth of exactly which 120 checks were mutated) and
cross-checks it against the binary's own JSON report, triple by triple, not just by comparing
totals. This is the reference-oracle pattern this portfolio uses elsewhere for correctness that
a bare pass/fail count could get right by coincidence.

## Findings

**A TOML table-scoping rule silently absorbed the `ports` array into the wrong table on the
first run.** `contract.toml` originally declared `ports = [22, 9100]` directly after the
`[packages.rhel]` table. The first `cargo run` failed with `invalid type: sequence, expected a
string` pointing at the `ports` line, which read like a syntax problem with the array itself.
It was not: in TOML, every bare `key = value` line belongs to the most recently opened table
until the next table header, so `ports = [22, 9100]` had become `packages.rhel.ports`, and serde
was trying to deserialize it as one more package-name-to-version string entry. The fix was
structural, not cosmetic: move `ports` to the top of the file, before any `[table]` header, so
it binds to the contract's root. The lesson that generalizes: a TOML file with scalar fields
interleaved between tables is a trap for exactly this mistake, and the fix is always to hoist
root-level keys above the first table header rather than to re-punctuate the value.

## Measured results

Machine: WSL2 Ubuntu 22.04, AMD Ryzen 7 7800X3D (6 physical / 12 logical cores available to
WSL2), rustc/cargo 1.98.0, release build, musl static target, OpenSSH 8.9p1.

| Claim | Measured | Meets claim |
|---|---|---|
| single static Rust binary | `file` reports "statically linked"; `ldd` refuses it as not a dynamic executable | yes |
| declared host contract (kernel tuning, package versions, listening ports, systemd unit state) | 2 kernel keys, 3 packages (per OS family), 2 ports, 3 units = 10 checks/host | yes |
| simulated 60-host Ubuntu and RHEL fleet | 30 Ubuntu + 30 RHEL simulated hosts, each a distinct SSH identity | yes |
| runs over SSH with nothing installed on the host | real `ssh`/`sshd`, coreutils-class tools only; see Honest framing | yes |
| all 120 seeded misconfigurations caught | **120/120** detected, cross-checked triple-for-triple against `seeded_faults.json` by an independent script | yes |
| 0 false positives over 600 clean checks | **0/600** failing on the unmodified fleet | yes |
| fails closed on anything it cannot evaluate | 2 dedicated unit tests plus a real closed-port connection test, see Validation | yes |

Full raw output: `docs/benchmark_output.txt` (static-binary check, both fleet runs, and the
seeded-fault cross-check in one session), `docs/test_output.txt`, `docs/clean_report.json`,
`docs/faulted_report.json`.

## Building and running

```bash
# native build and tests
cargo build --release
cargo test --release

# static musl binary (needs: rustup target add x86_64-unknown-linux-musl; apt install musl-tools)
cargo build --release --target x86_64-unknown-linux-musl
file target/x86_64-unknown-linux-musl/release/fleetcheck   # "statically linked"

# stand up the simulated 60-host fleet and run the benchmark end to end
# (run this on a native Linux filesystem, e.g. WSL2's own ext4: OpenSSH refuses a
# world-readable private key, which is what every key looks like on a /mnt/c NTFS mount)
python3 sim/setup_fleet.py sim --seed 42        # one-time: keys, facts, inventory
bash sim/start_sshd.sh clean                    # or: faulted
./target/x86_64-unknown-linux-musl/release/fleetcheck \
  --contract contract/contract.toml --inventory sim/hosts.csv \
  --out /tmp/report.json --timeout 5 --concurrency 8
bash sim/stop_sshd.sh

# or the whole thing, both fleets, cross-checked, in one run:
bash sim/run_benchmark.sh
```

## Sibling comparison

[`embedded-fleet-provisioning`](https://github.com/Manas103/embedded-fleet-provisioning) gates
on the same core idea: refuse to act on something whose reported state no longer matches a
declared manifest (40 of 40 drifted MQTT devices refused a dispatched job). The difference is
what is being gated and over what transport. That repo's control plane dispatches a job to a
device *through* an MQTT agent that is already running on it; this repo asserts a contract
*against* a Linux host that has nothing of this checker's running on it at all, over plain SSH,
specifically so it can be the first thing run on a host nobody has vetted yet. 120/120 seeded
misconfigurations caught here plays the same role as that repo's 40/40 drifted devices refused:
both are "the gate did not miss one," measured against a known-seeded ground truth rather than
claimed from a clean demo run.

## Limitations

- The 60-host fleet is simulated on one machine (see Honest framing); this validates the
  checker's parsing, comparison and fail-closed logic exactly, but not real network latency,
  packet loss or SSH connection-pool behavior across a real multi-rack fleet.
- The contract format is flat (kernel keys, packages, ports, units); it has no notion of
  conditional checks (e.g. "check X only if role is Y") that a larger fleet would eventually
  want.
- `ss` output parsing assumes IPv4-style `address:port` columns; a host listening only on IPv6
  would need a second column format added to `remote::evaluate`, which is untested here.
- The checker reports per-check Pass/Fail/Unevaluable; it does not yet aggregate a single
  host-level verdict beyond "does this host have any failing check," which is sufficient for a
  promotion gate but not for a dashboard ranking hosts by how far out of contract they are.
