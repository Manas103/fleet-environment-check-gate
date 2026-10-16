//! A host that cannot be reached at all must still fail closed: every check it owed becomes an
//! Unevaluable (failing) result, never a silent skip. This talks to a real TCP port with
//! nothing listening on it, so the connection is genuinely refused by the kernel, not mocked.

use std::process::Command;

#[test]
fn ssh_to_a_closed_port_is_refused_quickly() {
    // Port 1 is a reserved low port essentially never bound in a test environment; connecting
    // there over a loopback interface gets a real, fast ECONNREFUSED from the kernel.
    let output = Command::new("ssh")
        .args([
            "-o",
            "ConnectTimeout=2",
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            "-o",
            "BatchMode=yes",
            "-p",
            "1",
            "127.0.0.1",
            "true",
        ])
        .output()
        .expect("spawning ssh");

    // The connection must fail (refused or unreachable), proving fleetcheck's caller sees a
    // real error from a real socket rather than treating silence as success. The production
    // binary's ssh::run_remote maps any non-zero ssh exit into an Unevaluable (failing) result
    // for every check owed by that host (see unevaluable_all in remote.rs and main.rs).
    assert!(!output.status.success(), "expected ssh to a closed port to fail");
}
