use super::{Error, Result, state};
use crate::fs_safety::Directory;
use std::path::{Path, PathBuf};

/// The expected physical directory for a transfer, independent of a pathname's current target.
#[derive(Debug, Clone)]
pub(super) struct Stage {
    home: PathBuf,
    path: PathBuf,
    identity: (u64, u64),
}

impl Stage {
    pub(super) fn new(home: &Path, path: &Path, identity: (u64, u64)) -> Result<Self> {
        let parent = path
            .parent()
            .ok_or_else(|| Error::input("invalid transfer stage"))?;
        let name = path
            .file_name()
            .ok_or_else(|| Error::input("invalid transfer stage"))?;
        if !home.is_absolute()
            || !path.is_absolute()
            || parent != home.join(".skillator/rsync")
            || !name
                .to_str()
                .and_then(|name| name.strip_prefix("stage-"))
                .is_some_and(state::valid_id)
        {
            return Err(Error::input("invalid transfer stage"));
        }
        Ok(Self {
            home: home.to_path_buf(),
            path: path.to_path_buf(),
            identity,
        })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn identity(&self) -> (u64, u64) {
        self.identity
    }

    pub(super) fn open(&self) -> Result<Directory> {
        let canonical_home = self.home.canonicalize().map_err(Error::input_display)?;
        if self.path.parent() != Some(canonical_home.join(".skillator/rsync").as_path()) {
            return Err(Error::input(
                "synchronization stage changed after Begin; abort and recover",
            ));
        }
        let physical = self.path.canonicalize().map_err(|_| {
            Error::input("synchronization stage changed after Begin; abort and recover")
        })?;
        if physical != self.path || !physical.starts_with(&canonical_home) {
            return Err(Error::input(
                "synchronization stage changed after Begin; abort and recover",
            ));
        }
        let directory = Directory::open_existing_parent(&self.home, &self.path).map_err(|_| {
            Error::input("synchronization stage changed after Begin; abort and recover")
        })?;
        if directory.identity().map_err(Error::input_display)? != self.identity {
            return Err(Error::input(
                "synchronization stage changed after Begin; abort and recover",
            ));
        }
        Ok(directory)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::MetadataExt;

    #[test]
    fn parsing_a_remote_stage_does_not_access_its_filesystem() {
        let local = tempfile::tempdir().unwrap();
        let remote_home = local.path().join("remote-home-not-mounted");
        let remote_stage = remote_home.join(format!(
            ".skillator/rsync/stage-{}",
            state::new_id().unwrap()
        ));
        let descriptor = Stage::new(&remote_home, &remote_stage, (7, 11)).unwrap();
        assert_eq!(descriptor.path(), remote_stage);
        assert!(descriptor.open().is_err());
    }

    #[test]
    fn descriptor_rejects_an_outside_stage_response_and_a_moved_inode() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let id = state::new_id().unwrap();
        let stage = home
            .path()
            .canonicalize()
            .unwrap()
            .join(format!(".skillator/rsync/stage-{id}"));
        fs::create_dir_all(&stage).unwrap();
        let identity = Directory::open_existing_parent(home.path(), &stage)
            .unwrap()
            .identity()
            .unwrap();
        let descriptor =
            Stage::new(&home.path().canonicalize().unwrap(), &stage, identity).unwrap();

        let relocated = outside.path().join(format!("stage-{id}"));
        fs::rename(&stage, &relocated).unwrap();
        assert!(Stage::new(home.path(), &relocated, identity).is_err());
        std::os::unix::fs::symlink(&relocated, &stage).unwrap();
        assert_eq!(
            fs::metadata(&stage).unwrap().ino(),
            identity.1,
            "the stage retained its inode after moving"
        );
        assert!(descriptor.open().is_err());
    }
}
