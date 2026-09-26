//! Platform seam for publishing without overwriting a concurrently created path.

use std::ffi::CString;
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
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
    rename_with_mode(source, destination, RenameMode::NoReplace)
}

pub(crate) fn rename_exchange(left: &Path, right: &Path) -> io::Result<()> {
    rename_with_mode(left, right, RenameMode::Exchange)
}

enum RenameMode {
    NoReplace,
    Exchange,
}

fn rename_with_mode(source: &Path, destination: &Path, mode: RenameMode) -> io::Result<()> {
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
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            match mode {
                RenameMode::NoReplace => libc::RENAME_NOREPLACE,
                RenameMode::Exchange => libc::RENAME_EXCHANGE,
            },
        )
    };

    #[cfg(target_os = "macos")]
    // SAFETY: Both pointers refer to live, NUL-terminated CStrings. renamex_np
    // only reads these paths during the call, and the flags are its constants.
    let result = unsafe {
        libc::renamex_np(
            source.as_ptr(),
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
