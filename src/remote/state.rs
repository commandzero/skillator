use super::{Error, Result};
use crate::config::Fingerprint;
use crate::fs_safety::Directory;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Entry {
    File { hash: String, executable: bool },
    Link { target: String },
    Directory,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Baseline {
    pub files: BTreeMap<String, Option<Entry>>,
    #[serde(default)]
    pub contexts: BTreeMap<String, String>,
    pub user: BTreeMap<String, Option<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GitProvenance {
    pub source: String,
    pub origin: String,
    pub commit: String,
    pub branch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct History {
    pub version: u64,
    pub id: Option<String>,
    pub peers: BTreeMap<String, Baseline>,
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
    #[serde(default)]
    pub provenance: BTreeMap<String, GitProvenance>,
}

impl Default for History {
    fn default() -> Self {
        Self {
            version: 1,
            id: None,
            peers: BTreeMap::new(),
            aliases: BTreeMap::new(),
            provenance: BTreeMap::new(),
        }
    }
}

impl History {
    pub fn load(home: &Path) -> Result<Self> {
        match read_contained(home, ".skillator/rsync/state.json")? {
            None => Ok(Self::default()),
            Some(bytes) => {
                let state: Self = serde_json::from_slice(&bytes).map_err(Error::input_display)?;
                if state.version != 1
                    || state.id.as_ref().is_none_or(|id| !valid_id(id))
                    || state.peers.keys().any(|id| !valid_id(id))
                    || state.aliases.values().any(|id| !valid_id(id))
                {
                    return Err(Error::input(
                        "unsupported or invalid synchronization history; preserve state.json for recovery",
                    ));
                }
                for peer in state.peers.values() {
                    for path in peer.files.keys() {
                        relative(path)?;
                    }
                }
                Ok(state)
            }
        }
    }

    pub fn save(&self, home: &Path, expected: &Fingerprint) -> Result<()> {
        self.save_with(home, expected, || {})
    }

    fn save_with(
        &self,
        home: &Path,
        expected: &Fingerprint,
        after_exchange: impl FnOnce(),
    ) -> Result<()> {
        let bytes = serde_json::to_vec(self).map_err(Error::input_display)?;
        save_contained_bytes_with(
            home,
            ".skillator/rsync/state.json",
            &bytes,
            expected,
            after_exchange,
        )
    }
}

pub(super) fn save_contained_bytes(
    home: &Path,
    value: &str,
    bytes: &[u8],
    expected: &Fingerprint,
) -> Result<()> {
    save_contained_bytes_with(home, value, bytes, expected, || {})
}

fn save_contained_bytes_with(
    home: &Path,
    value: &str,
    bytes: &[u8],
    expected: &Fingerprint,
    after_exchange: impl FnOnce(),
) -> Result<()> {
    let path = contained(home, value)?;
    let parent =
        Directory::open_parent(home, path.parent().unwrap()).map_err(Error::input_display)?;
    let name = path.file_name().unwrap();
    let current = read_optional_at(&parent, name)?;
    if &current
        .as_deref()
        .map(Fingerprint::for_bytes)
        .unwrap_or(Fingerprint::Absent)
        != expected
    {
        return Err(Error::input(
            "synchronization history changed before saving",
        ));
    }
    if current.as_deref() == Some(bytes) {
        return Ok(());
    }
    let stage_name = format!(".skillator-rsync-{}", new_id()?);
    let stage_name = std::ffi::OsStr::new(&stage_name);
    let mut stage = parent
        .create_file(stage_name)
        .map_err(Error::input_display)?;
    if let Err(error) = stage.write_all(bytes).and_then(|()| stage.sync_all()) {
        let _ = parent.remove(stage_name);
        return Err(Error::input_display(error));
    }
    let mut stage_contains_prior = false;
    let result = (|| -> Result<()> {
        let latest = read_optional_at(&parent, name)?;
        if &latest
            .as_deref()
            .map(Fingerprint::for_bytes)
            .unwrap_or(Fingerprint::Absent)
            != expected
        {
            return Err(Error::input(
                "synchronization history changed during saving",
            ));
        }
        if *expected == Fingerprint::Absent {
            parent
                .rename_noreplace(stage_name, name)
                .map_err(Error::input_display)?;
            parent.as_file().sync_all().map_err(Error::input_display)?;
            if read_optional_at(&parent, name)?.as_deref() != Some(bytes) {
                return Err(Error::input(
                    "synchronization history changed after publication",
                ));
            }
            return Ok(());
        }
        parent
            .rename_exchange(stage_name, name)
            .map_err(Error::input_display)?;
        stage_contains_prior = true;
        after_exchange();
        parent.as_file().sync_all().map_err(Error::input_display)?;
        let moved = read_optional_at(&parent, stage_name)?;
        if &moved
            .as_deref()
            .map(Fingerprint::for_bytes)
            .unwrap_or(Fingerprint::Absent)
            != expected
        {
            return Err(Error::input(format!(
                "synchronization history changed during saving; preserve the exchanged entries and recover them from {}",
                path.parent().unwrap().join(stage_name).display()
            )));
        }
        let live = read_optional_at(&parent, name)?;
        if live.as_deref() != Some(bytes) {
            return Err(Error::input(format!(
                "synchronization history changed after publication; preserve the prior version at {} for recovery",
                path.parent().unwrap().join(stage_name).display()
            )));
        }
        parent.remove(stage_name).map_err(Error::input_display)?;
        stage_contains_prior = false;
        parent.as_file().sync_all().map_err(Error::input_display)?;
        Ok(())
    })();
    if result.is_err() && !stage_contains_prior {
        let _ = parent.remove(stage_name);
    }
    result?;
    Ok(())
}

fn read_optional_at(parent: &Directory, name: &std::ffi::OsStr) -> Result<Option<Vec<u8>>> {
    let mut file = match parent.open_file(name) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(Error::input_display(error)),
    };
    if !file.metadata().map_err(Error::input_display)?.is_file() {
        return Err(Error::input("configuration must be a regular file"));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(Error::input_display)?;
    Ok(Some(bytes))
}

pub(super) fn read_contained(home: &Path, value: &str) -> Result<Option<Vec<u8>>> {
    let path = contained(home, value)?;
    let parent = match Directory::open_existing_parent(home, path.parent().unwrap()) {
        Ok(parent) => parent,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(Error::input_display(error)),
    };
    read_optional_at(&parent, path.file_name().unwrap())
}

pub(super) fn valid_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

pub(super) fn new_id() -> Result<String> {
    let mut bytes = [0; 16];
    File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(Error::input_display)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

pub(super) fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
pub(super) fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::input_display(error)),
        Ok(meta) if !meta.is_file() => Err(Error::input(format!(
            "expected a regular file: {}",
            path.display()
        ))),
        Ok(_) => fs::read(path).map(Some).map_err(Error::input_display),
    }
}

#[cfg(test)]
pub(super) fn fingerprint(path: &Path) -> Result<Fingerprint> {
    Ok(read_optional(path)?
        .as_deref()
        .map(Fingerprint::for_bytes)
        .unwrap_or(Fingerprint::Absent))
}

pub(super) fn fingerprint_contained(home: &Path, value: &str) -> Result<Fingerprint> {
    Ok(read_contained(home, value)?
        .as_deref()
        .map(Fingerprint::for_bytes)
        .unwrap_or(Fingerprint::Absent))
}

pub(super) fn relative(value: &str) -> Result<&Path> {
    let path = Path::new(value);
    if value.is_empty()
        || value.contains('\0')
        || value.contains('\n')
        || value.contains('\r')
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(Error::input(format!(
            "expected a contained relative path: {value:?}"
        )));
    }
    Ok(path)
}

