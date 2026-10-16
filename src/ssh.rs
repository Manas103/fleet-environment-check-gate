use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

pub struct SshTarget {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub identity: std::path::PathBuf,
}

/// Runs `script` on the target over a real SSH session (the real `ssh` binary, real TCP
/// connection, real authentication) and returns stdout. The checker needs nothing installed
/// on the target beyond an SSH server and the standard tools the script calls.
pub fn run_remote(target: &SshTarget, script: &str, timeout_secs: u64) -> anyhow::Result<String> {
    let mut child = Command::new("ssh")
        .arg("-i")
        .arg(&target.identity)
        .arg("-p")
        .arg(target.port.to_string())
        .arg("-o")
        .arg("StrictHostKeyChecking=no")
        .arg("-o")
        .arg("UserKnownHostsFile=/dev/null")
        .arg("-o")
        .arg("LogLevel=ERROR")
        .arg("-o")
        .arg(format!("ConnectTimeout={timeout_secs}"))
        .arg(format!("{}@{}", target.user, target.host))
        .arg("bash")
        .arg("-s")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow::anyhow!("spawning ssh: {e}"))?;

    child
        .stdin
        .take()
        .expect("stdin piped")
        .write_all(script.as_bytes())?;

    let output = wait_with_timeout(child, Duration::from_secs(timeout_secs + 5))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("ssh exited with {:?}: {}", output.status.code(), stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn wait_with_timeout(
    mut child: std::process::Child,
    timeout: Duration,
) -> anyhow::Result<std::process::Output> {
    let start = std::time::Instant::now();
    loop {
        match child.try_wait()? {
            Some(status) => {
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                if let Some(mut s) = child.stdout.take() {
                    use std::io::Read;
                    let _ = s.read_to_end(&mut stdout);
                }
                if let Some(mut s) = child.stderr.take() {
                    use std::io::Read;
                    let _ = s.read_to_end(&mut stderr);
                }
                return Ok(std::process::Output { status, stdout, stderr });
            }
            None => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    return Err(anyhow::anyhow!("ssh session timed out after {timeout:?}"));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}
