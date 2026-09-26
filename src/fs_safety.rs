//! Platform seam for publishing without overwriting a concurrently created path.

use std::ffi::{CString, OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Component, Path};
use std::process::Command;

/// Bind a child process to an already opened directory inode, independent of
/// later changes to the path used to open it.
pub(crate) fn bind_directory(command: &mut Command, directory: &File) {
    let descriptor = directory.as_raw_fd();
    // SAFETY: fchdir is async-signal-safe in the forked child. The File stays
    // open until the subprocess exits and the child retains its cwd after exec.
    unsafe {
        command.pre_exec(move || {
            if libc::fchdir(descriptor) == 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        });
    }
}

pub(crate) fn rename_noreplace(source: &Path, destination: &Path) -> io::Result<()> {
    rename_with_mode(
        libc::AT_FDCWD,
        source,
        libc::AT_FDCWD,
        destination,
        RenameMode::NoReplace,
    )
}

pub(crate) fn rename_exchange(left: &Path, right: &Path) -> io::Result<()> {
    rename_with_mode(
        libc::AT_FDCWD,
        left,
        libc::AT_FDCWD,
        right,
        RenameMode::Exchange,
    )
}

/// A physical directory beneath home, held by inode while its pathname may change.
pub(crate) struct Directory(File);

impl Directory {
    pub(crate) fn open_parent(home: &Path, parent: &Path) -> io::Result<Self> {
        Self::open_beneath(home, parent, true)
    }

    pub(crate) fn open_existing_parent(home: &Path, parent: &Path) -> io::Result<Self> {
        Self::open_beneath(home, parent, false)
    }

