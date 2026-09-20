use super::{Error, Result};
use crate::config::{Fingerprint, save_bytes};
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
        let path = contained(home, ".skillator/rsync/state.json")?;
        match read_optional(&path)? {
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
        let path = contained(home, ".skillator/rsync/state.json")?;
        fs::create_dir_all(path.parent().unwrap()).map_err(Error::input_display)?;
        save_bytes(
            &path,
            &serde_json::to_vec(self).map_err(Error::input_display)?,
            expected,
        )
        .map_err(Error::input_display)?;
        Ok(())
    }
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

pub(super) fn fingerprint(path: &Path) -> Result<Fingerprint> {
    Ok(read_optional(path)?
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
