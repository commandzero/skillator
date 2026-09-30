use super::{Error, Result};
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub(super) const LIMIT: usize = 16 * 1024 * 1024;
pub(super) const TIMEOUT: Duration = Duration::from_secs(120);

pub(super) fn capture(command: &mut Command) -> Result<Vec<u8>> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(Error::input_display)?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (send, recv) = mpsc::channel();
    for (index, mut pipe) in [
        (0, Box::new(stdout) as Box<dyn Read + Send>),
        (1, Box::new(stderr) as Box<dyn Read + Send>),
    ] {
        let send = send.clone();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = pipe
                .by_ref()
                .take((LIMIT + 1) as u64)
                .read_to_end(&mut bytes)
                .map(|_| bytes)
                .map_err(|error| error.to_string());
            let _ = send.send((index, result));
        });
    }
    drop(send);
    let deadline = Instant::now() + TIMEOUT;
    let result = (|| {
        let mut output = [Vec::new(), Vec::new()];
        for _ in 0..2 {
            let (index, bytes) = recv
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|_| Error::input("subprocess timed out"))?;
            let bytes = bytes.map_err(Error::input)?;
            if bytes.len() > LIMIT {
                return Err(Error::input("subprocess output exceeds the size limit"));
            }
            output[index] = bytes;
        }
        loop {
            if let Some(status) = child.try_wait().map_err(Error::input_display)? {
                if !status.success() {
                    return Err(Error::input(format!(
                        "{} subprocess failed with {status}",
                        command.get_program().to_string_lossy()
                    )));
                }
                return Ok(std::mem::take(&mut output[0]));
            }
            if Instant::now() >= deadline {
                return Err(Error::input("subprocess timed out"));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    })();
    if result.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    result
}

pub(super) fn git(root: &std::path::Path, args: &[&str]) -> Result<Vec<u8>> {
    capture(
        Command::new("git")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_TERMINAL_PROMPT", "0")
            .args(["-c", "core.hooksPath=/dev/null", "-C"])
            .arg(root)
            .args(args),
    )
}
