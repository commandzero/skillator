use super::{EXPORT_PREFIX, Error, MARKER_CONTENT, MARKER_NAME, REPLICA_RELATIVE, Result, process};
use std::borrow::Cow;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub(super) struct Replica {
    pub path: PathBuf,
    pub exists: bool,
    pub created: bool,
}

pub(super) enum Direction {
    Push,
    Pull,
}

const RSYNC_OPTIONS: &str = "-rltp --checksum --safe-links --delete-delay --omit-dir-times --exclude=/.skillator-rsync-owned --itemize-changes --out-format=%i";
const RSYNC_PROBE_OPTIONS: &str = "--dry-run --list-only -- /dev/null";

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

fn ssh(destination: &str, script: &str) -> Result<Vec<u8>> {
    if !valid_destination(destination) {
        return Err(Error::input("invalid SSH destination"));
    }
    process::capture(
        Command::new("ssh")
            .args(SSH_OPTIONS)
            .arg("--")
            .arg(ssh_destination(destination).as_ref())
            .arg(script),
    )
}

fn ssh_destination(destination: &str) -> Cow<'_, str> {
    // rsync strips IPv6 brackets before invoking ssh; preflight must connect
    // to the same literal host, not try to resolve "[::1]" as a hostname.
    match destination.split_once('[') {
        Some(("", bracketed)) => Cow::Borrowed(bracketed.trim_end_matches(']')),
        Some((prefix, bracketed)) => {
            Cow::Owned(format!("{prefix}{}", bracketed.trim_end_matches(']')))
        }
        None => Cow::Borrowed(destination),
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn path_text(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| Error::input("path cannot be represented on a remote shell"))
}

fn checked_absolute(path: &Path) -> bool {
    let bytes = path.as_os_str().as_encoded_bytes();
    path.is_absolute()
        && !bytes.iter().any(u8::is_ascii_control)
        && (bytes == b"/"
            || bytes[1..]
                .split(|byte| *byte == b'/')
                .all(|part| !part.is_empty() && part != b"." && part != b".."))
}

pub(super) fn check_rsync(ipv6: bool) -> Result<()> {
    process::capture(
        Command::new("rsync")
            .args(RSYNC_OPTIONS.split_ascii_whitespace())
            .args(RSYNC_PROBE_OPTIONS.split_ascii_whitespace())
            .stdout(Stdio::null()),
    )
    .map_err(|error| {
        Error::input(format!(
            "rsync is unavailable or lacks required transfer options; install/update it and add it to PATH: {error}"
        ))
    })?;
    if ipv6 {
        // System openrsync accepts --ipv6 but splits bracketed hosts at the first colon.
        // Require advertised IPv6 support, in addition to the shared option probe.
        let capabilities = process::capture(Command::new("rsync").arg("--version"))
            .map_err(Error::input_display)?;
        if !capabilities
            .split(|byte| *byte == b',' || *byte == b'\n')
            .any(|capability| capability.trim_ascii() == b"IPv6")
        {
            return Err(Error::input(
                "local rsync does not advertise IPv6 support; install GNU rsync for bracketed IPv6 destinations",
            ));
        }
    }
    Ok(())
}

