//! Private bounded backup containers and identity checks.
use super::*;

pub(super) fn unique_artifact(parent: &Path, kind: &str) -> PathBuf {
    let sequence = OPERATION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(
        ".skillator-{kind}-{}-{sequence}",
        std::process::id()
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ContainerIdentity {
    pub(super) device: u64,
    pub(super) inode: u64,
}

impl ContainerIdentity {
    pub(super) fn read(path: &Path) -> std::io::Result<Self> {
        use std::os::unix::fs::MetadataExt;
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(std::io::Error::other(
                "backup container is not a physical directory",
            ));
        }
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }

    pub(super) fn contents(self, path: &Path) -> std::io::Result<Option<PathBuf>> {
        if Self::read(path)? != self {
            return Err(std::io::Error::other("backup container changed"));
        }
        let mut entries = fs::read_dir(path)?;
        let payload = entries.next().transpose()?.map(|entry| entry.path());
        if entries.next().transpose()?.is_some() || Self::read(path)? != self {
            return Err(std::io::Error::other(
                "backup container changed or has multiple entries",
            ));
        }
        Ok(payload)
    }

    pub(super) fn remove_empty(self, path: &Path) -> std::io::Result<()> {
        if self.contents(path)?.is_some() {
            return Err(std::io::Error::other("backup container is not empty"));
        }
        fs::remove_dir(path)
    }
}

#[derive(Debug, Clone)]
pub(super) struct BackupArtifact {
    pub(super) container: PathBuf,
    pub(super) payload: PathBuf,
    pub(super) identity: ContainerIdentity,
}

impl BackupArtifact {
    pub(super) fn create(parent: &Path, destination: &Path) -> Result<Self, ApplyFailure> {
        use std::os::unix::fs::DirBuilderExt;
        let basename = destination
            .file_name()
            .ok_or_else(|| ApplyFailure::Failed("destination has no basename".to_owned()))?;
        loop {
            let container = unique_artifact(parent, "backup");
            match fs::DirBuilder::new().mode(0o700).create(&container) {
                Ok(()) => {
                    let identity = ContainerIdentity::read(&container).map_err(failed)?;
                    return Ok(Self {
                        payload: container.join(basename),
                        container,
                        identity,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(failed(error)),
            }
        }
    }

    pub(super) fn matches(&self, expected: &EntryFingerprint) -> bool {
        self.identity
            .contents(&self.container)
            .ok()
            .flatten()
            .as_ref()
            == Some(&self.payload)
            && &fingerprint(&self.payload) == expected
            && self
                .identity
                .contents(&self.container)
                .ok()
                .flatten()
                .as_ref()
                == Some(&self.payload)
    }

    pub(super) fn remove_empty(&self) -> std::io::Result<()> {
        self.identity.remove_empty(&self.container)
    }

    pub(super) fn cleanup_empty(&self) -> Result<(), ApplyFailure> {
        self.remove_empty().map_err(|error| {
            ApplyFailure::RecoveryRequired(format!(
                "An empty backup folder could not be removed at {}: {error}",
                self.container.display()
            ))
        })
    }

    pub(super) fn capture(&self, source: &Path) -> Result<(), ApplyFailure> {
        if self
            .identity
            .contents(&self.container)
            .map_err(failed)?
            .is_some()
        {
            return Err(ApplyFailure::Changed);
        }
        if let Err(error) = rename_noreplace(source, &self.payload) {
            self.cleanup_empty()?;
            return Err(failed(error));
        }
        Ok(())
    }

    pub(super) fn remove_payload(&self) -> std::io::Result<()> {
        if self.identity.contents(&self.container)?.as_ref() != Some(&self.payload) {
            return Err(std::io::Error::other("backup container changed"));
        }
        remove_any(&self.payload)
    }

    pub(super) fn restore(&self, destination: &Path) -> std::io::Result<()> {
        if self.identity.contents(&self.container)?.as_ref() != Some(&self.payload) {
            return Err(std::io::Error::other("backup container changed"));
        }
        rename_noreplace(&self.payload, destination)
    }
}
