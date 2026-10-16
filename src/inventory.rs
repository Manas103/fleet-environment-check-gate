use crate::ssh::SshTarget;
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct HostEntry {
    pub host_id: String,
    pub os_family: String,
    pub ssh_host: String,
    pub ssh_port: u16,
    pub ssh_user: String,
    pub identity: String,
}

pub fn load_inventory(path: &std::path::Path) -> anyhow::Result<Vec<HostEntry>> {
    let mut rdr = csv::Reader::from_path(path)
        .map_err(|e| anyhow::anyhow!("reading inventory {:?}: {e}", path))?;
    let mut out = Vec::new();
    for rec in rdr.deserialize() {
        let entry: HostEntry = rec.map_err(|e| anyhow::anyhow!("parsing inventory row: {e}"))?;
        out.push(entry);
    }
    Ok(out)
}

impl HostEntry {
    pub fn target(&self) -> SshTarget {
        SshTarget {
            host: self.ssh_host.clone(),
            port: self.ssh_port,
            user: self.ssh_user.clone(),
            identity: std::path::PathBuf::from(&self.identity),
        }
    }
}