    fn open_beneath(home: &Path, parent: &Path, create: bool) -> io::Result<Self> {
        let home = home.canonicalize()?;
        let relative = parent.strip_prefix(&home).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "destination parent escapes home",
            )
        })?;
        let mut options = OpenOptions::new();
        options
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
        let mut directory = Self(options.open(&home)?);
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid directory component",
                ));
            };
            if create {
                let name = c_name(name)?;
                // SAFETY: The descriptor and NUL-terminated component are valid for this call.
                let created =
                    unsafe { libc::mkdirat(directory.0.as_raw_fd(), name.as_ptr(), 0o755) };
                if created != 0 && io::Error::last_os_error().kind() != io::ErrorKind::AlreadyExists
                {
                    return Err(io::Error::last_os_error());
                }
            }
            directory = directory.open_dir(name)?;
        }
        Ok(directory)
    }

    pub(crate) fn open_dir(&self, name: &OsStr) -> io::Result<Self> {
        let name = c_name(name)?;
        // SAFETY: openat receives a live directory descriptor and a valid C string.
        let fd = unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            // SAFETY: openat returned a new descriptor owned by this File.
            Ok(Self(unsafe { File::from_raw_fd(fd) }))
        }
    }

    pub(crate) fn create_dir(&self, name: &OsStr) -> io::Result<()> {
        let name = c_name(name)?;
        // SAFETY: The descriptor and C string are valid for this call.
        if unsafe { libc::mkdirat(self.0.as_raw_fd(), name.as_ptr(), 0o755) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub(crate) fn create_file(&self, name: &OsStr) -> io::Result<File> {
        let name = c_name(name)?;
        // SAFETY: openat receives a live descriptor and a valid C string.
        let fd = unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o644,
            )
        };
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            // SAFETY: openat returned a new descriptor owned by this File.
            Ok(unsafe { File::from_raw_fd(fd) })
        }
    }

    pub(crate) fn open_lock(&self, name: &OsStr) -> io::Result<File> {
        let name = c_name(name)?;
        // SAFETY: openat receives a live directory descriptor and valid name.
        let fd = unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDWR | libc::O_CREAT | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o644,
            )
        };
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            // SAFETY: openat returned a new descriptor owned by this File.
            Ok(unsafe { File::from_raw_fd(fd) })
        }
    }

    pub(crate) fn identity(&self) -> io::Result<(u64, u64)> {
        use std::os::unix::fs::MetadataExt;
        let metadata = self.0.metadata()?;
        Ok((metadata.dev(), metadata.ino()))
    }

    pub(crate) fn as_file(&self) -> &File {
        &self.0
    }

    pub(crate) fn open_file(&self, name: &OsStr) -> io::Result<File> {
        let name = c_name(name)?;
        // SAFETY: openat receives a live descriptor and a valid C string.
        let fd = unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            // SAFETY: openat returned a new descriptor owned by this File.
            let file = unsafe { File::from_raw_fd(fd) };
            if !file.metadata()?.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "entry is not a regular file",
                ));
            }
            Ok(file)
        }
    }

    pub(crate) fn symlink(&self, name: &OsStr, target: &OsStr) -> io::Result<()> {
        let name = c_name(name)?;
        let target = CString::new(target.as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "link target contains NUL"))?;
        // SAFETY: Both C strings and the directory descriptor remain valid.
        if unsafe { libc::symlinkat(target.as_ptr(), self.0.as_raw_fd(), name.as_ptr()) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub(crate) fn metadata(&self, name: &OsStr) -> io::Result<libc::stat> {
        let name = c_name(name)?;
        let mut stat = std::mem::MaybeUninit::uninit();
        // SAFETY: fstatat initializes stat on success; the descriptor and name are valid.
        if unsafe {
            libc::fstatat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } == 0
        {
            // SAFETY: fstatat succeeded and initialized the structure.
            Ok(unsafe { stat.assume_init() })
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub(crate) fn read_link(&self, name: &OsStr) -> io::Result<OsString> {
        let name = c_name(name)?;
        let mut buffer = vec![0_u8; 256];
        loop {
            // SAFETY: readlinkat writes at most buffer.len() bytes to its valid pointer.
            let size = unsafe {
                libc::readlinkat(
                    self.0.as_raw_fd(),
                    name.as_ptr(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                )
            };
            if size < 0 {
                return Err(io::Error::last_os_error());
            }
            if size as usize == buffer.len() {
                buffer.resize(buffer.len() * 2, 0);
                continue;
            }
            buffer.truncate(size as usize);
            return Ok(OsString::from_vec(buffer));
        }
    }

    pub(crate) fn is_empty_dir(&self, name: &OsStr) -> io::Result<bool> {
        let directory = self.open_dir(name)?;
        Ok(directory.entries()?.is_empty())
    }

    pub(crate) fn entries(&self) -> io::Result<Vec<OsString>> {
        // SAFETY: openat creates an independent directory stream offset while
        // remaining anchored to this inode. fdopendir takes its descriptor.
        let duplicate = unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                c".".as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        };
        if duplicate < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fdopendir takes ownership of the duplicated descriptor.
        let stream = unsafe { libc::fdopendir(duplicate) };
        if stream.is_null() {
            // SAFETY: fdopendir failed and did not consume duplicate.
            unsafe { libc::close(duplicate) };
            return Err(io::Error::last_os_error());
        }
        let mut names = Vec::new();
        loop {
            #[cfg(target_os = "macos")]
            // SAFETY: __error returns this thread's writable errno location.
            unsafe {
                *libc::__error() = 0
            };
            #[cfg(target_os = "linux")]
            // SAFETY: __errno_location returns this thread's writable errno location.
            unsafe {
                *libc::__errno_location() = 0
            };
            // SAFETY: stream is open until closed below.
            let entry = unsafe { libc::readdir(stream) };
            if entry.is_null() {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(0) {
                    // SAFETY: stream was opened by fdopendir and is still live.
                    unsafe { libc::closedir(stream) };
                    return Err(error);
                }
                break;
            }
            // SAFETY: d_name is NUL-terminated within the returned dirent.
            let name = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
            if name != b"." && name != b".." {
                names.push(OsString::from_vec(name.to_vec()));
            }
        }
        // SAFETY: stream was opened by fdopendir and must be closed once.
        unsafe { libc::closedir(stream) };
        Ok(names)
    }

    pub(crate) fn remove_tree(&self, name: &OsStr) -> io::Result<()> {
        let directory = self.open_dir(name)?;
        directory.remove_contents()?;
        self.remove(name)
    }

    fn remove_contents(&self) -> io::Result<()> {
        for name in self.entries()? {
            let stat = self.metadata(&name)?;
            if stat.st_mode & libc::S_IFMT == libc::S_IFDIR {
                self.remove_tree(&name)?;
            } else {
                self.remove(&name)?;
            }
        }
        Ok(())
    }

    pub(crate) fn remove(&self, name: &OsStr) -> io::Result<()> {
        let stat = self.metadata(name)?;
        let flags = if stat.st_mode & libc::S_IFMT == libc::S_IFDIR {
            libc::AT_REMOVEDIR
        } else {
            0
        };
        let name = c_name(name)?;
        // SAFETY: The descriptor and name are valid for unlinkat.
        if unsafe { libc::unlinkat(self.0.as_raw_fd(), name.as_ptr(), flags) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub(crate) fn rename_noreplace(&self, source: &OsStr, destination: &OsStr) -> io::Result<()> {
        rename_with_mode(
            self.0.as_raw_fd(),
            Path::new(source),
            self.0.as_raw_fd(),
            Path::new(destination),
            RenameMode::NoReplace,
        )
    }

    pub(crate) fn rename_exchange(&self, left: &OsStr, right: &OsStr) -> io::Result<()> {
        rename_with_mode(
            self.0.as_raw_fd(),
            Path::new(left),
            self.0.as_raw_fd(),
            Path::new(right),
            RenameMode::Exchange,
        )
    }
}

fn c_name(name: &OsStr) -> io::Result<CString> {
    if name.is_empty() || name.as_bytes().contains(&b'/') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid entry name",
        ));
    }
    CString::new(name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "entry name contains NUL"))
}

