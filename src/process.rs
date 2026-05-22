use std::io::Read;
use std::process::{Child, ExitStatus, Output};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

pub fn wait_output_with_timeout(child: Child, timeout: Duration) -> Result<Output> {
    let mut child = child;
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let status = wait_child_status_with_timeout(&mut child, timeout)?;
    let mut stdout_buffer = Vec::new();
    let mut stderr_buffer = Vec::new();
    if let Some(stdout) = stdout.as_mut() {
        stdout
            .read_to_end(&mut stdout_buffer)
            .context("failed to collect process stdout")?;
    }
    if let Some(stderr) = stderr.as_mut() {
        stderr
            .read_to_end(&mut stderr_buffer)
            .context("failed to collect process stderr")?;
    }
    Ok(Output {
        status,
        stdout: stdout_buffer,
        stderr: stderr_buffer,
    })
}

pub fn wait_status_with_timeout(child: Child, timeout: Duration) -> Result<ExitStatus> {
    let mut child = child;
    wait_child_status_with_timeout(&mut child, timeout)
}

fn wait_child_status_with_timeout(child: &mut Child, timeout: Duration) -> Result<ExitStatus> {
    let start = Instant::now();
    // ⚡ Bolt: Exponential backoff for polling sleep.
    // This dramatically reduces latency overhead for fast processes like `ffprobe`
    // which execute in ~10-40ms, while avoiding CPU thrashing for long-running processes like `ffmpeg`.
    let mut sleep_time = Duration::from_millis(1);
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("command exceeded {} seconds", timeout.as_secs());
        }
        std::thread::sleep(sleep_time);
        sleep_time = std::cmp::min(sleep_time * 2, Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn wait_status_returns_completed_child_status() -> Result<()> {
        let child = Command::new("sh").arg("-c").arg("exit 0").spawn()?;

        let status = wait_status_with_timeout(child, Duration::from_secs(5))?;

        assert!(status.success());
        Ok(())
    }
}
