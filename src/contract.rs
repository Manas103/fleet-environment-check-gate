use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Deserialize, Clone)]
pub struct Packages {
    pub ubuntu: BTreeMap<String, String>,
    pub rhel: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Contract {
    pub kernel: BTreeMap<String, String>,
    pub packages: Packages,
    pub ports: Vec<u16>,
    pub units: BTreeMap<String, String>,
}

impl Contract {
    pub fn load(path: &std::path::Path) -> anyhow::Result<Contract> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("reading contract {:?}: {e}", path))?;
        let contract: Contract = toml::from_str(&text)
            .map_err(|e| anyhow::anyhow!("parsing contract {:?}: {e}", path))?;
        Ok(contract)
    }

    pub fn packages_for(&self, os_family: &str) -> anyhow::Result<&BTreeMap<String, String>> {
        match os_family {
            "ubuntu" => Ok(&self.packages.ubuntu),
            "rhel" => Ok(&self.packages.rhel),
            other => Err(anyhow::anyhow!("no package contract for os family {other}")),
        }
    }
}