/// Control files belong to their local workflows, never to skill transfers.
pub(super) fn administrative(path: &Path) -> bool {
    let parts: Vec<_> = path.components().map(|part| part.as_os_str()).collect();
    parts.iter().any(|part| *part == ".git")
        || parts.windows(2).any(|pair| {
            (pair[0] == ".agents" && pair[1] == "skillator.yaml")
                || (pair[0] == ".skillator"
                    && ["config.yaml", "library.yaml", "targets.yaml", "rsync"]
                        .iter()
                        .any(|name| pair[1] == *name))
        })
}

pub(super) fn transferable(home: &Path, path: &str) -> Result<bool> {
    if administrative(relative(path)?) {
        return Ok(false);
    }
    let Some(actual) = observation_path(home, path)? else {
        return Ok(true);
    };
    let actual = if actual.exists() {
        actual.canonicalize().map_err(Error::input_display)?
    } else {
        actual
    };
    Ok(!administrative(
        actual
            .strip_prefix(home.canonicalize().map_err(Error::input_display)?)
            .map_err(Error::input_display)?,
    ))
}

/// Resolve physical parents but leave the final entry intact for link observation.
pub(super) fn contained(home: &Path, value: &str) -> Result<PathBuf> {
    resolve(home, value, false)?
        .ok_or_else(|| Error::input("destination parent is not a directory"))
}