fn replica_script(expected_root: Option<&Path>) -> Result<String> {
    let creating = expected_root.is_some();
    let marker_content = shell_quote(MARKER_CONTENT);
    let expected_root = expected_root
        .map(|path| path_text(path).map(shell_quote))
        .transpose()?;
    Ok(format!(
        "set -eu\n\
         command -v rsync >/dev/null 2>&1 || {{ echo 'rsync is not installed on receiver; install it and add it to the SSH shell PATH' >&2; exit 1; }}\n\
         for utility in {}; do\n\
           command -v \"$utility\" >/dev/null 2>&1 || {{ echo \"$utility is not installed on receiver; install it and add it to the SSH shell PATH\" >&2; exit 1; }}\n\
         done\n\
         rsync {RSYNC_OPTIONS} {RSYNC_PROBE_OPTIONS} >/dev/null || {{ echo 'receiver rsync lacks required transfer options; update rsync' >&2; exit 1; }}\n\
         case ${{HOME:-}} in /*) ;; *) echo 'receiver HOME is not absolute' >&2; exit 1;; esac\n\
         case $HOME in *[[:cntrl:]]*) echo 'receiver HOME contains control characters' >&2; exit 1;; esac\n\
         home=$(CDPATH= cd -P \"$HOME\" && printf '%s/' \"$PWD\") || {{ echo 'receiver HOME is not an existing directory' >&2; exit 1; }}\n\
         case $home in /*) ;; *) echo 'receiver HOME is not absolute' >&2; exit 1;; esac\n\
         case $home in *[[:cntrl:]]*) echo 'receiver HOME contains control characters' >&2; exit 1;; esac\n\
         home=${{home%/}}\n\
         root=${{home%/}}/{REPLICA_RELATIVE}\n\
         if [ {creation} = yes ] && [ \"$root\" != {expected_root} ]; then echo 'receiver HOME or replica root changed since inspection' >&2; exit 1; fi\n\
         part=${{home%/}}\n\
         fresh=no\n\
         for name in .skillator library replica; do\n\
           part=$part/$name\n\
           if [ -L \"$part\" ]; then echo 'replica path contains a symbolic link' >&2; exit 1; fi\n\
           if [ -e \"$part\" ]; then\n\
             if [ ! -d \"$part\" ]; then echo 'replica path is not a directory' >&2; exit 1; fi\n\
           elif [ {} = yes ]; then\n\
             mkdir -- \"$part\" || exit 1\n\
             if [ \"$name\" = replica ]; then fresh=yes; fi\n\
           else\n\
             printf 'absent\\n%s\\n' \"$root\"\n\
             exit 0\n\
           fi\n\
         done\n\
         marker=$root/{MARKER_NAME}\n\
         rollback_root() {{\n\
           if [ \"$1\" = marker ]; then\n\
             rm -- \"$marker\" || echo \"cannot remove partial replica marker at $marker\" >&2\n\
           fi\n\
           rmdir -- \"$root\" || echo \"newly created replica root retained at $root; remove it after inspection before retrying\" >&2\n\
           exit 1\n\
         }}\n\
         if [ \"$fresh\" = yes ]; then\n\
           if [ -e \"$marker\" ] || [ -L \"$marker\" ]; then\n\
             echo 'replica ownership marker appeared during creation; refusing to replace it' >&2\n\
             rollback_root no-marker\n\
           fi\n\
         elif [ -e \"$marker\" ] || [ -L \"$marker\" ]; then\n\
           if [ -L \"$marker\" ] || [ ! -f \"$marker\" ] || ! printf %s {} | cmp - \"$marker\" >/dev/null 2>&1; then\n\
             echo 'replica ownership marker is invalid; move the unmanaged replica aside' >&2; exit 1\n\
           fi\n\
         else\n\
           echo 'replica is unmarked; move the unmanaged replica aside' >&2; exit 1\n\
         fi\n\
         if [ {} = yes ]; then\n\
           if [ \"$fresh\" != yes ]; then echo 'replica root changed since inspection; refusing creation' >&2; exit 1; fi\n\
           if ! (set -C; : > \"$marker\"); then\n\
             echo \"cannot create replica ownership marker at $marker\" >&2\n\
             rollback_root no-marker\n\
           fi\n\
           if ! printf %s {} > \"$marker\"; then\n\
             echo \"cannot write replica ownership marker at $marker\" >&2\n\
             rollback_root marker\n\
           fi\n\
         fi\n\
         linked=$(find \"$root\" \\( -type f -o -type l \\) -links +1 -print) || {{ echo 'cannot inspect replica for hard links' >&2; exit 1; }}\n\
         if [ -n \"$linked\" ]; then echo 'replica contains a multiply linked file; move the unmanaged replica aside' >&2; exit 1; fi\n\
         if [ {} = no ]; then printf 'existing\\n%s\\n' \"$root\"; fi\n",
        if creating {
            "find cmp mkdir rm rmdir"
        } else {
            "find cmp mkdir"
        },
        if creating { "yes" } else { "no" },
        marker_content,
        if creating { "yes" } else { "no" },
        marker_content,
        if creating { "yes" } else { "no" },
        expected_root = expected_root.as_deref().unwrap_or("''"),
        creation = if creating { "yes" } else { "no" },
    ))
}