enum RenameMode {
    NoReplace,
    Exchange,
}

fn rename_with_mode(
    source_dir: libc::c_int,
    source: &Path,
    destination_dir: libc::c_int,
    destination: &Path,
    mode: RenameMode,
) -> io::Result<()> {
    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "source path contains NUL"))?;
    let destination = CString::new(destination.as_os_str().as_bytes()).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "destination path contains NUL")
    })?;

    #[cfg(target_os = "linux")]
    // SAFETY: Both CStrings own NUL-terminated path bytes and outlive the call.
    // The syscall receives two valid directory descriptors, two path pointers,
    // and a supported rename flag. It retains no pointers after returning.
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            source_dir,
            source.as_ptr(),
            destination_dir,
            destination.as_ptr(),
            match mode {
                RenameMode::NoReplace => libc::RENAME_NOREPLACE,
                RenameMode::Exchange => libc::RENAME_EXCHANGE,
            },
        )
    };

    #[cfg(target_os = "macos")]
    // SAFETY: Both pointers refer to live, NUL-terminated CStrings. renameatx_np
    // only reads these paths during the call, and the flags are its constants.
    let result = unsafe {
        libc::renameatx_np(
            source_dir,
            source.as_ptr(),
            destination_dir,
            destination.as_ptr(),
            match mode {
                RenameMode::NoReplace => libc::RENAME_EXCL,
                RenameMode::Exchange => libc::RENAME_SWAP,
            },
        )
    };

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    compile_error!("skillator MVP supports only macOS and Linux Unix targets");

    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn replaced_regular_file_rejects_special_entries() {
        let home = tempfile::tempdir().unwrap();
        let parent = home.path().canonicalize().unwrap();
        let directory = Directory::open_existing_parent(&parent, &parent).unwrap();
        let path = parent.join("payload");
        std::fs::write(&path, "original").unwrap();
        assert_eq!(
            directory.metadata(OsStr::new("payload")).unwrap().st_mode & libc::S_IFMT,
            libc::S_IFREG
        );
        std::fs::remove_file(&path).unwrap();
        assert!(
            Command::new("mkfifo")
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
        // Keep both ends open so the regression fails rather than hanging with
        // the former blocking implementation.
        let _fifo = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        assert!(directory.open_file(OsStr::new("payload")).is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(directory.open_file(OsStr::new("payload")).is_err());
    }

    #[test]
    fn opened_parent_keeps_publication_inside_home_after_ancestor_swap() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let parent = home.path().canonicalize().unwrap().join("a/b");
        std::fs::create_dir_all(&parent).unwrap();
        let directory = Directory::open_parent(home.path(), &parent).unwrap();
        std::fs::rename(home.path().join("a"), home.path().join("retained")).unwrap();
        std::os::unix::fs::symlink(outside.path(), home.path().join("a")).unwrap();
        directory
            .create_file(OsStr::new("sibling"))
            .unwrap()
            .write_all(b"safe")
            .unwrap();
        directory
            .rename_noreplace(OsStr::new("sibling"), OsStr::new("skill"))
            .unwrap();
        assert_eq!(
            std::fs::read(home.path().join("retained/b/skill")).unwrap(),
            b"safe"
        );
        assert!(std::fs::read_dir(outside.path()).unwrap().next().is_none());
        assert!(Directory::open_parent(home.path(), &parent).is_err());
    }

    #[test]
    fn stage_creation_and_cleanup_follow_the_opened_parent() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let parent = home.path().canonicalize().unwrap().join(".skillator/rsync");
        let directory = Directory::open_parent(home.path(), &parent).unwrap();
        std::fs::rename(home.path().join(".skillator"), home.path().join("retained")).unwrap();
        std::os::unix::fs::symlink(outside.path(), home.path().join(".skillator")).unwrap();
        directory.create_dir(OsStr::new("stage-test")).unwrap();
        let stage = directory.open_dir(OsStr::new("stage-test")).unwrap();
        stage
            .create_file(OsStr::new("data"))
            .unwrap()
            .write_all(b"safe")
            .unwrap();
        assert_eq!(
            std::fs::read(home.path().join("retained/rsync/stage-test/data")).unwrap(),
            b"safe"
        );
        assert!(std::fs::read_dir(outside.path()).unwrap().next().is_none());
        directory.remove_tree(OsStr::new("stage-test")).unwrap();
        assert!(!home.path().join("retained/rsync/stage-test").exists());
        assert!(Directory::open_parent(home.path(), &parent).is_err());
    }
}
