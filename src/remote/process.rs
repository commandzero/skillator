use super::{Error, Result};
use std::process::{Command, Stdio};

pub(super) fn capture(command: &mut Command) -> Result<Vec<u8>> {
    let output = command
        .stdin(Stdio::null())
        .output()
        .map_err(Error::input_display)?;
    if !output.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
    }
    if !output.status.success() {
        return Err(Error::input(format!(
            "{} failed with {}; see subprocess diagnostics on stderr",
            command.get_program().to_string_lossy(),
            output.status
        )));
    }
    Ok(output.stdout)
}

#[derive(Debug)]
pub(super) struct BoundedOutput {
    pub stdout: Vec<u8>,
    pub warning: Option<String>,
}

pub(super) fn sanitized(bytes: &[u8]) -> String {
    let mut result: String = String::from_utf8_lossy(bytes)
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .skip_while(|character| character.is_whitespace())
        .take(512)
        .collect();
    result.truncate(result.trim_end().len());
    result
}

pub(super) fn capture_bounded(
    command: &mut Command,
    cancel: &std::sync::atomic::AtomicBool,
    deadline: std::time::Duration,
    stdout_limit: usize,
    stderr_limit: usize,
) -> std::result::Result<BoundedOutput, String> {
    use std::io::Read;
    use std::os::unix::process::CommandExt;
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};

    if cancel.load(Ordering::Relaxed) {
        return Err("SSH operation cancelled".to_owned());
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = command
        .spawn()
        .map_err(|error| format!("cannot start SSH: {error}"))?;
    let exceeded = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let finished = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    fn spawn_reader<R: Read + Send + 'static>(
        mut pipe: R,
        limit: usize,
        exceeded: std::sync::Arc<std::sync::atomic::AtomicBool>,
        finished: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) -> std::thread::JoinHandle<std::io::Result<Vec<u8>>> {
        std::thread::spawn(move || {
            let result = (|| {
                let mut output = Vec::new();
                let mut buffer = [0u8; 8192];
                loop {
                    let count = pipe.read(&mut buffer)?;
                    if count == 0 {
                        break;
                    }
                    if output.len().saturating_add(count) > limit {
                        exceeded.store(true, Ordering::Relaxed);
                        break;
                    }
                    output.extend_from_slice(&buffer[..count]);
                }
                Ok(output)
            })();
            finished.fetch_add(1, Ordering::Relaxed);
            result
        })
    }
    let stdout = spawn_reader(
        child.stdout.take().expect("piped stdout"),
        stdout_limit,
        exceeded.clone(),
        finished.clone(),
    );
    let stderr = spawn_reader(
        child.stderr.take().expect("piped stderr"),
        stderr_limit,
        exceeded.clone(),
        finished.clone(),
    );
    let started = Instant::now();
    let mut status = None;
    let result = loop {
        if cancel.load(Ordering::Relaxed) {
            break Err("SSH operation cancelled".to_owned());
        }
        if exceeded.load(Ordering::Relaxed) {
            break Err("SSH response exceeds the safe output limit".to_owned());
        }
        if started.elapsed() >= deadline {
            break Err("SSH operation timed out".to_owned());
        }
        if status.is_none() {
            match child.try_wait() {
                Ok(found) => status = found,
                Err(error) => break Err(format!("cannot wait for SSH: {error}")),
            }
        }
        if let Some(status) = status
            && finished.load(Ordering::Relaxed) == 2
        {
            break Ok(status);
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    // The SSH process is the process-group leader. Also stop any remote-command
    // adapter children that inherited its stdout/stderr, so the reader threads
    // cannot remain blocked after cancellation or a deadline.
    if result.is_err() {
        let group = i32::try_from(child.id()).map_err(|_| "invalid SSH process id")?;
        // SAFETY: negative PID addresses only the private process group created above.
        unsafe { libc::kill(-group, libc::SIGKILL) };
        let _ = child.wait();
    }
    let stdout = stdout
        .join()
        .map_err(|_| "SSH stdout reader failed".to_owned())?
        .map_err(|error| format!("cannot read SSH stdout: {error}"))?;
    let stderr = stderr
        .join()
        .map_err(|_| "SSH stderr reader failed".to_owned())?
        .map_err(|error| format!("cannot read SSH stderr: {error}"))?;
    let status = result?;
    if exceeded.load(Ordering::Relaxed) {
        return Err("SSH response exceeds the safe output limit".to_owned());
    }
    let diagnostic = sanitized(&stderr);
    if !status.success() {
        return Err(if diagnostic.is_empty() {
            format!("SSH exited with {status}; check host trust, credentials and connection")
        } else {
            format!("SSH exited with {status}: {diagnostic}")
        });
    }
    Ok(BoundedOutput {
        stdout,
        warning: (!diagnostic.is_empty()).then_some(diagnostic),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, Instant};

    #[test]
    fn command_status_and_warnings_remain_separate_from_stdout() {
        let cancel = AtomicBool::new(false);
        let result = capture_bounded(
            Command::new("sh").args([
                "-c",
                "printf 'worker-07\\n'; printf '\\033[31mwarning\\n' >&2",
            ]),
            &cancel,
            Duration::from_secs(2),
            4096,
            4096,
        )
        .unwrap();
        assert_eq!(result.stdout, b"worker-07\n");
        assert!(result.warning.unwrap().contains("warning"));
        let error = capture_bounded(
            Command::new("sh").args(["-c", "printf 'worker-07\\n'; echo denied >&2; exit 1"]),
            &cancel,
            Duration::from_secs(2),
            4096,
            4096,
        )
        .err()
        .unwrap();
        assert!(error.contains("denied"));
        assert!(!error.contains("worker-07"));
    }

    #[test]
    fn oversized_output_timeout_and_cancellation_are_bounded() {
        let cancel = AtomicBool::new(false);
        assert!(
            capture_bounded(
                Command::new("sh").args(["-c", "printf '123456789'"]),
                &cancel,
                Duration::from_secs(2),
                4,
                4096,
            )
            .unwrap_err()
            .contains("limit")
        );
        let start = Instant::now();
        assert!(
            capture_bounded(
                Command::new("sh").args(["-c", "sleep 5 & wait"]),
                &cancel,
                Duration::from_millis(200),
                4096,
                4096,
            )
            .unwrap_err()
            .contains("timed out")
        );
        assert!(start.elapsed() < Duration::from_secs(2));
        let signal = std::sync::Arc::new(AtomicBool::new(false));
        let setter = signal.clone();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            setter.store(true, std::sync::atomic::Ordering::Relaxed);
        });
        let start = Instant::now();
        assert!(
            capture_bounded(
                Command::new("sh").args(["-c", "sleep 5 & wait"]),
                &signal,
                Duration::from_secs(10),
                4096,
                4096,
            )
            .unwrap_err()
            .contains("cancelled")
        );
        thread.join().unwrap();
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}
