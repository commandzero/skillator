use super::{
    Error, Result, process,
    session::{Request, Response, Session},
};
use crate::app::AppPaths;
use crate::fs_safety::{Directory, bind_directory};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
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
    Register,
    LockBusy,
    Begin,
    StageOutside,
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
            let _ = drain_diagnostics(&mut stderr);
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
                    if matches!(
                        (&request, *fault),
                        (Request::Begin { .. }, Fault::StageOutside)
                    ) {
                        let mut response = session.handle(request)?;
                        if let Response::Begun { stage, .. } = &mut response {
                            *stage = "/tmp/skillator-outside-stage".into();
                        }
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
                        | (Request::Register { .. }, Fault::Register)
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
                        "invalid remote response or incompatible Skillator; protocol 4 is required",
                    )
                })?
            }
        };
        checked_response(response)
    }

    pub fn pull(
        &self,
        exported: &str,
        local: &Path,
        source_identity: (u64, u64),
        local_identity: (u64, u64),
    ) -> Result<()> {
        transfer(
            self,
            exported,
            local,
            false,
            source_identity,
            local_identity,
        )
    }

    pub fn push(
        &mut self,
        local: &Path,
        staged: &str,
        local_identity: (u64, u64),
        target_identity: (u64, u64),
    ) -> Result<()> {
        if !matches!(self.request(Request::ValidateStage)?, Response::Ok) {
            return Err(Error::input("unexpected stage validation response"));
        }
        transfer(self, staged, local, true, target_identity, local_identity)?;
        if !matches!(self.request(Request::ValidateStage)?, Response::Ok) {
            return Err(Error::input("unexpected stage validation response"));
        }
        Ok(())
    }
}

fn checked_response(response: Response) -> Result<Response> {
    match response {
        Response::Error {
            code: code @ 2..=4,
            message,
        } => Err(Error { code, message }),
        Response::Error { .. } => Err(Error::input(
            "invalid remote error status or incompatible Skillator; protocol 4 is required",
        )),
        response => Ok(response),
    }
}

fn drain_diagnostics(input: &mut impl Read) -> std::io::Result<u64> {
    std::io::copy(input, &mut std::io::sink())
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
                    "Skillator version {version:?} is incompatible; synchronization protocol 4 is required on remote host"
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
    local: &Path,
    push: bool,
    endpoint_identity: (u64, u64),
    local_identity: (u64, u64),
) -> Result<()> {
    #[cfg(test)]
    if let Endpoint::Fault { inner, fault } = endpoint {
        if matches!(fault, Fault::Transfer) {
            return Err(Error::input("injected transfer failure"));
        }
        return transfer(
            inner,
            endpoint_path,
            local,
            push,
            endpoint_identity,
            local_identity,
        );
    }
    let local_stage = open_transfer_stage(local, local_identity)?;
    let local_name = transfer_name(local)?;
    let endpoint_path = Path::new(endpoint_path);
    let endpoint_name = transfer_name(endpoint_path)?;
    if matches!(endpoint, Endpoint::Local(_)) {
        let endpoint_stage = open_transfer_stage(endpoint_path, endpoint_identity)?;
        let (source, source_name, target, target_name) = if push {
            (&local_stage, local_name, &endpoint_stage, endpoint_name)
        } else {
            (&endpoint_stage, endpoint_name, &local_stage, local_name)
        };
        return copy_local_entry(source, source_name, target, target_name);
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
        Endpoint::Local(_) => unreachable!(),
        Endpoint::Ssh(remote) => {
            command
                .arg("-e")
                .arg(format!("ssh {}", SSH_OPTIONS.join(" ")));
            let stage = endpoint_path.parent().unwrap();
            command.arg("--rsync-path").arg(format!(
                "skillator __rsync-server --stage-hex {} --device {} --inode {} --name {} --",
                hex_encode(stage.as_os_str().as_bytes()),
                endpoint_identity.0,
                endpoint_identity.1,
                endpoint_name.to_string_lossy(),
            ));
            // rsync passes this path through the remote shell; quote it as one literal.
            format!(
                "{}:{}",
                remote.destination,
                shell_quote(&endpoint_path.to_string_lossy())
            )
        }
    };
    bind_directory(&mut command, local_stage.as_file());
    command.arg("--");
    if push {
        command.arg(local_name).arg(endpoint_arg);
    } else {
        command.arg(endpoint_arg).arg(local_name);
    }
    process::capture(&mut command)?;
    Ok(())
}