/// Observation can prove absence below a regular-file ancestor without permitting writes through it.
pub(super) fn observation_path(home: &Path, value: &str) -> Result<Option<PathBuf>> {
    resolve(home, value, true)
}

fn resolve(home: &Path, value: &str, observation: bool) -> Result<Option<PathBuf>> {
    let path = relative(value)?;
    let home = home.canonicalize().map_err(Error::input_display)?;
    let joined = home.join(path);
    let mut parent = joined.parent().unwrap().to_path_buf();
    let mut missing = Vec::new();
    loop {
        match parent.canonicalize() {
            Ok(real) => {
                if !real.starts_with(&home) {
                    return Err(Error::input("path escapes the user home"));
                }
                if real.is_file() {
                    if observation {
                        return Ok(None);
                    }
                    return Err(Error::input("destination parent is not a directory"));
                }
                let mut resolved = real;
                for name in missing.iter().rev() {
                    resolved.push(name);
                }
                resolved.push(joined.file_name().unwrap());
                return Ok(Some(resolved));
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    || (observation && error.kind() == std::io::ErrorKind::NotADirectory) =>
            {
                if fs::symlink_metadata(&parent).is_ok() {
                    return Err(Error::input("unresolvable destination parent"));
                }
                missing.push(
                    parent
                        .file_name()
                        .ok_or_else(|| Error::input("invalid path parent"))?
                        .to_os_string(),
                );
                parent.pop();
            }
            Err(error) => return Err(Error::input_display(error)),
        }
    }
}

pub(super) fn home_relative(home: &Path, path: &Path) -> Result<String> {
    let canonical_home = home.canonicalize().map_err(Error::input_display)?;
    if path.exists() {
        let physical = path.canonicalize().map_err(Error::input_display)?;
        if physical == canonical_home {
            return Err(Error::input(
                "home-rooted locations and skills are unsupported; register directories below the user home",
            ));
        }
        if !physical.starts_with(&canonical_home) {
            return Err(Error::input(
                "library location resolves outside the user home",
            ));
        }
    }
    let value = path
        .strip_prefix(home)
        .or_else(|_| path.strip_prefix(&canonical_home))
        .map_err(|_| {
            Error::input(format!(
                "library path is outside the user home: {}",
                path.display()
            ))
        })?;
    let value = value
        .to_str()
        .ok_or_else(|| Error::input("non-UTF-8 path cannot be synchronized"))?;
    relative(value)?;
    contained(home, value)?;
    Ok(value.to_owned())
}

pub(super) fn observe(path: &Path) -> Result<Option<Entry>> {
    use std::os::unix::fs::PermissionsExt;
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(Error::input_display(error)),
    };
    let entry = if meta.is_file() {
        let mut file = File::open(path).map_err(Error::input_display)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0; 65536];
        loop {
            let count = file.read(&mut buffer).map_err(Error::input_display)?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
        Entry::File {
            hash: hasher
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
            executable: meta.permissions().mode() & 0o111 != 0,
        }
    } else if meta.is_dir() {
        Entry::Directory
    } else if meta.file_type().is_symlink() {
        Entry::Link {
            target: fs::read_link(path)
                .map_err(Error::input_display)?
                .to_str()
                .ok_or_else(|| Error::input("non-UTF-8 link"))?
                .to_owned(),
        }
    } else {
        return Err(Error::input(format!(
            "unsupported file type: {}",
            path.display()
        )));
    };
    Ok(Some(entry))
}

