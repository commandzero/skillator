use super::{
    Error, Result, process,
    session::{Request, Response, Session},
};
use crate::app::AppPaths;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};

pub(super) enum Endpoint {
    Local(Box<Session>),
    Ssh(Remote),
    #[cfg(test)]
    Fault {
        inner: Box<Endpoint>,
        fault: Fault,
    },
}

#[cfg(test)]
#[derive(Clone, Copy)]
pub(super) enum Fault {
    Transfer,
    Publish,
    Verification,
    Acknowledge,
    LockBusy,
    Begin,
    InspectActive,
    AdvanceGitOnBegin,
}

pub(super) struct Remote {
    child: Child,
    input: Sender<Vec<u8>>,
    written: Receiver<std::result::Result<(), String>>,
    output: Receiver<std::result::Result<Vec<u8>, String>>,
    destination: String,
}

const SSH_OPTIONS: &[&str] = &[
    "-oBatchMode=yes",
    "-oStrictHostKeyChecking=yes",
    "-oUpdateHostKeys=no",
    "-oControlMaster=no",
    "-oControlPath=none",
    "-oControlPersist=no",
    "-oConnectTimeout=15",
    "-oServerAliveInterval=15",
    "-oServerAliveCountMax=2",
];

impl Endpoint {
    pub fn local(paths: AppPaths) -> Self {
        Self::Local(Box::new(Session::new(paths)))
    }

    pub fn ssh(destination: &str) -> Result<Self> {
        let mut child = Command::new("ssh").args(SSH_OPTIONS).arg("--").arg(destination)
            .arg("if command -v skillator >/dev/null 2>&1; then exec skillator __rsync; else printf '%s\\n' '{\"result\":\"error\",\"code\":3,\"message\":\"Skillator is not installed on remote host\"}'; fi")
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
            .spawn().map_err(Error::input_display)?;
        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();
        // Drain diagnostics without echoing network destinations or credential-bearing messages.
        std::thread::spawn(move || {
            let _ = std::io::copy(
                &mut stderr.by_ref().take(process::LIMIT as u64),
                &mut std::io::sink(),
            );
        });
        let (input, requests) = mpsc::channel::<Vec<u8>>();
        let (written_tx, written) = mpsc::channel();
        std::thread::spawn(move || {
            while let Ok(bytes) = requests.recv() {
                let result = stdin
                    .write_all(&bytes)
                    .and_then(|()| stdin.flush())
                    .map_err(|error| error.to_string());
                let failed = result.is_err();
                if written_tx.send(result).is_err() || failed {
                    break;
                }
            }
        });
        let (output_tx, output) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let result = read_frame(&mut reader).map_err(|error| error.to_string());
                let failed = result.is_err();
                if output_tx.send(result).is_err() || failed {
                    break;
                }
            }
        });
        Ok(Self::Ssh(Remote {
            child,
            input,
            written,
            output,
            destination: destination.to_owned(),
        }))
    }

    pub fn request(&mut self, request: Request) -> Result<Response> {
        let response = match self {
            Self::Local(session) => session.handle(request)?,
            #[cfg(test)]
            Self::Fault { inner, fault } => {
                if let Self::Local(session) = inner.as_mut() {
                    if matches!(request, Request::Finish)
                        && matches!(fault, Fault::Begin | Fault::InspectActive)
                    {
                        std::fs::write(session.paths.home().join("finish-observed"), "yes")
                            .unwrap();
                    }
                    if matches!((&request, *fault), (Request::Begin { .. }, Fault::Begin))
                        || (matches!(
                            (&request, *fault),
                            (Request::Inspect { .. }, Fault::InspectActive)
                        ) && session
                            .paths
                            .home()
                            .join(".skillator/rsync/state.json")
                            .exists())
                    {
                        return Err(Error::input("injected session failure"));
                    }
                    if matches!(
                        (&request, *fault),
                        (Request::Begin { .. }, Fault::AdvanceGitOnBegin)
                    ) {
                        let response = session.handle(request)?;
                        process::git(
                            &session.paths.home().join("Development/acme/skills"),
                            &["checkout", "next", "--"],
                        )?;
                        return Ok(response);
                    }
                }
                if matches!((&request, *fault), (Request::Lock { .. }, Fault::LockBusy)) {
                    return Err(Error::busy());
                }
                if matches!(
                    (&request, *fault),
                    (
                        Request::Publish {
                            desired: Some(super::state::Entry::File { .. }),
                            ..
                        },
                        Fault::Publish
                    ) | (Request::Acknowledge { .. }, Fault::Acknowledge)
                ) {
                    return Err(Error::input("injected operation failure"));
                }
                if let (
                    Request::Publish {
                        stage: Some(path),
                        desired: Some(super::state::Entry::File { .. }),
                        ..
                    },
                    Fault::Verification,
                ) = (&request, *fault)
                {
                    std::fs::write(path, "corrupted staged content").unwrap();
                }
                return inner.request(request);
            }
            Self::Ssh(remote) => {
                let mut bytes = serde_json::to_vec(&request).map_err(Error::input_display)?;
                if bytes.len() > process::LIMIT {
                    return Err(Error::input("protocol request exceeds size limit"));
                }
                bytes.push(b'\n');
                remote
                    .input
                    .send(bytes)
                    .map_err(|_| Error::input("remote input closed"))?;
                let written = remote
                    .written
                    .recv_timeout(process::TIMEOUT)
                    .map_err(|_| Error::input("remote request timed out"))?;
                let result = receive_response(&remote.output, written)?;
                let bytes = match result {
                    Ok(bytes) => bytes,
                    Err(_) => {
                        return Err(probe_version(remote));
                    }
                };
                serde_json::from_slice(&bytes).map_err(|_| {
                    Error::input(
                        "invalid remote response or incompatible Skillator; protocol 2 is required",
                    )
                })?
            }
        };
        match response {
            Response::Error { code, message } => Err(Error { code, message }),
            response => Ok(response),
        }
    }

    pub fn pull(&self, exported: &str, local: &std::path::Path) -> Result<()> {
        transfer(self, exported, local, false)
    }

    pub fn push(&self, local: &std::path::Path, staged: &str) -> Result<()> {
        transfer(self, staged, local, true)
    }
}

