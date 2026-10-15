//! Platform filesystem operations absent from std.

use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

pub(crate) fn rename_noreplace(source: &Path, destination: &Path) -> io::Result<()> {
    rename_with_mode(source, destination, RenameMode::NoReplace)
}

pub(crate) fn rename_exchange(left: &Path, right: &Path) -> io::Result<()> {
    rename_with_mode(left, right, RenameMode::Exchange)
}

pub(crate) fn preserve_symlink_times(path: &Path, metadata: &std::fs::Metadata) -> io::Result<()> {
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "symlink path contains NUL"))?;
    // Native time types vary across Unix architectures.
    #[allow(clippy::unnecessary_cast)]
    let times = [
        libc::timespec {
            tv_sec: metadata.atime() as libc::time_t,
            tv_nsec: metadata.atime_nsec() as libc::c_long,
        },
        libc::timespec {
            tv_sec: metadata.mtime() as libc::time_t,
            tv_nsec: metadata.mtime_nsec() as libc::c_long,
        },
    ];
    // SAFETY: The CString and two initialized timespecs outlive this call.
    // AT_SYMLINK_NOFOLLOW changes the link's timestamps, never its target's.
    // utimensat retains neither pointer.
    let result = unsafe {
        libc::utimensat(
            libc::AT_FDCWD,
            path.as_ptr(),
            times.as_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
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
