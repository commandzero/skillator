use super::{
    Error, Result, process,
    session::{Request, Response, Session},
    stage::Stage,
    state,
};
use crate::app::AppPaths;
use crate::fs_safety::bind_directory;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::ffi::OsStrExt;
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
                        "invalid remote response or incompatible Skillator; protocol 5 is required",
                    )
                })?
            }
        };
        checked_response(response)
    }

    pub fn pull_batch(&mut self, names: &[String], source: &Stage, local: &Stage) -> Result<()> {
        self.transfer_batch(names, source, local, false)
    }

    pub fn push_batch(&mut self, names: &[String], local: &Stage, target: &Stage) -> Result<()> {
        self.transfer_batch(names, target, local, true)
    }

    fn transfer_batch(
        &mut self,
        names: &[String],
        endpoint_stage: &Stage,
        local_stage: &Stage,
        push: bool,
    ) -> Result<()> {
        if names.is_empty() {
            return Ok(());
        }
        let mut seen = std::collections::BTreeSet::new();
        if names.len() > 64
            || names
                .iter()
                .any(|name| !state::valid_id(name) || !seen.insert(name))
        {
            return Err(Error::input("invalid transfer manifest"));
        }
        self.request_stage_validation()?;
        if matches!(self, Endpoint::Local(_)) {
            endpoint_stage.open()?;
        }
        local_stage.open()?;
        let transfer_result = transfer(self, names, endpoint_stage, local_stage, push);
        let local_validation = local_stage.open();
        let endpoint_validation = if matches!(self, Endpoint::Local(_)) {
            endpoint_stage.open().map(|_| ())
        } else {
            Ok(())
        };
        let remote_validation = self.request_stage_validation();
        transfer_result?;
        local_validation?;
        endpoint_validation?;
        remote_validation
    }

    fn request_stage_validation(&mut self) -> Result<()> {
        if matches!(self.request(Request::ValidateStage)?, Response::Ok) {
            Ok(())
        } else {
            Err(Error::input("unexpected stage validation response"))
        }
    }
}

