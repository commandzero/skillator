//! Host-local follower registration and read-only inspection of owned replicas.
use super::{MARKER_CONTENT, MARKER_NAME, REPLICA_RELATIVE, config::Config, process};
use crate::config::{Fingerprint, save_bytes};
use crate::domain::{SkillPath, SourceKey};
use crate::library::inspect_skill_metadata;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Follower<'a> {
    pub alias: &'a str,
    pub destination: &'a str,
    pub hostname: Option<&'a str>,
}

pub(crate) struct HostRegistry {
    path: PathBuf,
    original: Config,
    current: Config,
    fingerprint: Fingerprint,
    dirty: bool,
}

impl HostRegistry {
    pub fn load(home: &Path) -> Result<Self, String> {
        let directory = home.join(".skillator");
        let path = directory.join("config.yaml");
        checked_directory(&directory, true)?;
        let bytes = checked_config_bytes(&path)?;
        let (original, fingerprint) = match bytes {
            Some(bytes) => {
                let text = std::str::from_utf8(&bytes)
                    .map_err(|error| format!("{} is not UTF-8: {error}", path.display()))?;
                (
                    Config::parse(text).map_err(|error| error.to_string())?,
                    Fingerprint::for_bytes(&bytes),
                )
            }
            None => (Config::empty(), Fingerprint::Absent),
        };
        Ok(Self {
            path,
            current: original.clone(),
            original,
            fingerprint,
            dirty: false,
        })
    }

    pub fn followers(&self) -> impl Iterator<Item = Follower<'_>> {
        self.current
            .followers()
            .into_iter()
            .flat_map(|hosts| hosts.iter())
            .map(|(alias, host)| Follower {
                alias,
                destination: &host.destination,
                hostname: host.hostname.as_deref(),
            })
    }

    pub fn validate_name(&self, name: &str) -> Result<(), String> {
        self.current
            .validate_follower_name(name)
            .map_err(|error| error.to_string())
    }

    pub fn stage(&mut self, name: &str, hostname: &str) -> Result<(), String> {
        self.current
            .add_follower(name.to_owned(), hostname.to_owned())
            .map_err(|error| error.to_string())?;
        self.dirty = true;
        Ok(())
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn discard(&mut self) {
        self.current = self.original.clone();
        self.dirty = false;
    }

    pub fn save(&mut self) -> Result<(), String> {
        if !self.dirty {
            return Ok(());
        }
        let directory = self.path.parent().expect("configuration has parent");
        if fs::symlink_metadata(directory).is_err_and(|error| error.kind() == ErrorKind::NotFound) {
            fs::create_dir(directory)
                .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
        }
        checked_directory(directory, false)?;
        let current_bytes = checked_config_bytes(&self.path)?;
        let current_fingerprint = current_bytes
            .as_deref()
            .map(Fingerprint::for_bytes)
            .unwrap_or(Fingerprint::Absent);
        if current_fingerprint != self.fingerprint {
            return Err(
                "Configuration changed since it was loaded; reopen it and try again".to_owned(),
            );
        }
        let text = self.current.render().map_err(|error| error.to_string())?;
        let next = save_bytes(&self.path, text.as_bytes(), &self.fingerprint)
            .map_err(|error| error.to_string())?;
        self.fingerprint = next;
        self.original = self.current.clone();
        self.dirty = false;
        Ok(())
    }
}

fn checked_directory(path: &Path, absent_ok: bool) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(format!("unsafe configuration directory {}", path.display())),
        Err(error) if absent_ok && error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("cannot inspect {}: {error}", path.display())),
    }
}

fn checked_config_bytes(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => fs::read(path)
            .map(Some)
            .map_err(|error| format!("cannot read {}: {error}", path.display())),
        Ok(_) => Err(format!("unsafe configuration file {}", path.display())),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("cannot inspect {}: {error}", path.display())),
    }
}

#[derive(Debug)]
pub(crate) struct ProbeResult {
    pub hostname: String,
    pub warning: Option<String>,
}

fn ssh(destination: &str) -> Result<Command, String> {
    super::config::validate_destination(destination, destination)
        .map_err(|error| error.to_string())?;
    let mut command = Command::new("ssh");
    command
        .args([
            "-T",
            "-oBatchMode=yes",
            "-oStrictHostKeyChecking=yes",
            "-oUpdateHostKeys=no",
            "-oControlMaster=no",
            "-oControlPath=none",
            "-oControlPersist=no",
            "-oConnectTimeout=10",
            "-oServerAliveInterval=10",
            "-oServerAliveCountMax=1",
            "--",
        ])
        .arg(&*super::transport::ssh_destination(destination));
    Ok(command)
}