fn inspected_replica(output: Vec<u8>) -> Result<Replica> {
    let text = String::from_utf8(output).map_err(Error::input_display)?;
    let (status, raw) = text
        .strip_suffix('\n')
        .and_then(|text| text.split_once('\n'))
        .ok_or_else(|| Error::input("receiver returned an invalid replica path"))?;
    let path = PathBuf::from(raw);
    if !checked_absolute(&path) || !path.ends_with(REPLICA_RELATIVE) {
        return Err(Error::input("receiver returned an invalid replica path"));
    }
    match status {
        "existing" | "absent" => Ok(Replica {
            path,
            exists: status == "existing",
            created: false,
        }),
        _ => Err(Error::input("receiver returned an invalid replica state")),
    }
}

pub(super) fn remote_replica(destination: &str, create: bool) -> Result<Replica> {
    // Validate the complete read-only inspection response before creating anything.
    let mut replica = inspected_replica(ssh(destination, &replica_script(None)?)?)?;
    if create && !replica.exists {
        // The creation script checks its physical root against the inspected
        // path before any writes. It emits no response requiring validation.
        ssh(destination, &replica_script(Some(&replica.path))?)?;
        replica.exists = true;
        replica.created = true;
    }
    Ok(replica)
}

fn reject_linked_files(directory: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;

    for entry in fs::read_dir(directory).map_err(Error::input_display)? {
        let entry = entry.map_err(Error::input_display)?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(Error::input_display)?;
        if (metadata.is_file() || metadata.is_symlink()) && metadata.nlink() > 1 {
            return Err(Error::input(format!(
                "replica contains a multiply linked file at {}; move the unmanaged replica aside",
                path.display()
            )));
        }
        if metadata.is_dir() {
            reject_linked_files(&path)?;
        }
    }
    Ok(())
}

fn rollback_local_marker(root: &Path, marker: &Path, owned_marker: bool, failure: Error) -> Error {
    use std::fmt::Write as _;

    let mut message = failure.to_string();
    if owned_marker && let Err(error) = fs::remove_file(marker) {
        write!(
            message,
            "; cannot remove partial replica marker at {}: {error}",
            marker.display()
        )
        .expect("writing to a String cannot fail");
    }
    if let Err(error) = fs::remove_dir(root) {
        write!(
            message,
            "; newly created replica root retained at {}: {error}",
            root.display()
        )
        .expect("writing to a String cannot fail");
    }
    Error::input(message)
}

