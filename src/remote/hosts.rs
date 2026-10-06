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
    command.args([
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
        destination,
    ]);
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
// wc and cat; it never invokes Skillator, Git, rsync, or remote writes.
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
linked=$(find "$root" -type l -name SKILL.md -print) || {{ echo 'cannot inspect replica links' >&2; exit 1; }}
if [ -n "$linked" ]; then echo 'replica contains a linked SKILL.md; inspection cannot safely read it' >&2; exit 1; fi
printf '%s\000' "$root"
find "$root" -type f -name SKILL.md -exec sh -c '
  root=$1; shift
  for file do
    relative=${{file#"$root"/}}
    size=$(wc -c < "$file") || exit 1
    if [ "$size" -gt 262144 ]; then echo "SKILL.md exceeds the inspection size limit: $relative" >&2; exit 1; fi
    printf "%s\000%s\000" "$relative" "$size"
    cat "$file" || exit 1
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