/// Observe an entry through a held parent inode, independent of pathname swaps.
pub(super) fn observe_at(parent: &Directory, name: &std::ffi::OsStr) -> Result<Option<Entry>> {
    let stat = match parent.metadata(name) {
        Ok(stat) => stat,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(Error::input_display(error)),
    };
    let entry = match stat.st_mode & libc::S_IFMT {
        libc::S_IFREG => {
            let mut file = parent.open_file(name).map_err(Error::input_display)?;
            let mut hasher = Sha256::new();
            let mut buffer = [0; 65536];
            loop {
                let count = file.read(&mut buffer).map_err(Error::input_display)?;
                if count == 0 {
                    break;
                }
                hasher.update(&buffer[..count]);
            }
            Entry::File {
                hash: hasher
                    .finalize()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect(),
                executable: stat.st_mode & 0o111 != 0,
            }
        }
        libc::S_IFDIR => Entry::Directory,
        libc::S_IFLNK => Entry::Link {
            target: parent
                .read_link(name)
                .map_err(Error::input_display)?
                .to_str()
                .ok_or_else(|| Error::input("non-UTF-8 link"))?
                .to_owned(),
        },
        _ => return Err(Error::input("unsupported file type during publication")),
    };
    Ok(Some(entry))
}

#[cfg(test)]
pub(super) fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = File::options()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(Error::input_display)?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(Error::input_display)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_round_trip_is_conditional_and_versions_are_preserved() {
        let home = tempfile::tempdir().unwrap();
        assert!(History::load(home.path()).unwrap().id.is_none());
        let history = History {
            id: Some(new_id().unwrap()),
            ..History::default()
        };
        history.save(home.path(), &Fingerprint::Absent).unwrap();
        assert_eq!(History::load(home.path()).unwrap().id, history.id);
        assert!(history.save(home.path(), &Fingerprint::Absent).is_err());
        let path = home.path().join(".skillator/rsync/state.json");
        fs::write(&path, b"{\"version\":99,\"id\":null,\"peers\":{}}").unwrap();
        assert!(History::load(home.path()).is_err());
        assert!(fs::read_to_string(&path).unwrap().contains("99"));
    }

    #[test]
    fn history_save_rejects_a_state_link_outside_home() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir_all(home.path().join(".skillator/rsync")).unwrap();
        fs::write(outside.path().join("state.json"), "outside").unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("state.json"),
            home.path().join(".skillator/rsync/state.json"),
        )
        .unwrap();
        let history = History {
            id: Some(new_id().unwrap()),
            ..History::default()
        };
        assert!(History::load(home.path()).is_err());
        assert!(fingerprint_contained(home.path(), ".skillator/rsync/state.json").is_err());
        assert!(history.save(home.path(), &Fingerprint::Absent).is_err());
        assert_eq!(
            fs::read_to_string(outside.path().join("state.json")).unwrap(),
            "outside"
        );
    }

    #[test]
    fn contained_library_save_rejects_a_final_symlink() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir_all(home.path().join(".skillator")).unwrap();
        fs::write(outside.path().join("library.yaml"), "outside").unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("library.yaml"),
            home.path().join(".skillator/library.yaml"),
        )
        .unwrap();
        assert!(
            save_contained_bytes(
                home.path(),
                ".skillator/library.yaml",
                b"version: 1\nlocations: []\n",
                &Fingerprint::Absent
            )
            .is_err()
        );
        assert_eq!(
            fs::read_to_string(outside.path().join("library.yaml")).unwrap(),
            "outside"
        );
    }

    #[test]
    fn history_save_preserves_prior_state_when_live_entry_changes_after_exchange() {
        let home = tempfile::tempdir().unwrap();
        let history = History {
            id: Some(new_id().unwrap()),
            ..History::default()
        };
        history.save(home.path(), &Fingerprint::Absent).unwrap();
        let path = home.path().join(".skillator/rsync/state.json");
        let prior = fs::read(&path).unwrap();
        let mut updated = history.clone();
        updated.peers.insert(new_id().unwrap(), Baseline::default());
        let error = updated
            .save_with(home.path(), &Fingerprint::for_bytes(&prior), || {
                fs::write(&path, "intervening edit").unwrap();
            })
            .unwrap_err();
        assert!(error.to_string().contains("prior version"), "{error}");
        assert_eq!(fs::read(&path).unwrap(), b"intervening edit");
        let stages: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".skillator-rsync-")
            })
            .collect();
        assert_eq!(stages.len(), 1);
        assert_eq!(fs::read(&stages[0]).unwrap(), prior);
    }

    #[test]
    fn history_save_retains_both_entries_when_the_exchanged_backup_changes() {
        let home = tempfile::tempdir().unwrap();
        let history = History {
            id: Some(new_id().unwrap()),
            ..History::default()
        };
        history.save(home.path(), &Fingerprint::Absent).unwrap();
        let path = home.path().join(".skillator/rsync/state.json");
        let prior = fs::read(&path).unwrap();
        let mut updated = history.clone();
        updated.peers.insert(new_id().unwrap(), Baseline::default());
        let error = updated
            .save_with(home.path(), &Fingerprint::for_bytes(&prior), || {
                let backup = fs::read_dir(path.parent().unwrap())
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .find(|path| {
                        path.file_name()
                            .unwrap()
                            .to_string_lossy()
                            .starts_with(".skillator-rsync-")
                    })
                    .unwrap();
                fs::write(backup, "intervening backup").unwrap();
                fs::write(&path, "intervening live state").unwrap();
            })
            .unwrap_err();
        assert!(error.to_string().contains("preserve the exchanged entries"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "intervening live state");
        let backups: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".skillator-rsync-")
            })
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(
            fs::read_to_string(&backups[0]).unwrap(),
            "intervening backup"
        );
    }

    #[test]
    fn linked_parents_cannot_escape_home() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), home.path().join("escape")).unwrap();
        assert!(contained(home.path(), "escape/skill/file").is_err());
        for value in ["../file", "/file", "", "a/../b"] {
            assert!(contained(home.path(), value).is_err());
        }
        assert!(contained(home.path(), "new/skill/file").is_ok());
    }
    #[test]
    fn obstructed_observation_preserves_strict_write_containment() {
        let home = tempfile::tempdir().unwrap();
        fs::write(home.path().join("file"), "obstruction").unwrap();
        for path in ["file/child", "file/sub/deeper/child"] {
            assert!(observation_path(home.path(), path).unwrap().is_none());
            assert!(contained(home.path(), path).is_err());
        }
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("file"), "outside").unwrap();
        std::os::unix::fs::symlink(outside.path().join("file"), home.path().join("escape"))
            .unwrap();
        assert!(observation_path(home.path(), "escape/sub/child").is_err());
        assert_eq!(
            fs::read_to_string(outside.path().join("file")).unwrap(),
            "outside"
        );
    }
}