fn checked_response(response: Response) -> Result<Response> {
    match response {
        Response::Error {
            code: code @ 2..=4,
            message,
        } => Err(Error { code, message }),
        Response::Error { .. } => Err(Error::input(
            "invalid remote error status or incompatible Skillator; protocol 5 is required",
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
                    "Skillator version {version:?} is incompatible; synchronization protocol 5 is required on remote host"
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
    names: &[String],
    endpoint_stage: &Stage,
    local_stage: &Stage,
    push: bool,
) -> Result<()> {
    #[cfg(test)]
    if let Endpoint::Fault { inner, fault } = endpoint {
        if matches!(fault, Fault::Transfer) {
            return Err(Error::input("injected transfer failure"));
        }
        return transfer(inner, names, endpoint_stage, local_stage, push);
    }
    let opened_local_stage = local_stage.open()?;
    if matches!(endpoint, Endpoint::Local(_)) {
        let opened_endpoint_stage = endpoint_stage.open()?;
        let (source, target) = if push {
            (&opened_local_stage, &opened_endpoint_stage)
        } else {
            (&opened_endpoint_stage, &opened_local_stage)
        };
        for name in names {
            state::copy_transfer_entry_at(source, name.as_ref(), target, name.as_ref())?;
        }
        return Ok(());
    }
    let mut command = Command::new("rsync");
    // Older and newer rsync both receive the quoted stage as one literal path.
    command.env("RSYNC_OLD_ARGS", "1");
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
            let manifest = names.join("\n");
            command.arg("--rsync-path").arg(format!(
                "skillator __rsync-server --stage-hex {} --device {} --inode {} --names-hex {} --",
                state::hex(endpoint_stage.path().as_os_str().as_bytes()),
                endpoint_stage.identity().0,
                endpoint_stage.identity().1,
                state::hex(manifest.as_bytes()),
            ));
            format!(
                "{}:{}",
                remote.destination,
                shell_quote(&format!("{}/", endpoint_stage.path().display()))
            )
        }
    };
    bind_directory(&mut command, opened_local_stage.as_file());
    if push {
        command.arg("--");
        for name in names {
            command.arg(name);
        }
        command.arg(endpoint_arg);
    } else {
        // Rsync accepts one remote source argument. An explicit flat file list is
        // provided by --files-from without consuming its protocol stdin.
        let manifest_name = state::new_id()?;
        let mut manifest_file = opened_local_stage
            .create_file(manifest_name.as_ref())
            .map_err(Error::input_display)?;
        for name in names {
            writeln!(manifest_file, "{name}").map_err(Error::input_display)?;
        }
        manifest_file.sync_all().map_err(Error::input_display)?;
        command.arg(format!("--files-from={manifest_name}"));
        command.arg("--").arg(endpoint_arg).arg("./");
        let result = process::capture(&mut command);
        opened_local_stage
            .remove(manifest_name.as_ref())
            .map_err(Error::input_display)?;
        result?;
        return Ok(());
    }
    process::capture(&mut command)?;
    Ok(())
}

pub(crate) fn serve_transfer(
    home: &Path,
    stage_hex: &str,
    device: u64,
    inode: u64,
    names_hex: &str,
    server_args: &[std::ffi::OsString],
) -> Result<()> {
    let stage_text = String::from_utf8(hex_decode(stage_hex)?).map_err(Error::input_display)?;
    let home = home.canonicalize().map_err(Error::input_display)?;
    let stage = Stage::new(&home, Path::new(&stage_text), (device, inode))?;
    let names = String::from_utf8(hex_decode(names_hex)?).map_err(Error::input_display)?;
    let names: Vec<&str> = names.split('\n').collect();
    let mut unique = std::collections::BTreeSet::new();
    if names.is_empty()
        || names.len() > 64
        || names
            .iter()
            .any(|name| !state::valid_id(name) || !unique.insert(*name))
    {
        return Err(Error::input("invalid transfer manifest"));
    }
    let stage_path = format!("{}/", stage.path().display());
    let sender = server_args.get(1).is_some_and(|arg| arg == "--sender");
    if server_args.first().is_none_or(|arg| arg != "--server")
        || server_args
            .last()
            .is_none_or(|arg| arg.as_os_str() != std::ffi::OsStr::new(&stage_path))
        || server_args.len() < 4
        || server_args[server_args.len() - 2] != "."
        || server_args[1..server_args.len() - 2].iter().any(|arg| {
            let arg = arg.to_string_lossy();
            // GNU rsync appends protocol capabilities after `e.`. Their `L`
            // does not enable --copy-links; an `L` before that suffix does.
            let options = arg
                .split_once("e.")
                .map_or(arg.as_ref(), |(options, _)| options);
            // --files-from implicitly enables these sender options. The file
            // list is still confined to the isolated, explicit payload view.
            if sender
                && matches!(
                    arg.as_ref(),
                    "--files-from=-" | "--from0" | "--relative" | "--dirs"
                )
            {
                return false;
            }
            (arg != "--sender" && !arg.starts_with('-'))
                || arg.contains('/')
                || arg.contains("delete")
                || arg.contains("files-from")
                || arg.contains("relative")
                || arg.contains("recursive")
                || arg.contains("copy-links")
                || arg.contains("copy-dirlinks")
                || (arg.starts_with('-') && !arg.starts_with("--") && options.contains('L'))
        })
    {
        return Err(Error::input("invalid rsync server request"));
    }
    let directory = stage.open()?;
    // Rsync's sender trusts its client's file list. Restrict its filesystem view
    // to this manifest, so a forged file list cannot read other stage exports.
    // Likewise, a receiver cannot write any sibling in the real stage.
    let view_name = format!("batch-{}", state::new_id()?);
    directory
        .create_dir(view_name.as_ref())
        .map_err(Error::input_display)?;
    let result = (|| {
        let view = directory
            .open_dir(view_name.as_ref())
            .map_err(Error::input_display)?;
        if sender {
            for name in &names {
                state::copy_transfer_entry_at(&directory, name.as_ref(), &view, name.as_ref())?;
            }
        }
        let mut command = Command::new("rsync");
        command
            .args(&server_args[..server_args.len() - 1])
            .arg("./");
        bind_directory(&mut command, view.as_file());
        let status = command.status().map_err(Error::input_display)?;
        if !status.success() {
            return Err(Error::input(format!("rsync server failed with {status}")));
        }
        stage.open()?;
        if !sender {
            let found = view.entries().map_err(Error::input_display)?;
            if found.len() != names.len()
                || found
                    .iter()
                    .any(|name| !unique.contains(name.to_string_lossy().as_ref()))
            {
                return Err(Error::input(
                    "rsync transferred unlisted or missing payloads",
                ));
            }
            for name in &names {
                state::copy_transfer_entry_at(&view, name.as_ref(), &directory, name.as_ref())?;
            }
        }
        Ok(())
    })();
    let cleanup = directory
        .remove_tree(view_name.as_ref())
        .map_err(Error::input_display);
    result?;
    cleanup
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
        let name = super::super::state::new_id().unwrap();
        let payload = Path::new(&source_stage).join(&name);
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
        let source_stage = Stage::new(
            &source_home.path().canonicalize().unwrap(),
            Path::new(&source_stage),
            (source_dev, source_ino),
        )
        .unwrap();
        let target_stage = Stage::new(
            &home.path().canonicalize().unwrap(),
            Path::new(&stage),
            (device, inode),
        )
        .unwrap();
        let saved = format!("{stage}-saved");
        std::fs::rename(&stage, &saved).unwrap();
        std::os::unix::fs::symlink(outside.path(), &stage).unwrap();
        let error = endpoint
            .push_batch(&[name], &source_stage, &target_stage)
            .unwrap_err();
        assert!(error.message.contains("stage changed"), "{error}");
        assert!(std::fs::read_dir(outside.path()).unwrap().next().is_none());
    }

    #[test]
    fn remote_server_rejects_link_dereferencing_and_external_manifests() {
        let home = tempfile::tempdir().unwrap();
        let stage = home
            .path()
            .canonicalize()
            .unwrap()
            .join(".skillator/rsync")
            .join(format!("stage-{}", state::new_id().unwrap()));
        let encoded = state::hex(stage.as_os_str().as_bytes());
        let names = state::hex(state::new_id().unwrap().as_bytes());
        for option in [
            "-L",
            "-ltpcLe.LsfxCIvu",
            "--copy-links",
            "--copy-dirlinks",
            "--files-from=/etc/passwd",
        ] {
            let arguments = [
                "--server".into(),
                "--sender".into(),
                option.into(),
                ".".into(),
                format!("{}/", stage.display()).into(),
            ];
            let error =
                serve_transfer(home.path(), &encoded, 0, 0, &names, &arguments).unwrap_err();
            assert_eq!(error.message, "invalid rsync server request", "{option}");
        }
    }

    #[test]
    fn remote_server_rejects_a_swapped_stage_before_starting_rsync() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let stage = home
            .path()
            .canonicalize()
            .unwrap()
            .join(".skillator/rsync")
            .join(format!("stage-{}", super::super::state::new_id().unwrap()));
        std::fs::create_dir_all(&stage).unwrap();
        let metadata = stage.metadata().unwrap();
        use std::os::unix::fs::MetadataExt;
        let name = super::super::state::new_id().unwrap();
        let arguments = [
            "--server".into(),
            "-log".into(),
            ".".into(),
            format!("{}/", stage.display()).into(),
        ];
        let encoded = state::hex(stage.as_os_str().as_bytes());
        let names = state::hex(name.as_bytes());
        std::fs::rename(&stage, stage.with_extension("saved")).unwrap();
        std::fs::create_dir(&stage).unwrap();
        let error = serve_transfer(
            home.path(),
            &encoded,
            metadata.dev(),
            metadata.ino(),
            &names,
            &arguments,
        )
        .unwrap_err();
        assert!(error.message.contains("stage changed"), "{error}");
        std::fs::remove_dir(&stage).unwrap();
        std::os::unix::fs::symlink(outside.path(), &stage).unwrap();
        assert!(
            serve_transfer(
                home.path(),
                &encoded,
                metadata.dev(),
                metadata.ino(),
                &names,
                &arguments
            )
            .is_err()
        );
        assert!(std::fs::read_dir(outside.path()).unwrap().next().is_none());
    }

    #[test]
    fn remote_server_rejects_a_stage_beneath_another_home() {
        use std::os::unix::fs::MetadataExt;
        let home = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let stage = other
            .path()
            .join(".skillator/rsync")
            .join(format!("stage-{}", super::super::state::new_id().unwrap()));
        std::fs::create_dir_all(&stage).unwrap();
        let identity = stage.metadata().unwrap();
        let name = super::super::state::new_id().unwrap();
        let args = [
            "--server".into(),
            "-log".into(),
            ".".into(),
            format!("{}/", stage.display()).into(),
        ];
        let error = serve_transfer(
            home.path(),
            &state::hex(stage.as_os_str().as_bytes()),
            identity.dev(),
            identity.ino(),
            &state::hex(name.as_bytes()),
            &args,
        )
        .unwrap_err();
        assert!(error.message.contains("invalid transfer stage"), "{error}");
        assert!(std::fs::read_dir(&stage).unwrap().next().is_none());
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
