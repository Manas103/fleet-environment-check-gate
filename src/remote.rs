use crate::contract::Contract;
use crate::model::{CheckResult, Status};
use std::collections::BTreeMap;

pub const MARK_SYSCTL: &str = "__FLEETCHECK_SYSCTL__";
pub const MARK_PKG: &str = "__FLEETCHECK_PKG__";
pub const MARK_PORTS: &str = "__FLEETCHECK_PORTS__";
pub const MARK_UNITS: &str = "__FLEETCHECK_UNITS__";
pub const MARK_END: &str = "__FLEETCHECK_END__";

/// Builds the single remote command the checker sends over one SSH session. Everything the
/// host contract needs (kernel tuning, package versions, listening ports, systemd unit state)
/// is gathered in one round trip with coreutils-class tools already on any Ubuntu or RHEL box:
/// sysctl, dpkg-query or rpm, ss, systemctl. Nothing is installed on the target.
pub fn build_remote_script(contract: &Contract, os_family: &str) -> anyhow::Result<String> {
    let kernel_keys: Vec<&String> = contract.kernel.keys().collect();
    let pkgs = contract.packages_for(os_family)?;
    let pkg_names: Vec<&String> = pkgs.keys().collect();
    let unit_names: Vec<&String> = contract.units.keys().collect();

    let pkg_query = match os_family {
        "ubuntu" => format!(
            "dpkg-query -W -f='${{Package}} ${{Version}}\\n' {} 2>/dev/null",
            pkg_names
                .iter()
                .map(|p| p.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        ),
        "rhel" => format!(
            "rpm -q --qf '%{{NAME}} %{{VERSION}}-%{{RELEASE}}\\n' {} 2>/dev/null",
            pkg_names
                .iter()
                .map(|p| p.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        ),
        other => return Err(anyhow::anyhow!("no package query for os family {other}")),
    };

    let units_loop = unit_names
        .iter()
        .map(|u| {
            format!(
                "echo \"UNIT {u}\"; systemctl show {u} --property=ActiveState --no-pager 2>/dev/null"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    Ok(format!(
        r#"set -u
echo {MARK_SYSCTL}
sysctl {kernel_keys} 2>/dev/null
echo {MARK_PKG}
{pkg_query}
echo {MARK_PORTS}
ss -ltnH 2>/dev/null
echo {MARK_UNITS}
{units_loop}
echo {MARK_END}
"#,
        kernel_keys = kernel_keys
            .iter()
            .map(|k| k.as_str())
            .collect::<Vec<_>>()
            .join(" "),
    ))
}

#[derive(Debug, Default)]
struct Sections {
    sysctl: Option<Vec<String>>,
    pkg: Option<Vec<String>>,
    ports: Option<Vec<String>>,
    units: Option<Vec<String>>,
    ended: bool,
}

fn split_sections(raw: &str) -> Sections {
    let mut cur: Option<&str> = None;
    let mut buf: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    let mut ended = false;
    for line in raw.lines() {
        match line {
            MARK_SYSCTL => {
                cur = Some("sysctl");
                buf.entry("sysctl").or_default();
            }
            MARK_PKG => {
                cur = Some("pkg");
                buf.entry("pkg").or_default();
            }
            MARK_PORTS => {
                cur = Some("ports");
                buf.entry("ports").or_default();
            }
            MARK_UNITS => {
                cur = Some("units");
                buf.entry("units").or_default();
            }
            MARK_END => {
                ended = true;
                cur = None;
            }
            other => {
                if let Some(section) = cur {
                    buf.entry(section).or_default().push(other.to_string());
                }
            }
        }
    }
    Sections {
        sysctl: buf.remove("sysctl"),
        pkg: buf.remove("pkg"),
        ports: buf.remove("ports"),
        units: buf.remove("units"),
        ended,
    }
}

/// Parses the remote script's output and compares it against the contract. Any section that is
/// missing, or any value this parser cannot make sense of, becomes an `Unevaluable` (failing)
/// check rather than being silently dropped: this is what "fails closed" means in code.
pub fn evaluate(
    raw: &str,
    contract: &Contract,
    os_family: &str,
) -> anyhow::Result<Vec<CheckResult>> {
    let sections = split_sections(raw);
    let mut results = Vec::new();

    if !sections.ended {
        // The remote command never reached its final marker: treat every single check this
        // host owes as unevaluable rather than guessing from a partial transcript.
        return Ok(unevaluable_all(contract, os_family, "remote output truncated before __FLEETCHECK_END__"));
    }

    // Kernel tuning.
    let sysctl_lines = sections.sysctl.unwrap_or_default();
    let mut sysctl_actual: BTreeMap<String, String> = BTreeMap::new();
    for line in &sysctl_lines {
        if let Some((k, v)) = line.split_once('=') {
            sysctl_actual.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    for (key, expected) in &contract.kernel {
        let actual = sysctl_actual.get(key).cloned();
        results.push(match actual {
            Some(a) if a == *expected => CheckResult {
                category: "kernel".into(),
                name: key.clone(),
                expected: expected.clone(),
                actual: a,
                status: Status::Pass,
                reason: None,
            },
            Some(a) => CheckResult {
                category: "kernel".into(),
                name: key.clone(),
                expected: expected.clone(),
                actual: a,
                status: Status::Fail,
                reason: None,
            },
            None => CheckResult {
                category: "kernel".into(),
                name: key.clone(),
                expected: expected.clone(),
                actual: String::new(),
                status: Status::Unevaluable,
                reason: Some("sysctl key not reported".into()),
            },
        });
    }

    // Package versions.
    let pkg_lines = sections.pkg.unwrap_or_default();
    let mut pkg_actual: BTreeMap<String, String> = BTreeMap::new();
    for line in &pkg_lines {
        if let Some((name, version)) = line.split_once(' ') {
            pkg_actual.insert(name.trim().to_string(), version.trim().to_string());
        }
    }
    let expected_pkgs = contract.packages_for(os_family)?;
    for (name, expected) in expected_pkgs {
        let actual = pkg_actual.get(name).cloned();
        results.push(match actual {
            Some(a) if a == *expected => CheckResult {
                category: "package".into(),
                name: name.clone(),
                expected: expected.clone(),
                actual: a,
                status: Status::Pass,
                reason: None,
            },
            Some(a) => CheckResult {
                category: "package".into(),
                name: name.clone(),
                expected: expected.clone(),
                actual: a,
                status: Status::Fail,
                reason: None,
            },
            None => CheckResult {
                category: "package".into(),
                name: name.clone(),
                expected: expected.clone(),
                actual: String::new(),
                status: Status::Fail,
                reason: Some("package not installed".into()),
            },
        });
    }

    // Listening ports.
    let port_lines = sections.ports.unwrap_or_default();
    let mut listening: Vec<u16> = Vec::new();
    for line in &port_lines {
        // ss -ltnH columns: State Recv-Q Send-Q Local:Port Peer:Port [Process]
        if let Some(col) = line.split_whitespace().nth(3) {
            if let Some((_, port_str)) = col.rsplit_once(':') {
                if let Ok(port) = port_str.parse::<u16>() {
                    listening.push(port);
                }
            }
        }
    }
    for port in &contract.ports {
        let found = listening.contains(port);
        results.push(CheckResult {
            category: "port".into(),
            name: port.to_string(),
            expected: "LISTEN".into(),
            actual: if found { "LISTEN".into() } else { "not listening".into() },
            status: if found { Status::Pass } else { Status::Fail },
            reason: None,
        });
    }

    // systemd unit state.
    let unit_lines = sections.units.unwrap_or_default();
    let mut unit_actual: BTreeMap<String, String> = BTreeMap::new();
    let mut current_unit: Option<String> = None;
    for line in &unit_lines {
        if let Some(name) = line.strip_prefix("UNIT ") {
            current_unit = Some(name.trim().to_string());
        } else if let Some(state) = line.strip_prefix("ActiveState=") {
            if let Some(u) = current_unit.take() {
                unit_actual.insert(u, state.trim().to_string());
            }
        }
    }
    for (unit, expected) in &contract.units {
        let actual = unit_actual.get(unit).cloned();
        results.push(match actual {
            Some(a) if a == *expected => CheckResult {
                category: "systemd".into(),
                name: unit.clone(),
                expected: expected.clone(),
                actual: a,
                status: Status::Pass,
                reason: None,
            },
            Some(a) => CheckResult {
                category: "systemd".into(),
                name: unit.clone(),
                expected: expected.clone(),
                actual: a,
                status: Status::Fail,
                reason: None,
            },
            None => CheckResult {
                category: "systemd".into(),
                name: unit.clone(),
                expected: expected.clone(),
                actual: String::new(),
                status: Status::Unevaluable,
                reason: Some("unit state not reported".into()),
            },
        });
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{Contract, Packages};
    use std::collections::BTreeMap;

    fn test_contract() -> Contract {
        let mut kernel = BTreeMap::new();
        kernel.insert("net.ipv4.ip_forward".to_string(), "0".to_string());
        kernel.insert("vm.swappiness".to_string(), "10".to_string());
        let mut ubuntu = BTreeMap::new();
        ubuntu.insert("openssh-server".to_string(), "1:8.9p1-3ubuntu0.17".to_string());
        let mut rhel = BTreeMap::new();
        rhel.insert("openssh-server".to_string(), "8.7p1-34.el9_3".to_string());
        let mut units = BTreeMap::new();
        units.insert("sshd".to_string(), "active".to_string());
        Contract {
            kernel,
            packages: Packages { ubuntu, rhel },
            ports: vec![22],
            units,
        }
    }

    #[test]
    fn build_remote_script_uses_dpkg_for_ubuntu() {
        let c = test_contract();
        let script = build_remote_script(&c, "ubuntu").unwrap();
        assert!(script.contains("dpkg-query"));
        assert!(!script.contains("rpm -q"));
    }

    #[test]
    fn build_remote_script_uses_rpm_for_rhel() {
        let c = test_contract();
        let script = build_remote_script(&c, "rhel").unwrap();
        assert!(script.contains("rpm -q"));
        assert!(!script.contains("dpkg-query"));
    }

    #[test]
    fn evaluate_all_pass_on_clean_transcript() {
        let c = test_contract();
        let raw = format!(
            "{MARK_SYSCTL}\nnet.ipv4.ip_forward = 0\nvm.swappiness = 10\n{MARK_PKG}\nopenssh-server 1:8.9p1-3ubuntu0.17\n{MARK_PORTS}\nLISTEN 0      128          0.0.0.0:22        0.0.0.0:*\n{MARK_UNITS}\nUNIT sshd\nActiveState=active\n{MARK_END}\n"
        );
        let results = evaluate(&raw, &c, "ubuntu").unwrap();
        assert_eq!(results.len(), 5); // 2 kernel + 1 package + 1 port + 1 unit
        assert!(results.iter().all(|r| r.status == Status::Pass));
    }

    #[test]
    fn evaluate_catches_wrong_sysctl_value() {
        let c = test_contract();
        let raw = format!(
            "{MARK_SYSCTL}\nnet.ipv4.ip_forward = 1\nvm.swappiness = 10\n{MARK_PKG}\nopenssh-server 1:8.9p1-3ubuntu0.17\n{MARK_PORTS}\nLISTEN 0      128          0.0.0.0:22        0.0.0.0:*\n{MARK_UNITS}\nUNIT sshd\nActiveState=active\n{MARK_END}\n"
        );
        let results = evaluate(&raw, &c, "ubuntu").unwrap();
        let forward = results
            .iter()
            .find(|r| r.name == "net.ipv4.ip_forward")
            .unwrap();
        assert_eq!(forward.status, Status::Fail);
    }

    #[test]
    fn evaluate_catches_missing_port() {
        let c = test_contract();
        let raw = format!(
            "{MARK_SYSCTL}\nnet.ipv4.ip_forward = 0\nvm.swappiness = 10\n{MARK_PKG}\nopenssh-server 1:8.9p1-3ubuntu0.17\n{MARK_PORTS}\n{MARK_UNITS}\nUNIT sshd\nActiveState=active\n{MARK_END}\n"
        );
        let results = evaluate(&raw, &c, "ubuntu").unwrap();
        let port = results.iter().find(|r| r.category == "port").unwrap();
        assert_eq!(port.status, Status::Fail);
    }

    #[test]
    fn evaluate_catches_missing_package_as_fail_not_skip() {
        let c = test_contract();
        let raw = format!(
            "{MARK_SYSCTL}\nnet.ipv4.ip_forward = 0\nvm.swappiness = 10\n{MARK_PKG}\n{MARK_PORTS}\nLISTEN 0      128          0.0.0.0:22        0.0.0.0:*\n{MARK_UNITS}\nUNIT sshd\nActiveState=active\n{MARK_END}\n"
        );
        let results = evaluate(&raw, &c, "ubuntu").unwrap();
        let pkg = results.iter().find(|r| r.category == "package").unwrap();
        assert_eq!(pkg.status, Status::Fail);
    }

    /// Fails closed: a transcript that never reaches the final marker (a connection cut mid
    /// stream, a crashed remote shell) must mark every check this host owes as Unevaluable,
    /// not silently drop them from the report.
    #[test]
    fn evaluate_fails_closed_on_truncated_transcript() {
        let c = test_contract();
        let raw = format!("{MARK_SYSCTL}\nnet.ipv4.ip_forward = 0\n");
        let results = evaluate(&raw, &c, "ubuntu").unwrap();
        assert_eq!(results.len(), 5);
        assert!(results.iter().all(|r| r.status == Status::Unevaluable));
    }

    /// Fails closed: a unit whose state line never arrived (systemctl hung, or was itself
    /// missing on the target) is Unevaluable, not assumed healthy.
    #[test]
    fn evaluate_fails_closed_on_missing_unit_state() {
        let c = test_contract();
        let raw = format!(
            "{MARK_SYSCTL}\nnet.ipv4.ip_forward = 0\nvm.swappiness = 10\n{MARK_PKG}\nopenssh-server 1:8.9p1-3ubuntu0.17\n{MARK_PORTS}\nLISTEN 0      128          0.0.0.0:22        0.0.0.0:*\n{MARK_UNITS}\n{MARK_END}\n"
        );
        let results = evaluate(&raw, &c, "ubuntu").unwrap();
        let unit = results.iter().find(|r| r.category == "systemd").unwrap();
        assert_eq!(unit.status, Status::Unevaluable);
    }
}

fn unevaluable_all(contract: &Contract, os_family: &str, reason: &str) -> Vec<CheckResult> {
    let mut results = Vec::new();
    for key in contract.kernel.keys() {
        results.push(CheckResult {
            category: "kernel".into(),
            name: key.clone(),
            expected: contract.kernel[key].clone(),
            actual: String::new(),
            status: Status::Unevaluable,
            reason: Some(reason.to_string()),
        });
    }
    if let Ok(pkgs) = contract.packages_for(os_family) {
        for (name, expected) in pkgs {
            results.push(CheckResult {
                category: "package".into(),
                name: name.clone(),
                expected: expected.clone(),
                actual: String::new(),
                status: Status::Unevaluable,
                reason: Some(reason.to_string()),
            });
        }
    }
    for port in &contract.ports {
        results.push(CheckResult {
            category: "port".into(),
            name: port.to_string(),
            expected: "LISTEN".into(),
            actual: String::new(),
            status: Status::Unevaluable,
            reason: Some(reason.to_string()),
        });
    }
    for unit in contract.units.keys() {
        results.push(CheckResult {
            category: "systemd".into(),
            name: unit.clone(),
            expected: contract.units[unit].clone(),
            actual: String::new(),
            status: Status::Unevaluable,
            reason: Some(reason.to_string()),
        });
    }
    results
}