pub(crate) fn probe(destination: &str, cancel: &AtomicBool) -> Result<ProbeResult, String> {
    let output = process::capture_bounded(
        ssh(destination)?.arg("hostname"),
        cancel,
        Duration::from_secs(15),
        4096,
        4096,
    )?;
    let hostname = parse_hostname(&output.stdout)?;
    Ok(ProbeResult {
        hostname,
        warning: output.warning,
    })
}

fn parse_hostname(bytes: &[u8]) -> Result<String, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "SSH hostname is not UTF-8".to_owned())?;
    let hostname = text
        .strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .unwrap_or(text);
    super::config::validate_hostname(hostname)
        .map_err(|error| format!("SSH hostname output is invalid or ambiguous: {error}"))?;
    Ok(hostname.to_owned())
}

#[derive(Debug)]
pub(crate) struct ReplicaSkill {
    pub source_key: String,
    pub skill_path: String,
    pub description: String,
    pub document: String,
    pub diagnostic: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct ReplicaInventory {
    pub path: String,
    pub skills: Vec<ReplicaSkill>,
    pub warning: Option<String>,
}

// Only the known PR38 replica is read. The script uses POSIX shell, find, cmp,
// wc, cat and readlink -n; it never invokes Skillator, Git, rsync, or remote writes.
fn inspection_script() -> String {
    format!(
        r#"set -eu
case ${{HOME:-}} in /*) ;; *) echo 'receiver HOME must be absolute' >&2; exit 1;; esac
case $HOME in *[[:cntrl:]]*) echo 'receiver HOME contains control characters' >&2; exit 1;; esac
home=$(CDPATH= cd -P "$HOME" && printf '%s' "$PWD") || {{ echo 'receiver HOME is unavailable' >&2; exit 1; }}
part=${{home%/}}
for name in .skillator library replica; do
  part=$part/$name
  if [ -L "$part" ]; then echo 'replica path contains a symbolic link' >&2; exit 1; fi
  if [ ! -d "$part" ]; then echo 'owned replica is absent; synchronize it from the leader first' >&2; exit 1; fi
done
root=$part
marker=$root/{MARKER_NAME}
if [ -L "$marker" ] || [ ! -f "$marker" ] || ! printf '%s' '{MARKER_CONTENT}' | cmp - "$marker" >/dev/null 2>&1; then
  echo 'replica ownership marker is invalid; move the unmanaged replica aside' >&2; exit 1
fi
linked=$(find "$root" -type f -links +1 -print) || {{ echo 'cannot inspect replica hard links' >&2; exit 1; }}
if [ -n "$linked" ]; then echo 'replica contains a multiply linked file' >&2; exit 1; fi
printf '%s\000' "$root"
find "$root" -name SKILL.md ! -type d -exec sh -c '
  set -eu
  root=$1; shift
  fail() {{ echo "$1: $relative" >&2; exit 1; }}
  for file do
    relative=${{file#"$root"/}}
    skill=$(CDPATH= cd -P "${{file%/*}}" && printf "%s." "$PWD") || fail "cannot inspect skill directory"
    skill=${{skill%.}}
    current=$skill
    pending=SKILL.md
    hops=0
    while [ -n "$pending" ]; do
      case $pending in
        */*) component=${{pending%%/*}}; pending=${{pending#*/}}; more=yes;;
        *) component=$pending; pending=; more=no;;
      esac
      case $component in
        ""|.) ;;
        ..)
          [ "$current" != "$skill" ] || fail "SKILL.md escapes its skill directory"
          current=$(CDPATH= cd -P "$current/.." && printf "%s." "$PWD") || fail "cannot resolve SKILL.md parent"
          current=${{current%.}}
          ;;
        *)
          document=$current/$component
          if [ -L "$document" ]; then
            [ "$hops" -lt 40 ] || fail "SKILL.md has too many symbolic links"
            # -n avoids BSD/GNU delimiter differences; the sentinel preserves target newlines.
            target=$(readlink -n "$document" && printf ".") || fail "cannot read SKILL.md symbolic link"
            target=${{target%.}}
            case $target in
              /*) fail "SKILL.md contains an absolute symbolic link";;
              "") fail "SKILL.md contains an empty symbolic link";;
            esac
            hops=$((hops + 1))
            if [ "$more" = yes ]; then pending=$target/$pending; else pending=$target; fi
          elif [ "$more" = yes ]; then
            [ -d "$document" ] || fail "SKILL.md parent is not a directory"
            current=$(CDPATH= cd -P "$document" && printf "%s." "$PWD") || fail "cannot resolve SKILL.md directory"
            current=${{current%.}}
          else
            current=$document
          fi
          ;;
      esac
      case $current in
        "$skill"|"$skill"/*) ;;
        *) fail "SKILL.md escapes its skill directory";;
      esac
    done
    document=$current
    [ -f "$document" ] || fail "SKILL.md is not a regular file"
    size=$(wc -c < "$document") || exit 1
    if [ "$size" -gt 262144 ]; then echo "SKILL.md exceeds the inspection size limit: $relative" >&2; exit 1; fi
    printf "%s\000%s\000" "$relative" "$size"
    cat "$document" || exit 1
  done
' sh "$root" {{}} +
"#,
        MARKER_NAME = MARKER_NAME,
        MARKER_CONTENT = MARKER_CONTENT
    )
}

pub(crate) fn inspect(destination: &str, cancel: &AtomicBool) -> Result<ReplicaInventory, String> {
    let output = process::capture_bounded(
        ssh(destination)?.arg(inspection_script()),
        cancel,
        Duration::from_secs(30),
        4 * 1024 * 1024,
        4096,
    )?;
    decode_inventory(&output.stdout, output.warning)
}

fn take_field<'a>(bytes: &mut &'a [u8]) -> Result<&'a str, String> {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .ok_or("invalid replica inspection response")?;
    let field = std::str::from_utf8(&bytes[..end]).map_err(|_| "replica path is not UTF-8")?;
    *bytes = &bytes[end + 1..];
    Ok(field)
}

fn decode_inventory(output: &[u8], warning: Option<String>) -> Result<ReplicaInventory, String> {
    let mut remaining = output;
    let path = take_field(&mut remaining)?.to_owned();
    let root = Path::new(&path);
    if !root.is_absolute()
        || !root.ends_with(REPLICA_RELATIVE)
        || root.components().any(|part| {
            matches!(
                part,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
        || path.chars().any(char::is_control)
    {
        return Err("receiver returned an unsafe replica path".to_owned());
    }
    let mut skills = Vec::new();
    while !remaining.is_empty() {
        let relative = take_field(&mut remaining)?;
        let (source_key, path) = relative
            .split_once("/_skills/")
            .ok_or("invalid replica skill layout")?;
        SourceKey::parse(source_key).map_err(|_| "invalid replica source key")?;
        let skill_path = if path == "SKILL.md" {
            "."
        } else {
            path.strip_suffix("/SKILL.md")
                .ok_or("invalid replica skill document path")?
        };
        SkillPath::parse(skill_path).map_err(|_| "invalid replica skill path")?;
        let size = take_field(&mut remaining)?
            .trim()
            .parse::<usize>()
            .map_err(|_| "invalid replica document length")?;
        if size > 262144 || size > remaining.len() {
            return Err("invalid replica document length".to_owned());
        }
        let document_bytes = &remaining[..size];
        remaining = &remaining[size..];
        let (description, diagnostic, warnings) = metadata(document_bytes, skill_path);
        let document = String::from_utf8_lossy(document_bytes)
            .chars()
            .map(|character| {
                if character.is_control() && character != '\n' && character != '\t' {
                    ' '
                } else {
                    character
                }
            })
            .collect::<String>();
        skills.push(ReplicaSkill {
            source_key: source_key.to_owned(),
            skill_path: skill_path.to_owned(),
            description,
            document,
            diagnostic,
            warnings,
        });
    }
    skills.sort_by(|left, right| {
        (&left.source_key, &left.skill_path).cmp(&(&right.source_key, &right.skill_path))
    });
    Ok(ReplicaInventory {
        path,
        skills,
        warning,
    })
}

fn metadata(bytes: &[u8], path: &str) -> (String, Option<String>, Vec<String>) {
    let directory_name = (path != ".").then(|| path.rsplit('/').next().unwrap_or(path));
    match inspect_skill_metadata(bytes, directory_name) {
        Ok(metadata) => (
            metadata
                .description
                .chars()
                .map(|character| {
                    if character.is_control() {
                        ' '
                    } else {
                        character
                    }
                })
                .collect(),
            (!metadata.errors.is_empty())
                .then(|| process::sanitized(metadata.errors.join("; ").as_bytes())),
            metadata
                .warnings
                .into_iter()
                .map(|warning| process::sanitized(warning.as_bytes()))
                .collect(),
        ),
        Err(issue) => (
            String::new(),
            Some(process::sanitized(issue.as_bytes())),
            Vec::new(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn owned_replica(home: &Path) -> PathBuf {
        let root = home.join(REPLICA_RELATIVE);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(MARKER_NAME), MARKER_CONTENT).unwrap();
        root
    }

    fn execute_inspection(home: &Path) -> std::process::Output {
        Command::new("sh")
            .args(["-c", &inspection_script()])
            .env("HOME", home)
            .output()
            .unwrap()
    }

    #[derive(Debug, PartialEq, Eq)]
    enum ReplicaEntry {
        Directory,
        File(Vec<u8>),
        Symlink(PathBuf),
        Other,
    }

    fn replica_contents(root: &Path) -> std::collections::BTreeMap<PathBuf, ReplicaEntry> {
        fn collect(
            root: &Path,
            directory: &Path,
            entries: &mut std::collections::BTreeMap<PathBuf, ReplicaEntry>,
        ) {
            for entry in fs::read_dir(directory).unwrap() {
                let path = entry.unwrap().path();
                let metadata = fs::symlink_metadata(&path).unwrap();
                let content = if metadata.file_type().is_symlink() {
                    ReplicaEntry::Symlink(fs::read_link(&path).unwrap())
                } else if metadata.is_dir() {
                    collect(root, &path, entries);
                    ReplicaEntry::Directory
                } else if metadata.is_file() {
                    ReplicaEntry::File(fs::read(&path).unwrap())
                } else {
                    ReplicaEntry::Other
                };
                entries.insert(path.strip_prefix(root).unwrap().to_owned(), content);
            }
        }
        let mut entries = std::collections::BTreeMap::new();
        collect(root, root, &mut entries);
        entries
    }

    fn inspected_inventory(home: &Path) -> ReplicaInventory {
        let output = execute_inspection(home);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        decode_inventory(&output.stdout, None).unwrap()
    }

    fn assert_inspection_rejected(home: &Path, root: &Path, diagnostic: &str) {
        let before = replica_contents(root);
        let output = execute_inspection(home);
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(diagnostic),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(replica_contents(root), before);
    }

    fn configuration(home: &Path) -> PathBuf {
        let directory = home.join(".skillator");
        fs::create_dir_all(&directory).unwrap();
        directory.join("config.yaml")
    }

    #[test]
    fn legacy_hosts_and_leader_keep_their_roles_and_destinations() {
        let home = tempfile::tempdir().unwrap();
        let path = configuration(home.path());
        fs::write(
            &path,
            "version: 1\nhosts:\n  build: {destination: user@host}\n",
        )
        .unwrap();
        let mut registry = HostRegistry::load(home.path()).unwrap();
        assert_eq!(
            registry.followers().collect::<Vec<_>>(),
            [Follower {
                alias: "build",
                destination: "user@host",
                hostname: None,
            }]
        );
        registry.stage("new-host", "worker-07.example.net").unwrap();
        registry.save().unwrap();
        let restored = HostRegistry::load(home.path()).unwrap();
        let followers: Vec<_> = restored.followers().collect();
        assert_eq!(followers[0].destination, "user@host");
        assert_eq!(followers[1].alias, "new-host");
        assert_eq!(followers[1].destination, "new-host");
        assert_eq!(followers[1].hostname, Some("worker-07.example.net"));
        fs::write(&path, "version: 1\nleader: {destination: user@main}\n").unwrap();
        let mut follower = HostRegistry::load(home.path()).unwrap();
        assert!(follower.stage("next", "next.example").is_err());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "version: 1\nleader: {destination: user@main}\n"
        );
    }

    #[test]
    fn staging_is_reversible_and_stale_changes_are_not_overwritten() {
        let home = tempfile::tempdir().unwrap();
        let path = configuration(home.path());
        let original = b"version: 1\nhosts: {old: {destination: user@old}}\n";
        fs::write(&path, original).unwrap();
        let mut registry = HostRegistry::load(home.path()).unwrap();
        for name in ["local", "old", "-oProxyCommand=bad", "space name", "a;bad"] {
            assert!(registry.validate_name(name).is_err(), "{name}");
        }
        registry.stage("new", "worker-07").unwrap();
        registry.discard();
        assert!(!registry.dirty());
        assert_eq!(fs::read(&path).unwrap(), original);
        registry.stage("new", "worker-07").unwrap();
        let changed = b"version: 1\nhosts: {old: {destination: changed}}\n";
        fs::write(&path, changed).unwrap();
        assert!(registry.save().unwrap_err().contains("changed"));
        assert!(registry.dirty());
        assert_eq!(fs::read(&path).unwrap(), changed);
    }

    #[test]
    fn absent_registry_is_created_only_when_saved_and_unsafe_paths_never_replaced() {
        let home = tempfile::tempdir().unwrap();
        let mut registry = HostRegistry::load(home.path()).unwrap();
        assert!(registry.stage("bad", "not a hostname!").is_err());
        assert!(!registry.dirty());
        assert!(registry.followers().next().is_none());
        registry.stage("new", "worker.example").unwrap();
        assert!(!home.path().join(".skillator").exists());
        registry.save().unwrap();
        assert_eq!(
            HostRegistry::load(home.path())
                .unwrap()
                .followers()
                .next()
                .unwrap()
                .alias,
            "new"
        );
        let outside = tempfile::tempdir().unwrap();
        let outside_file = outside.path().join("config.yaml");
        fs::write(&outside_file, "untouched").unwrap();
        let config = home.path().join(".skillator/config.yaml");
        fs::remove_file(&config).unwrap();
        symlink(&outside_file, &config).unwrap();
        assert!(HostRegistry::load(home.path()).is_err());
        assert_eq!(fs::read(&outside_file).unwrap(), b"untouched");
        fs::remove_file(&config).unwrap();
        fs::remove_dir(home.path().join(".skillator")).unwrap();
        symlink(outside.path(), home.path().join(".skillator")).unwrap();
        assert!(HostRegistry::load(home.path()).is_err());
    }

    #[test]
    fn malformed_registry_and_probe_output_are_rejected() {
        let home = tempfile::tempdir().unwrap();
        let path = configuration(home.path());
        for data in [
            "version: 1\nhosts: {}\n",
            "version: 2\nhosts: {a: {destination: a}}\n",
            "version: 1\nhosts: {a: {destination: a, hostname: 'bad name'}}\n",
            "version: 1\nhosts: {a: {destination: a, surprise: true}}\n",
        ] {
            fs::write(&path, data).unwrap();
            assert!(HostRegistry::load(home.path()).is_err(), "{data}");
            assert_eq!(fs::read_to_string(&path).unwrap(), data);
        }
        for valid in [
            b"worker-07.example.net\n".as_slice(),
            b"worker-07\r\n",
            b"worker-07.",
        ] {
            assert!(parse_hostname(valid).is_ok());
        }
        for invalid in [
            b"".as_slice(),
            b"worker\nworker\n",
            b"banner\nworker\n",
            b"worker\r",
            b"worker\n\n",
            b"worker\x1b[31m",
            b"-worker",
            b"worker..example",
            b"worker example",
            b"\xff",
        ] {
            assert!(parse_hostname(invalid).is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn frontmatter_descriptions_preserve_length_without_terminal_controls() {
        let source = format!(
            "---\nname: demo\ndescription: \"{}\\u001b[31m\"\n---\n",
            "A".repeat(700)
        );
        let (description, diagnostic, warnings) = metadata(source.as_bytes(), "demo");
        assert!(diagnostic.is_none());
        assert!(warnings.is_empty());
        assert!(description.starts_with(&"A".repeat(700)));
        assert!(description.ends_with(" [31m"));
        assert!(!description.chars().any(char::is_control));
        let (description, diagnostic, warnings) =
            metadata(b"---\nname: demo\ndescription: Root skill\n---\n", ".");
        assert_eq!(description, "Root skill");
        assert!(diagnostic.is_none());
        assert!(warnings.is_empty());
        let (_, diagnostic, warnings) = metadata(
            b"---\nname: \"bad\\u001b[31m\"\ndescription: unsafe\n---\n",
            "demo",
        );
        assert!(diagnostic.is_some());
        assert!(
            diagnostic
                .unwrap()
                .chars()
                .all(|character| !character.is_control())
        );
        assert!(
            warnings
                .iter()
                .all(|warning| !warning.chars().any(char::is_control))
        );
    }

    #[test]
    fn replacing_a_loaded_registry_with_a_link_blocks_save() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let path = configuration(home.path());
        fs::write(&path, "version: 1\nhosts: {old: {destination: old}}\n").unwrap();
        let mut registry = HostRegistry::load(home.path()).unwrap();
        registry.stage("new", "new.example").unwrap();
        let outside_path = outside.path().join("config.yaml");
        fs::write(&outside_path, "important external bytes").unwrap();
        fs::remove_file(&path).unwrap();
        symlink(&outside_path, &path).unwrap();
        assert!(registry.save().is_err());
        assert_eq!(
            fs::read(&outside_path).unwrap(),
            b"important external bytes"
        );
        assert!(registry.dirty());
    }

    #[test]
    fn inspection_reads_owned_replica_documents_without_mutating_content() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join(REPLICA_RELATIVE);
        let skill = root.join("local/library/_skills/demo");
        fs::create_dir_all(&skill).unwrap();
        fs::write(root.join(MARKER_NAME), MARKER_CONTENT).unwrap();
        let text = "---\nname: demo\ndescription: Remote documentation\n---\nReal body\n";
        fs::write(skill.join("SKILL.md"), text).unwrap();
        let execute = || {
            Command::new("sh")
                .args(["-c", &inspection_script()])
                .env("HOME", home.path())
                .output()
                .unwrap()
        };
        let inspected = execute();
        assert!(
            inspected.status.success(),
            "{}",
            String::from_utf8_lossy(&inspected.stderr)
        );
        let decoded = decode_inventory(&inspected.stdout, None).unwrap();
        assert_eq!(decoded.path, root.canonicalize().unwrap().to_str().unwrap());
        assert_eq!(decoded.skills.len(), 1);
        assert_eq!(decoded.skills[0].source_key, "local/library");
        assert_eq!(decoded.skills[0].skill_path, "demo");
        assert_eq!(decoded.skills[0].description, "Remote documentation");
        assert_eq!(decoded.skills[0].document, text);
        assert_eq!(fs::read_to_string(skill.join("SKILL.md")).unwrap(), text);

        fs::write(
            skill.join("SKILL.md"),
            "---\nname: wrong\ndescription: Remote\n---\n",
        )
        .unwrap();
        let inspected = execute();
        assert!(inspected.status.success());
        let decoded = decode_inventory(&inspected.stdout, None).unwrap();
        assert!(decoded.skills[0].diagnostic.is_none());
        assert!(
            decoded.skills[0]
                .warnings
                .iter()
                .any(|warning| warning.contains("directory"))
        );
        assert_eq!(
            decoded.skills[0].document,
            "---\nname: wrong\ndescription: Remote\n---\n"
        );
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: ../unsafe\ndescription: Remote\n---\n",
        )
        .unwrap();
        let inspected = execute();
        let decoded = decode_inventory(&inspected.stdout, None).unwrap();
        assert!(decoded.skills[0].diagnostic.is_some());
        fs::hard_link(skill.join("SKILL.md"), skill.join("second.md")).unwrap();
        assert!(!execute().status.success());
        fs::remove_file(skill.join("second.md")).unwrap();
        fs::remove_file(skill.join("SKILL.md")).unwrap();
        symlink("/etc/passwd", skill.join("SKILL.md")).unwrap();
        assert!(!execute().status.success());
        fs::remove_file(root.join(MARKER_NAME)).unwrap();
        assert!(!execute().status.success());
    }

    #[test]
    fn inspection_reads_internal_document_links_at_their_registered_paths() {
        let home = tempfile::tempdir().unwrap();
        let root = owned_replica(home.path());
        let skills = root.join("local/library/_skills");
        let demo = skills.join("demo");
        fs::create_dir_all(&demo).unwrap();
        let text = "---\nname: demo\ndescription: Linked documentation\n---\nLinked body\n";
        fs::write(demo.join("instructions.md"), text).unwrap();
        symlink("instructions.md", demo.join("SKILL.md")).unwrap();
        let root_text = "---\nname: root-skill\ndescription: Source root\n---\nRoot body\n";
        fs::write(skills.join("root.md"), root_text).unwrap();
        symlink("./root.md", skills.join("SKILL.md")).unwrap();
        let before = replica_contents(&root);

        let inventory = inspected_inventory(home.path());
        assert_eq!(
            inventory.path,
            root.canonicalize().unwrap().to_str().unwrap()
        );
        assert_eq!(inventory.skills.len(), 2);
        assert_eq!(inventory.skills[0].source_key, "local/library");
        assert_eq!(inventory.skills[0].skill_path, ".");
        assert_eq!(inventory.skills[0].description, "Source root");
        assert_eq!(inventory.skills[0].document, root_text);
        assert!(inventory.skills[0].diagnostic.is_none());
        assert!(inventory.skills[0].warnings.is_empty());
        assert_eq!(inventory.skills[1].skill_path, "demo");
        assert_eq!(inventory.skills[1].description, "Linked documentation");
        assert_eq!(inventory.skills[1].document, text);
        assert!(inventory.skills[1].diagnostic.is_none());
        assert!(inventory.skills[1].warnings.is_empty());
        assert_eq!(replica_contents(&root), before);

        let mismatched = "---\nname: another-name\ndescription: Still readable\n---\n";
        fs::write(demo.join("instructions.md"), mismatched).unwrap();
        let before = replica_contents(&root);
        let inventory = inspected_inventory(home.path());
        assert_eq!(inventory.skills[1].document, mismatched);
        assert!(inventory.skills[1].diagnostic.is_none());
        assert!(
            inventory.skills[1]
                .warnings
                .iter()
                .any(|warning| warning.contains("directory"))
        );
        assert_eq!(replica_contents(&root), before);

        let invalid = "---\nname: ../unsafe\ndescription: Read-only diagnostic\n---\n";
        fs::write(demo.join("instructions.md"), invalid).unwrap();
        let before = replica_contents(&root);
        let inventory = inspected_inventory(home.path());
        assert_eq!(inventory.skills[1].document, invalid);
        assert!(inventory.skills[1].diagnostic.is_some());
        assert_eq!(replica_contents(&root), before);
    }

    #[test]
    fn inspection_resolves_nested_link_chains_from_physical_parent_directories() {
        let home = tempfile::tempdir().unwrap();
        let root = owned_replica(home.path());
        let skill = root.join("owner/repository/_skills/group/nested");
        let docs = skill.join("docs");
        fs::create_dir_all(docs.join("deep")).unwrap();
        let text = "---\nname: nested\ndescription: Nested chain\n---\nActual nested body\n";
        fs::write(docs.join("instructions.md\n"), text).unwrap();
        symlink("aliases/first.md", skill.join("SKILL.md")).unwrap();
        symlink("docs/deep", skill.join("aliases")).unwrap();
        symlink("../second.md", docs.join("deep/first.md")).unwrap();
        symlink("./instructions.md\n", docs.join("second.md")).unwrap();
        let before = replica_contents(&root);

        let inventory = inspected_inventory(home.path());
        assert_eq!(inventory.skills.len(), 1);
        assert_eq!(inventory.skills[0].source_key, "owner/repository");
        assert_eq!(inventory.skills[0].skill_path, "group/nested");
        assert_eq!(inventory.skills[0].description, "Nested chain");
        assert_eq!(inventory.skills[0].document, text);
        assert!(inventory.skills[0].diagnostic.is_none());
        assert!(inventory.skills[0].warnings.is_empty());
        assert_eq!(replica_contents(&root), before);
    }

    #[test]
    fn inspection_rejects_unsafe_document_links_without_changing_the_replica() {
        for case in [
            "escaping",
            "escaping-and-returning",
            "chained-escaping",
            "absolute-internal",
            "absolute-external",
            "chained-absolute",
            "cyclic",
            "broken",
            "chained-broken",
            "broken-parent",
            "directory",
        ] {
            let home = tempfile::tempdir().unwrap();
            let root = owned_replica(home.path());
            let skill = root.join("local/library/_skills/demo");
            let sibling = root.join("local/library/_skills/demo-sibling");
            fs::create_dir_all(&skill).unwrap();
            fs::create_dir_all(&sibling).unwrap();
            let text = "---\nname: demo\ndescription: Boundary fixture\n---\n";
            fs::write(skill.join("instructions.md"), text).unwrap();
            fs::write(sibling.join("instructions.md"), text).unwrap();
            let outside = home.path().join("external.md");
            fs::write(&outside, "Untouched external document").unwrap();
            let (target, diagnostic) = match case {
                "escaping" => (
                    PathBuf::from("../demo-sibling/instructions.md"),
                    "escapes its skill directory",
                ),
                "escaping-and-returning" => (
                    PathBuf::from("../demo/instructions.md"),
                    "escapes its skill directory",
                ),
                "chained-escaping" => {
                    symlink("../demo-sibling/instructions.md", skill.join("next.md")).unwrap();
                    (PathBuf::from("next.md"), "escapes its skill directory")
                }
                "absolute-internal" => (skill.join("instructions.md"), "absolute symbolic link"),
                "absolute-external" => (outside.clone(), "absolute symbolic link"),
                "chained-absolute" => {
                    symlink(skill.join("instructions.md"), skill.join("next.md")).unwrap();
                    (PathBuf::from("next.md"), "absolute symbolic link")
                }
                "cyclic" => {
                    symlink("SKILL.md", skill.join("next.md")).unwrap();
                    (PathBuf::from("next.md"), "too many symbolic links")
                }
                "broken" => (PathBuf::from("missing.md"), "not a regular file"),
                "chained-broken" => {
                    symlink("missing.md", skill.join("next.md")).unwrap();
                    (PathBuf::from("next.md"), "not a regular file")
                }
                "broken-parent" => (PathBuf::from("missing/instructions.md"), "not a directory"),
                "directory" => {
                    fs::create_dir(skill.join("docs")).unwrap();
                    (PathBuf::from("docs"), "not a regular file")
                }
                _ => unreachable!(),
            };
            symlink(target, skill.join("SKILL.md")).unwrap();
            assert_inspection_rejected(home.path(), &root, diagnostic);
            assert_eq!(
                fs::read_to_string(&outside).unwrap(),
                "Untouched external document",
                "{case}"
            );
        }
    }

    #[test]
    fn inspection_rejects_unsafe_directory_links_in_document_paths() {
        for case in ["escaping", "absolute", "cyclic", "broken"] {
            let home = tempfile::tempdir().unwrap();
            let root = owned_replica(home.path());
            let skill = root.join("local/library/_skills/demo");
            let sibling = root.join("local/library/_skills/demo-sibling");
            fs::create_dir_all(skill.join("nested")).unwrap();
            fs::create_dir_all(&sibling).unwrap();
            fs::write(skill.join("nested/instructions.md"), "Internal document").unwrap();
            fs::write(sibling.join("instructions.md"), "Sibling document").unwrap();
            let (target, diagnostic) = match case {
                "escaping" => (
                    PathBuf::from("../demo-sibling"),
                    "escapes its skill directory",
                ),
                "absolute" => (skill.join("nested"), "absolute symbolic link"),
                "cyclic" => (PathBuf::from("docs"), "too many symbolic links"),
                "broken" => (PathBuf::from("missing"), "not a directory"),
                _ => unreachable!(),
            };
            symlink(target, skill.join("docs")).unwrap();
            symlink("docs/instructions.md", skill.join("SKILL.md")).unwrap();
            assert_inspection_rejected(home.path(), &root, diagnostic);
        }
    }

    #[test]
    fn inspection_ignores_supporting_directories_named_skill_md() {
        let home = tempfile::tempdir().unwrap();
        let root = owned_replica(home.path());
        let skill = root.join("local/library/_skills/demo");
        fs::create_dir_all(skill.join("docs/SKILL.md")).unwrap();
        let text = "---\nname: demo\ndescription: Supporting directory\n---\nReal body\n";
        fs::write(skill.join("SKILL.md"), text).unwrap();
        fs::write(skill.join("docs/SKILL.md/asset.png"), b"Supporting asset").unwrap();
        let before = replica_contents(&root);

        let inventory = inspected_inventory(home.path());
        assert_eq!(inventory.skills.len(), 1);
        assert_eq!(inventory.skills[0].source_key, "local/library");
        assert_eq!(inventory.skills[0].skill_path, "demo");
        assert_eq!(inventory.skills[0].document, text);
        assert_eq!(replica_contents(&root), before);
    }

    #[test]
    fn inspection_rejects_unsupported_document_types() {
        // Keep the Unix socket pathname below the receiver platform limit.
        let home = tempfile::tempdir_in("/tmp").unwrap();
        let root = owned_replica(home.path());
        let skill = root.join("local/library/_skills/demo");
        fs::create_dir_all(&skill).unwrap();
        let _socket = std::os::unix::net::UnixListener::bind(skill.join("SKILL.md")).unwrap();
        assert_inspection_rejected(home.path(), &root, "not a regular file");
    }

    #[test]
    fn inspection_bounds_link_hops_and_preserves_document_limits() {
        for links in [40, 41] {
            let home = tempfile::tempdir().unwrap();
            let root = owned_replica(home.path());
            let skill = root.join("local/library/_skills/demo");
            fs::create_dir_all(&skill).unwrap();
            let text = "---\nname: demo\ndescription: Bounded chain\n---\n";
            fs::write(skill.join("instructions.md"), text).unwrap();
            for index in 0..links {
                let name = if index == 0 {
                    "SKILL.md".to_owned()
                } else {
                    format!("link-{index}.md")
                };
                let target = if index + 1 == links {
                    "instructions.md".to_owned()
                } else {
                    format!("link-{}.md", index + 1)
                };
                symlink(target, skill.join(name)).unwrap();
            }
            if links == 40 {
                let before = replica_contents(&root);
                let inventory = inspected_inventory(home.path());
                assert_eq!(inventory.skills.len(), 1);
                assert_eq!(inventory.skills[0].skill_path, "demo");
                assert_eq!(inventory.skills[0].document, text);
                assert_eq!(replica_contents(&root), before);
            } else {
                assert_inspection_rejected(home.path(), &root, "too many symbolic links");
            }
        }

        let home = tempfile::tempdir().unwrap();
        let root = owned_replica(home.path());
        let skill = root.join("local/library/_skills/demo");
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("instructions.md"), vec![b'x'; 262145]).unwrap();
        symlink("instructions.md", skill.join("SKILL.md")).unwrap();
        assert_inspection_rejected(home.path(), &root, "size limit");
        fs::write(skill.join("instructions.md"), "Small linked document").unwrap();
        fs::hard_link(skill.join("instructions.md"), skill.join("second.md")).unwrap();
        assert_inspection_rejected(home.path(), &root, "multiply linked file");
    }

    #[test]
    fn absent_and_unmarked_replicas_are_diagnostic_only() {
        let home = tempfile::tempdir().unwrap();
        let execute = || {
            Command::new("sh")
                .args(["-c", &inspection_script()])
                .env("HOME", home.path())
                .output()
                .unwrap()
        };
        let absent = execute();
        assert!(!absent.status.success());
        assert!(String::from_utf8_lossy(&absent.stderr).contains("absent"));
        assert!(!home.path().join(REPLICA_RELATIVE).exists());
        fs::create_dir_all(home.path().join(REPLICA_RELATIVE)).unwrap();
        let unmarked = execute();
        assert!(!unmarked.status.success());
        assert!(String::from_utf8_lossy(&unmarked.stderr).contains("ownership marker"));
    }
}