pub(super) fn local_replica(home: &Path, create: bool) -> Result<Replica> {
    let invalid_home = || Error::input("local HOME must be an existing absolute directory");
    if !home.is_absolute()
        || home.to_str().is_none()
        || home
            .as_os_str()
            .as_encoded_bytes()
            .iter()
            .any(u8::is_ascii_control)
    {
        return Err(invalid_home());
    }
    let home = fs::canonicalize(home).map_err(|_| invalid_home())?;
    if !checked_absolute(&home) || home.to_str().is_none() || !home.is_dir() {
        return Err(invalid_home());
    }
    let root = home.join(REPLICA_RELATIVE);
    let mut part = home;
    let mut created_root = false;
    for name in [".skillator", "library", "replica"] {
        part.push(name);
        match fs::symlink_metadata(&part) {
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
                return Err(Error::input(format!(
                    "replica path {} is a link or is not a directory",
                    part.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if !create {
                    return Ok(Replica {
                        path: root,
                        exists: false,
                        created: false,
                    });
                }
                fs::create_dir(&part).map_err(Error::input_display)?;
                if name == "replica" {
                    created_root = true;
                }
            }
            Err(error) => return Err(Error::input_display(error)),
        }
    }
    let marker = root.join(MARKER_NAME);
    match fs::symlink_metadata(&marker) {
        Ok(meta) => {
            let valid = if !meta.is_file() || meta.file_type().is_symlink() {
                false
            } else {
                fs::read(&marker).map_err(|error| {
                    let failure = Error::input_display(error);
                    if created_root {
                        rollback_local_marker(&root, &marker, false, failure)
                    } else {
                        failure
                    }
                })? == MARKER_CONTENT.as_bytes()
            };
            if !valid {
                let failure = Error::input(format!(
                    "replica ownership marker is invalid at {}; move the unmanaged replica aside",
                    root.display()
                ));
                return Err(if created_root {
                    rollback_local_marker(&root, &marker, false, failure)
                } else {
                    failure
                });
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && created_root => {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&marker)
                .map_err(|error| {
                    rollback_local_marker(&root, &marker, false, Error::input_display(error))
                })?;
            if let Err(error) = file.write_all(MARKER_CONTENT.as_bytes()) {
                drop(file);
                return Err(rollback_local_marker(
                    &root,
                    &marker,
                    true,
                    Error::input_display(error),
                ));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::input(format!(
                "replica is unmarked at {}; move the unmanaged replica aside",
                root.display()
            )));
        }
        Err(error) if created_root => {
            return Err(rollback_local_marker(
                &root,
                &marker,
                false,
                Error::input_display(error),
            ));
        }
        Err(error) => return Err(Error::input_display(error)),
    }
    reject_linked_files(&root)?;
    Ok(Replica {
        path: root,
        exists: true,
        created: created_root,
    })
}

fn valid_export_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    checked_absolute(path)
        && name.starts_with(EXPORT_PREFIX)
        && name[EXPORT_PREFIX.len()..]
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        && name.len() > EXPORT_PREFIX.len()
}

fn validated_export_script(path: &Path) -> Result<String> {
    let parent = path.parent().expect("absolute export has a parent");
    Ok(format!(
        "set -eu\n\
         tmp=${{TMPDIR:-/tmp}}\n\
         tmp=$(cd -P \"$tmp\" && pwd -P) || {{ echo 'leader temporary directory is unavailable' >&2; exit 1; }}\n\
         parent=$(cd -P {} && pwd -P) || {{ echo 'leader export parent is unavailable' >&2; exit 1; }}\n\
         [ \"$tmp\" = \"$parent\" ] || {{ echo 'leader export is outside its temporary directory' >&2; exit 1; }}\n\
         dir={}\n\
         [ ! -L \"$dir\" ] && [ -d \"$dir\" ] || {{ echo 'leader export is not a physical directory' >&2; exit 1; }}\n\
         marker=$dir/{MARKER_NAME}\n\
         [ ! -L \"$marker\" ] && [ -f \"$marker\" ] && printf %s {} | cmp - \"$marker\" >/dev/null 2>&1 || {{ echo 'leader export ownership marker is invalid' >&2; exit 1; }}\n",
        shell_quote(path_text(parent)?),
        shell_quote(path_text(path)?),
        shell_quote(MARKER_CONTENT),
    ))
}

pub(super) fn remote_export(destination: &str) -> Result<PathBuf> {
    let script = format!(
        "set -eu\n\
         command -v rsync >/dev/null 2>&1 || {{ echo 'rsync is not installed on leader; install it and add it to the SSH shell PATH' >&2; exit 1; }}\n\
         rsync {RSYNC_OPTIONS} {RSYNC_PROBE_OPTIONS} >/dev/null || {{ echo 'leader rsync lacks required transfer options; update rsync' >&2; exit 1; }}\n\
         command -v skillator >/dev/null 2>&1 || {{ echo 'Skillator is not installed on leader; install it and add it to the SSH shell PATH' >&2; exit 1; }}\n\
         command -v cmp >/dev/null 2>&1 || {{ echo 'cmp is not installed on leader; install it and add it to the SSH shell PATH' >&2; exit 1; }}\n\
         command -v rm >/dev/null 2>&1 || {{ echo 'rm is not installed on leader; install it and add it to the SSH shell PATH' >&2; exit 1; }}\n\
         command -v find >/dev/null 2>&1 || {{ echo 'find is not installed on leader; install it and add it to the SSH shell PATH' >&2; exit 1; }}\n\
         command -v chmod >/dev/null 2>&1 || {{ echo 'chmod is not installed on leader; install it and add it to the SSH shell PATH' >&2; exit 1; }}\n\
         skillator library rsync --prepare-export\n",
    );
    let output = ssh(destination, &script)?;
    let path = String::from_utf8(output).map_err(Error::input_display)?;
    let Some(raw) = path.strip_suffix('\n') else {
        return Err(Error::input(
            "leader returned an invalid export path; refusing cleanup",
        ));
    };
    let path = PathBuf::from(raw);
    if !valid_export_path(&path) {
        return Err(Error::input(
            "leader returned an invalid export path; refusing cleanup",
        ));
    }
    // Validate the same physical parent, directory and marker used for cleanup
    // before handing the leader's path to rsync. A failed validation cannot
    // safely authorize cleanup, so expose the retained path for manual recovery.
    validated_export_script(&path)
        .and_then(|script| ssh(destination, &script).map(|_| ()))
        .map_err(|error| {
            Error::input(format!(
                "leader export may remain at {} after physical validation failed: {error}; refusing cleanup",
                path.display()
            ))
        })?;
    Ok(path)
}

pub(super) fn cleanup_export(destination: &str, path: &Path) -> Result<()> {
    if !valid_export_path(path) {
        return Err(Error::input("invalid leader export path; refusing cleanup"));
    }
    let mut script = validated_export_script(path)?;
    script.push_str(
        "find \"$dir\" -type d ! -perm -0700 -exec chmod u+rwx {} \\;\nrm -r -- \"$dir\"\n",
    );
    ssh(destination, &script).map(|_| ())
}

pub(super) fn rsync(
    destination: &str,
    export: &Path,
    replica: &Path,
    direction: Direction,
    check: bool,
) -> Result<bool> {
    if !valid_destination(destination) {
        return Err(Error::input("invalid SSH destination"));
    }
    let mut command = Command::new("rsync");
    // Keep old and new rsync implementations' remote path parsing consistent.
    command.env("RSYNC_OLD_ARGS", "1");
    command.args(RSYNC_OPTIONS.split_ascii_whitespace());
    if check {
        command.arg("--dry-run");
    }
    command
        .arg("-e")
        .arg(format!("ssh {}", SSH_OPTIONS.join(" ")));
    let export = format!("{}/", path_text(export)?.trim_end_matches('/'));
    let replica = format!("{}/", path_text(replica)?.trim_end_matches('/'));
    let remote_arg = |path: &str| format!("{destination}:{}", shell_quote(path));
    match direction {
        Direction::Push => {
            command.arg("--").arg(export).arg(remote_arg(&replica));
        }
        Direction::Pull => {
            command.arg("--").arg(remote_arg(&export)).arg(replica);
        }
    }
    Ok(!process::capture(&mut command)?.is_empty())
}

pub(super) fn valid_destination(destination: &str) -> bool {
    let (user, host) = destination
        .split_once('@')
        .map_or((None, destination), |(user, host)| (Some(user), host));
    if user.is_some_and(|user| !ordinary_name(user)) {
        return false;
    }
    if let Some(address) = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
    {
        address.parse::<std::net::Ipv6Addr>().is_ok()
    } else {
        ordinary_name(host)
    }
}

fn ordinary_name(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn rsync_and_ssh_destination_forms_keep_the_same_host() {
        for (rsync_host, ssh_host) in [
            ("alias_1", "alias_1"),
            ("user@host.example", "user@host.example"),
            ("[::1]", "::1"),
            ("user@[2001:db8::1]", "user@2001:db8::1"),
        ] {
            assert!(valid_destination(rsync_host));
            assert_eq!(ssh_destination(rsync_host), ssh_host);
        }
        for destination in [
            "local:prod",
            "user@local:prod",
            "host::module",
            "::1",
            "host:22",
            "host:/path",
            "[::1]:22",
            "[invalid]",
            "user@[::1]suffix",
            "user@host[::1]",
            "user@-host",
            "-user@host",
            "@host",
            "user@",
            "user@@host",
        ] {
            assert!(!valid_destination(destination), "accepted {destination}");
        }
    }

    #[test]
    fn receiver_inspection_requires_one_utf8_fixed_root_and_valid_state() {
        let root = b"/home/a/.skillator/library/replica";
        for response in [
            b"absent\n/home/a/.skillator/library/replica\nextra\n".as_slice(),
            b"created\n/home/a/.skillator/library/replica\n",
            b"absent\n/home/a/.skillator/library/replica",
            b"absent\n/home/a/../a/.skillator/library/replica\n",
            b"absent\n/home/a/other\n",
            b"absent\nrelative/.skillator/library/replica\n",
            b"absent\n/home/a/.skillator/library/replica\n\xff",
        ] {
            assert!(inspected_replica(response.to_vec()).is_err());
        }
        let mut response = b"absent\n".to_vec();
        response.extend_from_slice(root);
        response.push(b'\n');
        let replica = inspected_replica(response).unwrap();
        assert!(!replica.exists);
        assert!(!replica.created);
        assert_eq!(
            replica.path,
            Path::new("/home/a/.skillator/library/replica")
        );
    }

    #[test]
    fn receiver_creation_requires_the_previously_inspected_physical_root() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let changed_home = temp.path().join("changed");
        fs::create_dir(&home).unwrap();
        fs::create_dir(&changed_home).unwrap();
        let run = |home: &Path, expected: Option<&Path>| {
            process::capture(
                Command::new("sh")
                    .arg("-c")
                    .arg(replica_script(expected).unwrap())
                    .env("HOME", home),
            )
        };

        let inspection = inspected_replica(run(&home, None).unwrap()).unwrap();
        assert!(!inspection.exists);
        assert!(!home.join(".skillator").exists());
        assert!(run(&changed_home, Some(&inspection.path)).is_err());
        assert!(!changed_home.join(".skillator").exists());
        assert!(run(&home, Some(&changed_home.join(REPLICA_RELATIVE))).is_err());
        assert!(!home.join(".skillator").exists());

        run(&home, Some(&inspection.path)).unwrap();
        assert_eq!(
            fs::read(inspection.path.join(MARKER_NAME)).unwrap(),
            MARKER_CONTENT.as_bytes()
        );
        assert!(inspected_replica(run(&home, None).unwrap()).unwrap().exists);
        assert!(run(&home, Some(&inspection.path)).is_err());
    }

    #[test]
    fn local_marker_rollback_removes_only_its_own_partial_marker_and_empty_root() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("replica");
        let marker = root.join(MARKER_NAME);
        let failure = || Error::input("marker write failed");
        fs::create_dir(&root).unwrap();
        fs::write(&marker, "partial").unwrap();
        let error = rollback_local_marker(&root, &marker, true, failure());
        assert!(error.to_string().contains("marker write failed"));
        assert!(!root.exists());

        fs::create_dir(&root).unwrap();
        fs::write(&marker, "foreign marker").unwrap();
        let error = rollback_local_marker(&root, &marker, false, failure());
        assert!(error.to_string().contains("marker write failed"));
        assert!(error.to_string().contains("root retained"));
        assert_eq!(fs::read(&marker).unwrap(), b"foreign marker");
        fs::remove_file(&marker).unwrap();
        fs::write(root.join("unrelated"), "keep").unwrap();
        let error = rollback_local_marker(&root, &marker, false, failure());
        assert!(error.to_string().contains("root retained"));
        assert_eq!(fs::read(root.join("unrelated")).unwrap(), b"keep");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn local_replica_rejects_non_utf8_original_and_physical_home_before_writes() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let parent = tempfile::tempdir().unwrap();
        let physical = parent.path().join(OsStr::from_bytes(b"home-\xff"));
        fs::create_dir(&physical).unwrap();
        let utf8_alias = parent.path().join("home-alias");
        symlink(&physical, &utf8_alias).unwrap();
        for (home, original_is_utf8) in [(&physical, false), (&utf8_alias, true)] {
            assert_eq!(home.to_str().is_some(), original_is_utf8);
            assert!(local_replica(home, true).is_err());
            assert!(!physical.join(".skillator").exists());
        }
    }

    #[test]
    fn local_replica_checks_ownership_before_writes() {
        let home = tempfile::tempdir().unwrap();
        let preview = local_replica(home.path(), false).unwrap();
        assert!(!preview.exists);
        assert!(!preview.path.exists());
        fs::create_dir_all(&preview.path).unwrap();
        assert!(local_replica(home.path(), true).is_err());
        assert!(!preview.path.join(MARKER_NAME).exists());
        fs::write(preview.path.join(MARKER_NAME), MARKER_CONTENT).unwrap();
        assert!(local_replica(home.path(), false).unwrap().exists);
        fs::write(preview.path.join(MARKER_NAME), "wrong").unwrap();
        assert!(local_replica(home.path(), true).is_err());
    }

    #[test]
    fn local_replica_creation_marks_only_its_fixed_root() {
        let home = tempfile::tempdir().unwrap();
        let sibling = home.path().join("untouched");
        fs::write(&sibling, "keep").unwrap();
        let replica = local_replica(home.path(), true).unwrap();
        assert!(replica.exists);
        assert_eq!(
            fs::read(replica.path.join(MARKER_NAME)).unwrap(),
            MARKER_CONTENT.as_bytes()
        );
        assert_eq!(fs::read(&sibling).unwrap(), b"keep");
    }

    #[test]
    fn local_replica_rejects_linked_ancestors_and_markers() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), home.path().join(".skillator")).unwrap();
        assert!(local_replica(home.path(), false).is_err());
        fs::remove_file(home.path().join(".skillator")).unwrap();
        let replica = local_replica(home.path(), true).unwrap();
        fs::remove_file(replica.path.join(MARKER_NAME)).unwrap();
        fs::write(outside.path().join("marker"), MARKER_CONTENT).unwrap();
        symlink(
            outside.path().join("marker"),
            replica.path.join(MARKER_NAME),
        )
        .unwrap();
        assert!(local_replica(home.path(), true).is_err());
    }

    #[test]
    fn local_replica_rejects_linked_root() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir_all(home.path().join(".skillator/library")).unwrap();
        symlink(outside.path(), home.path().join(REPLICA_RELATIVE)).unwrap();
        assert!(local_replica(home.path(), true).is_err());
        assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
    }

    #[test]
    fn export_paths_require_private_generated_names() {
        for path in [
            "/tmp/other",
            "/tmp/skillator-library-export-",
            "/tmp/skillator-library-export-a/../victim",
            "/tmp/./skillator-library-export-a",
            "/tmp//skillator-library-export-a",
            "/tmp/skillator-library-export-a/",
            "/tmp/skillator-library-export-a\n",
            "relative/skillator-library-export-a",
        ] {
            assert!(!valid_export_path(Path::new(path)), "{path}");
        }
        assert!(valid_export_path(Path::new(
            "/tmp/skillator-library-export-ab12"
        )));
    }

    #[test]
    fn export_guard_requires_physical_temp_parent_root_and_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let export = tmp.path().join(format!("{EXPORT_PREFIX}valid"));
        let unrelated = outside.path().join(format!("{EXPORT_PREFIX}valid"));
        fs::create_dir(&export).unwrap();
        fs::create_dir(&unrelated).unwrap();
        fs::write(export.join(MARKER_NAME), MARKER_CONTENT).unwrap();
        fs::write(unrelated.join(MARKER_NAME), MARKER_CONTENT).unwrap();

        let validate = |path: &Path| {
            process::capture(
                Command::new("sh")
                    .arg("-c")
                    .arg(validated_export_script(path).unwrap())
                    .env("TMPDIR", tmp.path()),
            )
        };
        assert!(validate(&export).is_ok());
        assert!(validate(&unrelated).is_err());

        fs::write(export.join(MARKER_NAME), "wrong").unwrap();
        assert!(validate(&export).is_err());
        fs::remove_file(export.join(MARKER_NAME)).unwrap();
        symlink(unrelated.join(MARKER_NAME), export.join(MARKER_NAME)).unwrap();
        assert!(validate(&export).is_err());
        fs::remove_file(export.join(MARKER_NAME)).unwrap();
        fs::remove_dir(&export).unwrap();
        symlink(&unrelated, &export).unwrap();
        assert!(validate(&export).is_err());
    }

    #[test]
    fn shell_quoting_keeps_metacharacters_literal() {
        let path = "/home/a space/quote's $(literal) `also literal`";
        assert_eq!(
            process::capture(
                Command::new("sh")
                    .arg("-c")
                    .arg(format!("printf %s {}", shell_quote(path)))
            )
            .unwrap(),
            path.as_bytes()
        );
    }
}
