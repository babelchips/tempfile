use std::env;
use std::ffi::{CString, OsStr};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;
use util;

#[cfg(not(target_os = "redox"))]
use libc::{c_char, c_int, link, rename, unlink};

#[cfg(not(target_os = "redox"))]
#[inline(always)]
pub fn cvt_err(result: c_int) -> io::Result<c_int> {
    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(result)
    }
}

#[cfg(target_os = "redox")]
#[inline(always)]
pub fn cvt_err(result: Result<usize, syscall::Error>) -> io::Result<usize> {
    result.map_err(|err| io::Error::from_raw_os_error(err.errno))
}

// Stolen from std.
pub fn cstr(path: &Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contained a null"))
}

pub fn create_named(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

fn create_unlinked(path: &Path) -> io::Result<File> {
    let tmp;
    // shadow this to decrease the lifetime. It can't live longer than `tmp`.
    let mut path = path;
    if !path.is_absolute() {
        let cur_dir = env::current_dir()?;
        tmp = cur_dir.join(path);
        path = &tmp;
    }

    let f = create_named(path)?;
    // don't care whether the path has already been unlinked,
    // but perhaps there are some IO error conditions we should send up?
    let _ = fs::remove_file(path);
    Ok(f)
}

#[cfg(target_os = "linux")]
pub fn create(dir: &Path) -> io::Result<File> {
    use libc::{open64, O_CLOEXEC, O_EXCL, O_RDWR, O_TMPFILE};
    use std::os::unix::io::FromRawFd;
    match unsafe {
        let path = cstr(dir)?;
        open64(
            path.as_ptr() as *const c_char,
            O_CLOEXEC | O_EXCL | O_TMPFILE | O_RDWR,
            0o600,
        )
    } {
        -1 => create_unix(dir),
        fd => Ok(unsafe { FromRawFd::from_raw_fd(fd) }),
    }
}

#[cfg(not(target_os = "linux"))]
pub fn create(dir: &Path) -> io::Result<File> {
    create_unix(dir)
}

fn create_unix(dir: &Path) -> io::Result<File> {
    util::create_helper(
        dir,
        OsStr::new(".tmp"),
        OsStr::new(""),
        ::NUM_RAND_CHARS,
        |path| create_unlinked(&path),
    )
}

pub fn reopen(file: &File, path: &Path) -> io::Result<File> {
    let new_file = OpenOptions::new().read(true).write(true).open(path)?;
    let old_meta = file.metadata()?;
    let new_meta = new_file.metadata()?;
    if old_meta.dev() != new_meta.dev() || old_meta.ino() != new_meta.ino() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "original tempfile has been replaced",
        ));
    }
    Ok(new_file)
}

#[cfg(not(target_os = "redox"))]
pub fn persist(old_path: &Path, new_path: &Path, overwrite: bool) -> io::Result<()> {
    unsafe {
        let old_path = cstr(old_path)?;
        let new_path = cstr(new_path)?;
        if overwrite {
            cvt_err(rename(
                old_path.as_ptr() as *const c_char,
                new_path.as_ptr() as *const c_char,
            ))?;
        } else {
            cvt_err(link(
                old_path.as_ptr() as *const c_char,
                new_path.as_ptr() as *const c_char,
            ))?;
            // Ignore unlink errors. Can we do better?
            // On recent linux, we can use renameat2 to do this atomically.
            let _ = unlink(old_path.as_ptr() as *const c_char);
        }
        Ok(())
    }
}

#[cfg(target_os = "redox")]
pub fn persist(old_path: &Path, new_path: &Path, overwrite: bool) -> io::Result<()> {
    // XXX implement when possible
    Err(io::Error::from_raw_os_error(syscall::ENOSYS))
}
