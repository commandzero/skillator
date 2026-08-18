//! Platform seam for publishing without overwriting a concurrently created path.

use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

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
