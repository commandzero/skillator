//! Atomic conditional configuration publication.

use super::*;

pub(super) fn conditional_save(
    path: &Path,
    bytes: &[u8],
    expected: &Fingerprint,
) -> Result<Fingerprint, SaveError> {
    conditional_save_with(path, bytes, expected, || {})
}

pub(super) fn conditional_save_with(
    path: &Path,
    bytes: &[u8],
    expected: &Fingerprint,
    before_publish: impl FnOnce(),
) -> Result<Fingerprint, SaveError> {
    let current = current_fingerprint(path)?;
    if &current != expected {
        return Err(SaveError::Stale);
    }
    let desired = Fingerprint::for_bytes(bytes);
    if current == desired {
        return Ok(desired);
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut stage = ConfigurationStage {
        destination: path,
        path: stage_path(parent),
        parent,
        expected,
        contains_prior: false,
    };
    let result = stage.publish(bytes, before_publish).map(|()| desired);
    if result.is_err() && !stage.contains_prior {
        let _ = fs::remove_file(&stage.path);
    }
    result
}

fn current_fingerprint(path: &Path) -> Result<Fingerprint, SaveError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Fingerprint::for_bytes(&bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Fingerprint::Absent),
        Err(error) => Err(error.into()),
    }
}

struct ConfigurationStage<'a> {
    destination: &'a Path,
    path: PathBuf,
    parent: &'a Path,
    expected: &'a Fingerprint,
    contains_prior: bool,
}

impl ConfigurationStage<'_> {
    fn publish(&mut self, bytes: &[u8], before_publish: impl FnOnce()) -> Result<(), SaveError> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        if &current_fingerprint(self.destination)? != self.expected {
            return Err(SaveError::Stale);
        }
        before_publish();
        if *self.expected == Fingerprint::Absent {
            self.publish_new()
        } else {
            self.replace_existing()
        }
    }

    fn publish_new(&self) -> Result<(), SaveError> {
        rename_noreplace(&self.path, self.destination).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                SaveError::Stale
            } else {
                SaveError::Io(error)
            }
        })?;
        sync_directory(self.parent)?;
        Ok(())
    }

    fn replace_existing(&mut self) -> Result<(), SaveError> {
        rename_exchange(&self.path, self.destination)?;
        self.contains_prior = true;
        sync_directory(self.parent)?;
        let moved = fs::read(&self.path)
            .map(|bytes| Fingerprint::for_bytes(&bytes))
            .map_err(SaveError::Io)?;
        if &moved != self.expected {
            return self.restore_concurrent_change();
        }
        fs::remove_file(&self.path).map_err(|error| {
            SaveError::Io(std::io::Error::other(format!(
                "configuration was saved but prior content remains at {}: {error}",
                self.path.display()
            )))
        })?;
        self.contains_prior = false;
        sync_directory(self.parent)?;
        Ok(())
    }

    fn restore_concurrent_change(&mut self) -> Result<(), SaveError> {
        if rename_exchange(&self.path, self.destination).is_ok() {
            self.contains_prior = false;
            let _ = sync_directory(self.parent);
            Err(SaveError::Stale)
        } else {
            Err(SaveError::Io(std::io::Error::other(format!(
                "Configuration changed during saving and could not be restored; recover it from {}",
                self.path.display()
            ))))
        }
    }
}

pub(super) fn stage_path(parent: &Path) -> PathBuf {
    let sequence = STAGE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(
        ".skillator-config-stage-{}-{sequence}",
        std::process::id()
    ))
}

pub(super) fn sync_directory(path: &Path) -> Result<(), std::io::Error> {
    OpenOptions::new().read(true).open(path)?.sync_all()
}
