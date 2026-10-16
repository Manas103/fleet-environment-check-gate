use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Status {
    Pass,
    Fail,
    /// The checker could not evaluate this check at all (parse failure, unreachable host,
    /// missing section in the remote output). Counted as a failure, never skipped: this is
    /// the fail-closed rule.
    Unevaluable,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    pub category: String,
    pub name: String,
    pub expected: String,
    pub actual: String,
    pub status: Status,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostResult {
    pub host_id: String,
    pub os_family: String,
    pub reachable: bool,
    pub checks: Vec<CheckResult>,
}

impl HostResult {
    pub fn fail_count(&self) -> usize {
        self.checks
            .iter()
            .filter(|c| c.status != Status::Pass)
            .count()
    }
}

#[derive(Debug, Serialize)]
pub struct FleetReport {
    pub hosts: Vec<HostResult>,
    pub total_checks: usize,
    pub total_fails: usize,
}

impl FleetReport {
    pub fn from_hosts(hosts: Vec<HostResult>) -> FleetReport {
        let total_checks: usize = hosts.iter().map(|h| h.checks.len()).sum();
        let total_fails: usize = hosts.iter().map(|h| h.fail_count()).sum();
        FleetReport {
            hosts,
            total_checks,
            total_fails,
        }
    }
}
