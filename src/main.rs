mod contract;
mod inventory;
mod model;
mod remote;
mod ssh;

use clap::Parser;
use contract::Contract;
use inventory::load_inventory;
use model::{FleetReport, HostResult, Status};
use std::path::PathBuf;
use std::sync::mpsc;

/// fleetcheck: a single static binary that asserts a declared host contract (kernel tuning,
/// package versions, listening ports, systemd unit state) against a fleet of Linux hosts over
/// SSH. Nothing is installed on the target beyond an SSH server and the standard tools already
/// on any Ubuntu or RHEL box.
#[derive(Parser)]
#[command(name = "fleetcheck")]
struct Cli {
    /// Path to the host contract (TOML).
    #[arg(long)]
    contract: PathBuf,
    /// Path to the fleet inventory (CSV: host_id,os_family,ssh_host,ssh_port,ssh_user,identity).
    #[arg(long)]
    inventory: PathBuf,
    /// Path to write the JSON fleet report.
    #[arg(long)]
    out: PathBuf,
    /// Per-host SSH connect timeout in seconds.
    #[arg(long, default_value_t = 5)]
    timeout: u64,
    /// Number of hosts checked concurrently.
    #[arg(long, default_value_t = 8)]
    concurrency: usize,
}

fn check_one(
    entry: &inventory::HostEntry,
    contract: &Contract,
    timeout: u64,
) -> HostResult {
    let target = entry.target();
    let script = match remote::build_remote_script(contract, &entry.os_family) {
        Ok(s) => s,
        Err(e) => {
            return HostResult {
                host_id: entry.host_id.clone(),
                os_family: entry.os_family.clone(),
                reachable: false,
                checks: unreachable_checks(contract, &format!("building remote script: {e}")),
            }
        }
    };

    match ssh::run_remote(&target, &script, timeout) {
        Ok(raw) => {
            let checks = match remote::evaluate(&raw, contract, &entry.os_family) {
                Ok(c) => c,
                Err(e) => unreachable_checks(contract, &format!("evaluating output: {e}")),
            };
            HostResult {
                host_id: entry.host_id.clone(),
                os_family: entry.os_family.clone(),
                reachable: true,
                checks,
            }
        }
        Err(e) => HostResult {
            host_id: entry.host_id.clone(),
            os_family: entry.os_family.clone(),
            reachable: false,
            checks: unreachable_checks(contract, &format!("ssh session failed: {e}")),
        },
    }
}

/// Fail-closed fallback: a host this checker cannot reach or cannot parse owes every check in
/// the contract a Fail-class result (Unevaluable), never a silent skip.
fn unreachable_checks(contract: &Contract, reason: &str) -> Vec<model::CheckResult> {
    let mut out = Vec::new();
    for key in contract.kernel.keys() {
        out.push(model::CheckResult {
            category: "kernel".into(),
            name: key.clone(),
            expected: contract.kernel[key].clone(),
            actual: String::new(),
            status: Status::Unevaluable,
            reason: Some(reason.to_string()),
        });
    }
    for name in contract.packages.ubuntu.keys().chain(contract.packages.rhel.keys()) {
        out.push(model::CheckResult {
            category: "package".into(),
            name: name.clone(),
            expected: String::new(),
            actual: String::new(),
            status: Status::Unevaluable,
            reason: Some(reason.to_string()),
        });
    }
    for port in &contract.ports {
        out.push(model::CheckResult {
            category: "port".into(),
            name: port.to_string(),
            expected: "LISTEN".into(),
            actual: String::new(),
            status: Status::Unevaluable,
            reason: Some(reason.to_string()),
        });
    }
    for unit in contract.units.keys() {
        out.push(model::CheckResult {
            category: "systemd".into(),
            name: unit.clone(),
            expected: contract.units[unit].clone(),
            actual: String::new(),
            status: Status::Unevaluable,
            reason: Some(reason.to_string()),
        });
    }
    out
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let contract = Contract::load(&cli.contract)?;
    let hosts = load_inventory(&cli.inventory)?;

    let pool = threadpool::ThreadPool::new(cli.concurrency.max(1));
    let (tx, rx) = mpsc::channel();
    let n = hosts.len();
    for entry in hosts {
        let tx = tx.clone();
        let contract = contract.clone();
        let timeout = cli.timeout;
        pool.execute(move || {
            let result = check_one(&entry, &contract, timeout);
            tx.send(result).expect("send host result");
        });
    }
    drop(tx);

    let mut results = Vec::with_capacity(n);
    for _ in 0..n {
        results.push(rx.recv()?);
    }
    results.sort_by(|a, b| a.host_id.cmp(&b.host_id));

    let report = FleetReport::from_hosts(results);
    std::fs::write(&cli.out, serde_json::to_string_pretty(&report)?)?;

    println!(
        "fleetcheck: {} hosts, {} checks, {} failing (report: {})",
        report.hosts.len(),
        report.total_checks,
        report.total_fails,
        cli.out.display()
    );

    if report.total_fails > 0 {
        std::process::exit(1);
    }
    Ok(())
}