fn transfer_name(path: &Path) -> Result<&std::ffi::OsStr> {
    let name = path
        .file_name()
        .ok_or_else(|| Error::input("invalid transfer path"))?;
    if !super::state::valid_id(&name.to_string_lossy()) {
        return Err(Error::input("invalid transfer entry name"));
    }
    Ok(name)
}

fn open_transfer_stage(path: &Path, identity: (u64, u64)) -> Result<Directory> {
    let stage = path
        .parent()
        .ok_or_else(|| Error::input("invalid transfer stage"))?;
    let root = stage
        .parent()
        .ok_or_else(|| Error::input("invalid transfer stage"))?;
    if root.file_name() != Some(std::ffi::OsStr::new("rsync"))
        || root.parent().and_then(Path::file_name) != Some(std::ffi::OsStr::new(".skillator"))
        || !stage
            .file_name()
            .unwrap()
            .to_string_lossy()
            .strip_prefix("stage-")
            .is_some_and(super::state::valid_id)
    {
        return Err(Error::input("invalid transfer stage"));
    }
    let home = root.parent().unwrap().parent().unwrap();
    let relative = stage.strip_prefix(home).map_err(Error::input_display)?;
    let canonical_home = home.canonicalize().map_err(Error::input_display)?;
    let directory =
        Directory::open_existing_parent(&canonical_home, &canonical_home.join(relative))
            .map_err(Error::input_display)?;
    if directory.identity().map_err(Error::input_display)? != identity {
        return Err(Error::input(
            "synchronization stage changed after Begin; abort and recover",
        ));
    }
    Ok(directory)
}

fn copy_local_entry(
    source: &Directory,
    source_name: &std::ffi::OsStr,
    target: &Directory,
    target_name: &std::ffi::OsStr,
) -> Result<()> {
    let metadata = source.metadata(source_name).map_err(Error::input_display)?;
    match metadata.st_mode & libc::S_IFMT {
        libc::S_IFREG => {
            let mut input = source
                .open_file(source_name)
                .map_err(Error::input_display)?;
            let mut output = target
                .create_file(target_name)
                .map_err(Error::input_display)?;
            std::io::copy(&mut input, &mut output).map_err(Error::input_display)?;
            output
                .set_permissions(std::fs::Permissions::from_mode(u32::from(
                    metadata.st_mode & 0o777,
                )))
                .map_err(Error::input_display)?;
            output.sync_all().map_err(Error::input_display)?;
        }
        libc::S_IFLNK => {
            let link = source
                .read_link(source_name)
                .map_err(Error::input_display)?;
            target
                .symlink(target_name, &link)
                .map_err(Error::input_display)?;
        }
        _ => return Err(Error::input("unsupported transfer entry")),
    }
    Ok(())
}