fn receive_response(
    output: &Receiver<std::result::Result<Vec<u8>, String>>,
    written: std::result::Result<(), String>,
) -> Result<std::result::Result<Vec<u8>, String>> {
    if written.is_err() {
        // Preserve an already returned missing-installation diagnostic, but never
        // wait the full response timeout after a failed write.
        return match output.recv_timeout(std::time::Duration::from_millis(250)) {
            Ok(Ok(bytes)) => Ok(Ok(bytes)),
            _ => Err(Error::input(
                "remote request write failed; SSH input closed",
            )),
        };
    }
    output
        .recv_timeout(process::TIMEOUT)
        .map_err(|_| Error::input("remote response timed out"))
}

fn probe_version(remote: &Remote) -> Error {
    let result = process::capture(Command::new("ssh").args(SSH_OPTIONS).arg("--").arg(&remote.destination)
        .arg("if command -v skillator >/dev/null 2>&1; then skillator --version; else printf '%s\\n' '__skillator_missing__'; fi"));
    match result {
        Ok(bytes) => {
            let version: String = String::from_utf8_lossy(&bytes)
                .chars()
                .filter(|c| !c.is_control())
                .take(256)
                .collect();
            if version == "__skillator_missing__" {
                Error::input("Skillator is not installed on remote host")
            } else {
                Error::input(format!(
                    "Skillator version {version:?} is incompatible; synchronization protocol 2 is required on remote host"
                ))
            }
        }
        Err(_) => Error::input(
            "remote connection failed; verify SSH authentication, host trust, and Skillator installation on remote host",
        ),
    }
}

