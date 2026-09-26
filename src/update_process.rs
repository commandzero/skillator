//! Bounded Unix subprocess ownership for Library pulls.
use std::io::{self, Read};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

static CANCELLED: AtomicBool = AtomicBool::new(false);
static SIGNAL_OWNER: Mutex<()> = Mutex::new(());

extern "C" fn interrupt(_: libc::c_int) {
    CANCELLED.store(true, Ordering::Relaxed);
}

pub struct InterruptGuard {
    previous: libc::sigaction,
    _owner: MutexGuard<'static, ()>,
}

impl InterruptGuard {
    pub fn install() -> io::Result<Self> {
        let owner = SIGNAL_OWNER
            .lock()
            .map_err(|_| io::Error::other("signal owner poisoned"))?;
        CANCELLED.store(false, Ordering::Relaxed);
        // SAFETY: sigaction structs are valid when zero-initialized; both pointers
        // remain live for the call. The handler only stores to a lock-free atomic.
        let previous = unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            let mut previous = std::mem::zeroed();
            action.sa_sigaction = interrupt as *const () as usize;
            libc::sigemptyset(&mut action.sa_mask);
            if libc::sigaction(libc::SIGINT, &action, &mut previous) != 0 {
                return Err(io::Error::last_os_error());
            }
            previous
        };
        Ok(Self {
            previous,
            _owner: owner,
        })
    }

    pub fn cancelled(&self) -> bool {
        CANCELLED.load(Ordering::Relaxed)
    }
}

impl Drop for InterruptGuard {
    fn drop(&mut self) {
        // SAFETY: previous was initialized by sigaction and remains live. The
        // ownership mutex prevents overlapping installs through this module.
        unsafe {
            libc::sigaction(libc::SIGINT, &self.previous, std::ptr::null_mut());
        }
    }
}

pub enum Completion {
    Exited(ExitStatus),
    Timeout,
    Cancelled,
}

pub struct Captured {
    pub completion: Completion,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

fn nonblocking(pipe: &impl AsRawFd) -> io::Result<()> {
    // SAFETY: the borrowed pipe keeps this descriptor open throughout both calls.
    unsafe {
        let flags = libc::fcntl(pipe.as_raw_fd(), libc::F_GETFL);
        if flags < 0 || libc::fcntl(pipe.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

fn signal_group(pid: u32, signal: libc::c_int) {
    // SAFETY: process_group(0) creates a group whose ID is the live child's PID.
    // Negative PID addresses only that group, never the parent's group.
    unsafe {
        libc::kill(-(pid as libc::pid_t), signal);
    }
}

fn drain(pipe: &mut impl Read, bytes: &mut Vec<u8>) -> io::Result<()> {
    let mut chunk = [0; 8192];
    // Bound each drain pass so constant output cannot starve timeout checks.
    for _ in 0..16 {
        match pipe.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => {
                let keep = count.min(65536_usize.saturating_sub(bytes.len()));
                bytes.extend_from_slice(&chunk[..keep]);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

pub fn capture(
    command: &mut Command,
    timeout: Duration,
    interrupt: &InterruptGuard,
) -> io::Result<Captured> {
    if interrupt.cancelled() {
        return Ok(Captured {
            completion: Completion::Cancelled,
            stdout: vec![],
            stderr: vec![],
        });
    }
    let start = Instant::now();
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()?;
    let pid = child.id();
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut reaped = false;
    let result: io::Result<Completion> = (|| {
        nonblocking(&stdout)?;
        nonblocking(&stderr)?;
        loop {
            drain(&mut stdout, &mut out)?;
            drain(&mut stderr, &mut err)?;
            let reason = if interrupt.cancelled() {
                Some(Completion::Cancelled)
            } else if start.elapsed() >= timeout {
                Some(Completion::Timeout)
            } else {
                None
            };
            if let Some(reason) = reason {
                signal_group(pid, libc::SIGTERM);
                // Do not reap before killing the remaining group: retaining the
                // direct child prevents its PID from being reused during cleanup.
                std::thread::sleep(Duration::from_millis(100));
                signal_group(pid, libc::SIGKILL);
                child.wait()?;
                reaped = true;
                drain(&mut stdout, &mut out)?;
                drain(&mut stderr, &mut err)?;
                return Ok(reason);
            }
            if let Some(status) = child.try_wait()? {
                reaped = true;
                drain(&mut stdout, &mut out)?;
                drain(&mut stderr, &mut err)?;
                return Ok(Completion::Exited(status));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    })();
    if result.is_err() && !reaped {
        signal_group(pid, libc::SIGKILL);
        let _ = child.wait();
    }
    Ok(Captured {
        completion: result?,
        stdout: out,
        stderr: err,
    })
}
