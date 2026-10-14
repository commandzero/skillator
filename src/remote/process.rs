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

/// Capture a delivery subprocess without terminal output. Unlike host probes,
/// rsync inventory is unbounded and transfers have no artificial deadline.
pub(super) fn capture_transfer(
    command: &mut Command,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<Vec<u8>> {
    use std::io::Read;
    use std::os::unix::process::CommandExt;
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    if cancel.load(Ordering::Relaxed) {
        return Err(Error::input("Skill delivery cancelled"));
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = command.spawn().map_err(|error| {
        Error::input(format!(
            "cannot start {}: {error}",
            command.get_program().to_string_lossy()
        ))
    })?;
    fn read_pipe<R: Read + Send + 'static>(
        mut pipe: R,
        limit: Option<usize>,
    ) -> std::thread::JoinHandle<std::io::Result<Vec<u8>>> {
        std::thread::spawn(move || {
            let mut output = Vec::new();
            let mut buffer = [0u8; 8192];
            loop {
                let count = pipe.read(&mut buffer)?;
                if count == 0 {
                    return Ok(output);
                }
                let retain = limit.map_or(count, |max| max.saturating_sub(output.len()).min(count));
                output.extend_from_slice(&buffer[..retain]);
            }
        })
    }
    let stdout = read_pipe(child.stdout.take().expect("piped stdout"), None);
    // A broken remote can produce unlimited stderr. Retain useful diagnostics
    // while continuing to drain its pipe so it cannot block the transfer.
    let stderr = read_pipe(child.stderr.take().expect("piped stderr"), Some(65536));
    let mut status = None;
    let result = loop {
        if cancel.load(Ordering::Relaxed) {
            break Err(Error::input("Skill delivery cancelled"));
        }
        if status.is_none() {
            match child.try_wait() {
                Ok(found) => status = found,
                Err(error) => {
                    break Err(Error::input(format!(
                        "cannot wait for {}: {error}",
                        command.get_program().to_string_lossy()
                    )));
                }
            }
        }
        if let Some(finished_status) = status
            && stdout.is_finished()
            && stderr.is_finished()
        {
            break Ok(finished_status);
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    if result.is_err() {
        // SAFETY: process_group(0) created a private group led by this child;
        // a negative PID signals its descendants even if the leader already
        // exited but a descendant still holds a pipe open.
        if let Ok(group) = i32::try_from(child.id()) {
            unsafe { libc::kill(-group, libc::SIGKILL) };
        }
        let _ = child.wait();
    }
    let stdout = stdout
        .join()
        .map_err(|_| Error::input("delivery stdout reader failed"))?
        .map_err(|error| Error::input(format!("cannot read delivery stdout: {error}")))?;
    let stderr = stderr
        .join()
        .map_err(|_| Error::input("delivery stderr reader failed"))?
        .map_err(|error| Error::input(format!("cannot read delivery stderr: {error}")))?;
    let status = result?;
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::input("Skill delivery cancelled"));
    }
    if !status.success() {
        let diagnostic = sanitized(&stderr);
        return Err(Error::input(if diagnostic.is_empty() {
            format!(
                "{} failed with {status}",
                command.get_program().to_string_lossy()
            )
        } else {
            format!(
                "{} failed with {status}: {diagnostic}",
                command.get_program().to_string_lossy()
            )
        }));
    }
    Ok(stdout)
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

    #[test]
    fn transfer_keeps_entire_large_itemized_inventory() {
        let cancel = AtomicBool::new(false);
        let output = capture_transfer(
            Command::new("sh").args([
                "-c",
                "i=0; while [ \"$i\" -lt 12000 ]; do printf '>f+++++++++ path/%s\\n' \"$i\"; i=$((i+1)); done",
            ]),
            &cancel,
        )
        .unwrap();
        assert_eq!(output.iter().filter(|&&byte| byte == b'\n').count(), 12000);
    }

    #[test]
    fn transfer_captures_failure_without_writing_to_terminal() {
        let cancel = AtomicBool::new(false);
        let error = capture_transfer(
            Command::new("sh").args([
                "-c",
                "printf 'private inventory\\n'; printf 'receiver refused the transfer\\n' >&2; exit 23",
            ]),
            &cancel,
        )
        .unwrap_err();
        assert!(error.to_string().contains("receiver refused the transfer"));
        assert!(!error.to_string().contains("private inventory"));
    }

    #[test]
    fn transfer_cancellation_reaps_descendants_holding_pipes() {
        use std::fs;
        use std::sync::Arc;
        let root = tempfile::tempdir().unwrap();
        let ready = root.path().join("ready");
        let delayed = root.path().join("delayed");
        let signal = Arc::new(AtomicBool::new(false));
        let child_signal = signal.clone();
        let ready_path = ready.clone();
        let delayed_path = delayed.clone();
        let runner = std::thread::spawn(move || {
            capture_transfer(
                Command::new("sh")
                    .arg("-c")
                    .arg("sh -c 'sleep 1; printf escaped > \"$1\"' sh \"$2\" & printf ready > \"$1\"; wait")
                    .arg("sh")
                    .arg(ready_path)
                    .arg(delayed_path),
                &child_signal,
            )
        });
        let start = Instant::now();
        while !ready.exists() && start.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(ready.exists(), "subprocess did not begin");
        signal.store(true, std::sync::atomic::Ordering::Relaxed);
        let start = Instant::now();
        let result = runner.join().unwrap();
        assert!(result.unwrap_err().to_string().contains("cancelled"));
        assert!(start.elapsed() < Duration::from_secs(2));
        std::thread::sleep(Duration::from_millis(1200));
        assert!(!delayed.exists(), "descendant survived cancellation");
        assert_eq!(fs::read(&ready).unwrap(), b"ready");
    }
}