fn transfer(
    endpoint: &Endpoint,
    endpoint_path: &str,
    local: &std::path::Path,
    push: bool,
) -> Result<()> {
    #[cfg(test)]
    if let Endpoint::Fault { inner, fault } = endpoint {
        if matches!(fault, Fault::Transfer) {
            return Err(Error::input("injected transfer failure"));
        }
        return transfer(inner, endpoint_path, local, push);
    }
    let mut command = Command::new("rsync");
    command.args([
        "--links",
        "--perms",
        "--times",
        "--checksum",
        "--timeout=60",
    ]);
    let endpoint_arg = match endpoint {
        #[cfg(test)]
        Endpoint::Fault { .. } => unreachable!(),
        Endpoint::Local(_) => endpoint_path.to_owned(),
        Endpoint::Ssh(remote) => {
            command
                .arg("-e")
                .arg(format!("ssh {}", SSH_OPTIONS.join(" ")));
            // rsync passes this path through the remote shell; quote it as one literal.
            format!("{}:{}", remote.destination, shell_quote(endpoint_path))
        }
    };
    command.arg("--");
    if push {
        command.arg(local).arg(endpoint_arg);
    } else {
        command.arg(endpoint_arg).arg(local);
    }
    process::capture(&mut command)?;
    Ok(())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

impl Drop for Remote {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn read_frame(reader: &mut impl BufRead) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take((process::LIMIT + 1) as u64)
        .read_until(b'\n', &mut bytes)
        .map_err(Error::input_display)?;
    if bytes.last() != Some(&b'\n') || bytes.len() > process::LIMIT {
        return Err(Error::input("closed or oversized protocol frame"));
    }
    Ok(bytes)
}

pub(crate) fn serve(paths: AppPaths) -> Result<()> {
    let mut session = Session::new(paths);
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    loop {
        if input.fill_buf().map_err(Error::input_display)?.is_empty() {
            return Ok(());
        }
        let response = match read_frame(&mut input)
            .and_then(|bytes| serde_json::from_slice(&bytes).map_err(Error::input_display))
            .and_then(|request| session.handle(request))
        {
            Ok(response) => response,
            Err(error) => Response::Error {
                code: error.code,
                message: error.to_string(),
            },
        };
        let mut bytes = serde_json::to_vec(&response).map_err(Error::input_display)?;
        if bytes.len() > process::LIMIT {
            return Err(Error::input("protocol response exceeds size limit"));
        }
        bytes.push(b'\n');
        output
            .write_all(&bytes)
            .and_then(|()| output.flush())
            .map_err(Error::input_display)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_write_does_not_wait_for_the_full_response_timeout() {
        let (sender, output) = mpsc::channel();
        let started = std::time::Instant::now();
        assert!(
            receive_response(&output, Err("broken pipe".into()))
                .unwrap_err()
                .message
                .contains("write failed")
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        sender
            .send(Ok(b"missing Skillator diagnostic".to_vec()))
            .unwrap();
        assert_eq!(
            receive_response(&output, Err("broken pipe".into()))
                .unwrap()
                .unwrap(),
            b"missing Skillator diagnostic"
        );
    }

    #[test]
    fn protocol_frames_are_bounded_and_shell_paths_stay_literal() {
        assert!(read_frame(&mut std::io::Cursor::new(b"unterminated")).is_err());
        let mut oversized = vec![b'x'; process::LIMIT];
        oversized.push(b'\n');
        assert!(read_frame(&mut std::io::Cursor::new(oversized)).is_err());
        let path = "/home/a space/quote's $(literal) `also literal`";
        let output = process::capture(
            Command::new("sh")
                .arg("-c")
                .arg(format!("printf %s {}", shell_quote(path))),
        )
        .unwrap();
        assert_eq!(output, path.as_bytes());
    }
}