pub(crate) fn serve_transfer(
    stage_hex: &str,
    device: u64,
    inode: u64,
    name: &std::ffi::OsStr,
    server_args: &[std::ffi::OsString],
) -> Result<()> {
    let stage_text = String::from_utf8(hex_decode(stage_hex)?).map_err(Error::input_display)?;
    let stage = Path::new(&stage_text);
    let path = stage.join(name);
    transfer_name(&path)?;
    if server_args.first().is_none_or(|arg| arg != "--server")
        || server_args.last().is_none_or(|arg| arg != path.as_os_str())
    {
        return Err(Error::input("invalid rsync server request"));
    }
    let directory = open_transfer_stage(&path, (device, inode))?;
    let mut command = Command::new("rsync");
    command
        .args(&server_args[..server_args.len() - 1])
        .arg(name);
    bind_directory(&mut command, directory.as_file());
    let status = command.status().map_err(Error::input_display)?;
    if !status.success() {
        return Err(Error::input(format!("rsync server failed with {status}")));
    }
    Ok(())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hex_decode(value: &str) -> Result<Vec<u8>> {
    if !value.len().is_multiple_of(2)
        || value.len() > 32768
        || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(Error::input("invalid encoded transfer stage"));
    }
    (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .map_err(|_| Error::input("invalid encoded transfer stage"))
        })
        .collect()
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
    fn invalid_peer_error_codes_cannot_turn_failure_into_success() {
        for code in [0, 1, 5, 255] {
            let error = checked_response(Response::Error {
                code,
                message: "peer supplied error".into(),
            })
            .unwrap_err();
            assert_eq!(error.code, 3);
            assert!(error.message.contains("invalid remote error status"));
        }
        for code in [2, 3, 4] {
            let error = checked_response(Response::Error {
                code,
                message: "valid error".into(),
            })
            .unwrap_err();
            assert_eq!(error.code, code);
            assert_eq!(error.message, "valid error");
        }
    }

    #[test]
    fn stderr_drain_consumes_beyond_the_protocol_output_limit() {
        let bytes = (process::LIMIT + 8192) as u64;
        let mut diagnostics = std::io::repeat(b'x').take(bytes);
        assert_eq!(drain_diagnostics(&mut diagnostics).unwrap(), bytes);
    }

    #[test]
    fn push_rejects_a_replaced_stage_before_rsync() {
        let home = tempfile::tempdir().unwrap();
        let source_home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let mut source = Endpoint::local(AppPaths::new(source_home.path().into()));
        let Response::Snapshot {
            token: source_token,
            ..
        } = source
            .request(Request::Inspect {
                sources: Vec::new(),
            })
            .unwrap()
        else {
            panic!()
        };
        let Response::Begun {
            stage: source_stage,
            device: source_dev,
            inode: source_ino,
            ..
        } = source
            .request(Request::Begin {
                token: source_token,
            })
            .unwrap()
        else {
            panic!()
        };
        let payload = Path::new(&source_stage).join(super::super::state::new_id().unwrap());
        std::fs::write(&payload, "private skill content").unwrap();
        let mut endpoint = Endpoint::local(AppPaths::new(home.path().into()));
        let Response::Snapshot { token, .. } = endpoint
            .request(Request::Inspect {
                sources: Vec::new(),
            })
            .unwrap()
        else {
            panic!()
        };
        let Response::Begun {
            stage,
            device,
            inode,
            ..
        } = endpoint.request(Request::Begin { token }).unwrap()
        else {
            panic!()
        };
        let saved = format!("{stage}-saved");
        std::fs::rename(&stage, &saved).unwrap();
        std::os::unix::fs::symlink(outside.path(), &stage).unwrap();
        let target = format!("{stage}/incoming");
        let error = endpoint
            .push(&payload, &target, (source_dev, source_ino), (device, inode))
            .unwrap_err();
        assert!(error.message.contains("stage changed"), "{error}");
        assert!(std::fs::read_dir(outside.path()).unwrap().next().is_none());
    }

    #[test]
    fn remote_server_rejects_a_swapped_stage_before_starting_rsync() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let stage = home
            .path()
            .join(".skillator/rsync")
            .join(format!("stage-{}", super::super::state::new_id().unwrap()));
        std::fs::create_dir_all(&stage).unwrap();
        let metadata = stage.metadata().unwrap();
        use std::os::unix::fs::MetadataExt;
        let name = super::super::state::new_id().unwrap();
        let path = stage.join(&name);
        let arguments = [
            "--server".into(),
            ".".into(),
            path.as_os_str().to_os_string(),
        ];
        let encoded = hex_encode(stage.as_os_str().as_bytes());
        std::fs::rename(&stage, stage.with_extension("saved")).unwrap();
        std::fs::create_dir(&stage).unwrap();
        let error = serve_transfer(
            &encoded,
            metadata.dev(),
            metadata.ino(),
            name.as_ref(),
            &arguments,
        )
        .unwrap_err();
        assert!(error.message.contains("stage changed"), "{error}");
        std::fs::remove_dir(&stage).unwrap();
        std::os::unix::fs::symlink(outside.path(), &stage).unwrap();
        assert!(
            serve_transfer(
                &encoded,
                metadata.dev(),
                metadata.ino(),
                name.as_ref(),
                &arguments
            )
            .is_err()
        );
        assert!(std::fs::read_dir(outside.path()).unwrap().next().is_none());
    }

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
