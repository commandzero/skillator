use super::{EXPORT_PREFIX, Error, MARKER_CONTENT, MARKER_NAME, REPLICA_RELATIVE, Result, process};
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
            .arg(destination)
            .arg(script),
    )
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

pub(super) fn check_rsync() -> Result<()> {
    process::capture(
        Command::new("rsync")
            .args(RSYNC_OPTIONS.split_ascii_whitespace())
            .args(RSYNC_PROBE_OPTIONS.split_ascii_whitespace())
            .stdout(Stdio::null()),
    )
        .map(|_| ())
        .map_err(|error| {
            Error::input(format!(
                "rsync is unavailable or lacks required transfer options; install/update it and add it to PATH: {error}"
            ))
        })
}

pub(super) fn remote_replica(destination: &str, create: bool) -> Result<Replica> {
    // A missing destination is not created during preview; rsync dry-run is only
    // necessary when there is an existing, marked destination to compare.
    let marker_content = shell_quote(MARKER_CONTENT);
    let script = format!(
        "set -eu\n\
         command -v rsync >/dev/null 2>&1 || {{ echo 'rsync is not installed on receiver; install it and add it to the SSH shell PATH' >&2; exit 1; }}\n\
         command -v find >/dev/null 2>&1 || {{ echo 'find is not installed on receiver; install it and add it to the SSH shell PATH' >&2; exit 1; }}\n\
         rsync {RSYNC_OPTIONS} {RSYNC_PROBE_OPTIONS} >/dev/null || {{ echo 'receiver rsync lacks required transfer options; update rsync' >&2; exit 1; }}\n\
         case ${{HOME:-}} in /*) ;; *) echo 'receiver HOME is not absolute' >&2; exit 1;; esac\n\
         case $HOME in *[[:cntrl:]]*) echo 'receiver HOME contains control characters' >&2; exit 1;; esac\n\
         home=$(CDPATH= cd -P \"$HOME\" && printf '%s/' \"$PWD\") || {{ echo 'receiver HOME is not an existing directory' >&2; exit 1; }}\n\
         case $home in /*) ;; *) echo 'receiver HOME is not absolute' >&2; exit 1;; esac\n\
         case $home in *[[:cntrl:]]*) echo 'receiver HOME contains control characters' >&2; exit 1;; esac\n\
         home=${{home%/}}\n\
         root=${{home%/}}/{REPLICA_RELATIVE}\n\
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
         if [ -e \"$marker\" ] || [ -L \"$marker\" ]; then\n\
           if [ -L \"$marker\" ] || [ ! -f \"$marker\" ] || ! printf %s {marker_content} | cmp - \"$marker\" >/dev/null 2>&1; then\n\
             echo 'replica ownership marker is invalid; move the unmanaged replica aside' >&2; exit 1\n\
           fi\n\
         elif [ \"$fresh\" = yes ]; then\n\
           printf %s {marker_content} > \"$marker\"\n\
         else\n\
           echo 'replica is unmarked; move the unmanaged replica aside' >&2; exit 1\n\
         fi\n\
         linked=$(find \"$root\" -type f -links +1 -print) || {{ echo 'cannot inspect replica for hard links' >&2; exit 1; }}\n\
         if [ -n \"$linked\" ]; then echo 'replica contains a multiply linked file; move the unmanaged replica aside' >&2; exit 1; fi\n\
         if [ \"$fresh\" = yes ]; then printf 'created\\n%s\\n' \"$root\"; else printf 'existing\\n%s\\n' \"$root\"; fi\n",
        if create { "yes" } else { "no" },
    );
    // The receiver only creates the marker when it created the replica root.
    let output = ssh(destination, &script)?;
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
        "existing" | "created" => Ok(Replica {
            path,
            exists: true,
            created: status == "created",
        }),
        "absent" if !create => Ok(Replica {
            path,
            exists: false,
            created: false,
        }),
        _ => Err(Error::input("receiver returned an invalid replica state")),
    }
}

fn reject_linked_files(directory: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;

    for entry in fs::read_dir(directory).map_err(Error::input_display)? {
        let entry = entry.map_err(Error::input_display)?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(Error::input_display)?;
        if metadata.is_file() && metadata.nlink() > 1 {
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

pub(super) fn local_replica(home: &Path, create: bool) -> Result<Replica> {
    if !checked_absolute(home) || !home.is_dir() {
        return Err(Error::input(
            "local HOME must be an existing absolute directory",
        ));
    }
    let root = home.join(REPLICA_RELATIVE);
    let mut part = home.to_path_buf();
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
            if !meta.is_file()
                || meta.file_type().is_symlink()
                || fs::read(&marker).map_err(Error::input_display)? != MARKER_CONTENT.as_bytes()
            {
                return Err(Error::input(format!(
                    "replica ownership marker is invalid at {}; move the unmanaged replica aside",
                    root.display()
                )));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && created_root => {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&marker)
                .map_err(Error::input_display)?;
            file.write_all(MARKER_CONTENT.as_bytes())
                .map_err(Error::input_display)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::input(format!(
                "replica is unmarked at {}; move the unmanaged replica aside",
                root.display()
            )));
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
    // before handing the leader's path to rsync.
    ssh(destination, &validated_export_script(&path)?)?;
    Ok(path)
}

pub(super) fn cleanup_export(destination: &str, path: &Path) -> Result<()> {
    if !valid_export_path(path) {
        return Err(Error::input("invalid leader export path; refusing cleanup"));
    }
    let mut script = validated_export_script(path)?;
    script.push_str("rm -r -- \"$dir\"\n");
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

fn valid_destination(destination: &str) -> bool {
    !destination.is_empty()
        && !destination.starts_with('-')
        && destination
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "@._-:[]".contains(c))
        && destination.matches('@').count() <= 1
        && !destination.starts_with('@')
        && !destination.ends_with('@')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

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
