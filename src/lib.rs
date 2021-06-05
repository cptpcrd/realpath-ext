#![cfg_attr(not(feature = "std"), no_std)]

mod slicevec;
mod util;

use slicevec::SliceVec;
use util::{ComponentIter, ComponentStack, SymlinkCounter};

/// "Normalize" the given path.
///
/// This is a wrapper around [`normpath_raw()`] that allocates a buffer; see that function's
/// documentation for details.
#[cfg(feature = "std")]
pub fn normpath<P: AsRef<std::path::Path>>(path: P) -> std::io::Result<std::path::PathBuf> {
    use std::os::unix::prelude::*;

    let path = path.as_ref().as_os_str().as_bytes();

    let mut buf = vec![0; path.len()];

    let len = normpath_raw(path, &mut buf).map_err(std::io::Error::from_raw_os_error)?;
    buf.truncate(len);

    Ok(std::ffi::OsString::from_vec(buf).into())
}

/// "Normalize" the given path.
///
/// Other than the differences described below, the `path` and `buf` arguments to this function,
/// and the return values, have the same meaning as for [`realpath_raw()`].
///
/// This function was designed after Python's `os.path.normpath()`. It will remove `.` elements,
/// condense extra slashes, and collapse `..` entries. Think of it as a version of
/// [`realpath_raw()`] that doesn't actually touch the filesystem. (As a consequence of this, if
/// the given `path` is relative, the returned path will also be relative.)
///
/// Note that because this function doesn't actually touch the filesystem, the returned path may
/// not refer to the correct file! Certain combinations of `..` and/or symbolic links can cause
/// this; the only way to get the definitive canonicalized path is to use [`realpath_raw()`].
///
/// Example usage:
///
/// ```
/// # use realpath_ext::normpath_raw;
/// let mut buf = [0; libc::PATH_MAX as usize];
/// let n = normpath_raw(b"/a/b/./c/../", &mut buf).unwrap();
/// assert_eq!(&buf[..n], b"/a/b");
/// ```
///
/// # Errors
///
/// This function may fail with the following errors:
///
/// - `ENAMETOOLONG`: The given `buf` is not long enough to store the normalized path.
/// - `ENOENT`: The given `path` is empty.
/// - `EINVAL`: The given `path` contains a NUL byte (not allowed in \*nix paths).
pub fn normpath_raw(path: &[u8], buf: &mut [u8]) -> Result<usize, i32> {
    let mut buf = SliceVec::empty(buf);

    for component in ComponentIter::new(path)? {
        if component == b"/" || component == b"//" {
            buf.replace(component)?;
        } else if component == b".." {
            buf.make_parent_path()?;
        } else {
            if !matches!(buf.as_ref(), b"/" | b"//" | b"") {
                buf.push(b'/')?;
            }
            buf.extend_from_slice(component)?;
        }
    }

    if buf.is_empty() {
        buf.push(b'.')?;
    }

    Ok(buf.len())
}

bitflags::bitflags! {
    /// Flags that modify path resolution.
    ///
    /// These flags were modeled after the options to the GNU `realpath` program.
    pub struct RealpathFlags: u32 {
        /// Allow any component of the given path to be missing, inaccessible, or not a directory
        /// when it should be.
        const ALLOW_MISSING = 0x01;
        /// Allow the last component of the given path to be missing.
        const ALLOW_LAST_MISSING = 0x02;
        /// Do not resolve symbolic links as they are encountered.
        ///
        /// Note that if this option is passed, the returned path may not refer to the correct file!
        /// Certain combinations of `..` and/or symbolic links can cause this.
        const IGNORE_SYMLINKS = 0x04;
    }
}

/// Canonicalize the given path.
///
/// This is a wrapper around [`realpath_raw()`] that allocates a buffer; see that function's
/// documentation for details.
#[cfg(feature = "std")]
pub fn realpath<P: AsRef<std::path::Path>>(
    path: P,
    flags: RealpathFlags,
) -> std::io::Result<std::path::PathBuf> {
    use std::os::unix::prelude::*;

    let mut buf = vec![0; libc::PATH_MAX as usize];

    let len = realpath_raw(path.as_ref().as_os_str().as_bytes(), &mut buf, flags)
        .map_err(std::io::Error::from_raw_os_error)?;
    buf.truncate(len);

    Ok(std::ffi::OsString::from_vec(buf).into())
}

/// Canonicalize the given path.
///
/// This function resolves the path specified by `path`, storing the result in `buf`. On success,
/// the length of the resolved path is returned; on error, an OS error code is returned.
///
/// If `flags` is specified as `RealpathFlags::empty()`, this is roughly equivalent to the libc's
/// `realpath()`. Otherwise, the given `flags` modify aspects of path resolution.
///
/// This function does not allocate any memory. It will only call the following C functions:
/// - `sysconf(_SC_SYMLOOP_MAX)`
/// - `readlink()`
/// - `stat()` (only if it needs to be verified that the path is a directory)
/// - `getcwd()` (only if the given `path` is relative and does not contain a reference to an
///   absolute symbolic link)
///
/// Example usage:
///
/// ```
/// # use realpath_ext::{RealpathFlags, realpath_raw};
/// let mut buf = [0; libc::PATH_MAX as usize];
/// let n = realpath_raw(b"///", &mut buf, RealpathFlags::empty()).unwrap();
/// assert_eq!(&buf[..n], b"/");
/// ```
///
/// The returned path will ALWAYS be absolute.
///
/// # Errors
///
/// This function may fail with the following errors:
///
/// - `ENAMETOOLONG`: Either:
///    1. The given `buf` is not long enough to store the canonicalized path, or
///    2. The current working directory cannot be represented in a buffer of length `PATH_MAX`, or
///    3. An intermediate result created by combining any symbolic link paths exceeded the system
///       `PATH_MAX`. (Note that the actual limit is slightly higher than `PATH_MAX` to account for
///       storage overhead; this should not be relied upon.)
/// - `EINVAL`: The given `path` contains a NUL byte (not allowed in \*nix paths).
/// - `ELOOP`: Too many symbolic links were encounted during resolution.
///
///   This function will use `sysconf()` to check the system's `SYMLOOP_MAX` value to determine
///   the limit. If that fails (for example, it always fails on glibc), this function will fall
///   back on a limit of 40 (which is Linux's limit).
/// - `ENOENT`/`EACCES`/`ENOTDIR`: The given `path` (or a component of it) does not exist, is
///   inaccessible, or is not a directory (respectively).
///
///   `ENOENT` and `EACCES` may also be returned if `getcwd()` had to be called (see above for the
///   conditions in which this may be necessary) and the path to the current directory cannot be
///   obtained.
///
///   (Note that these errors may be ignored depending on the specified `flags`.)
/// - `EIO`: An I/O error occurred while interacting with the filesystem.
pub fn realpath_raw(path: &[u8], buf: &mut [u8], flags: RealpathFlags) -> Result<usize, i32> {
    let mut stack = [0; libc::PATH_MAX as usize + 100];
    let mut stack = ComponentStack::new(&mut stack);

    let mut path_it = ComponentIter::new(path)?;

    let mut buf = SliceVec::empty(buf);

    let mut links = SymlinkCounter::new();

    while let Some(component) = stack.next().or_else(|| path_it.next()) {
        debug_assert_ne!(buf.as_ref(), b".");

        if component == b"/" || component == b"//" {
            buf.replace(component)?;
        } else if component == b".." {
            buf.make_parent_path()?;
        } else {
            let oldlen = buf.len();

            if !matches!(buf.as_ref(), b"/" | b"//" | b"") {
                buf.push(b'/')?;
            }
            buf.extend_from_slice(component)?;
            buf.push(b'\0')?;

            let res = if flags.contains(RealpathFlags::IGNORE_SYMLINKS) {
                // If IGNORE_SYMLINKS was passed, call readlink() to make sure it exists, but then
                // act like it isn't a symlink if it is
                Err(unsafe { util::readlink_empty(buf.as_ptr()) }
                    .err()
                    .unwrap_or(libc::EINVAL))
            } else {
                unsafe { stack.push_readlink(buf.as_ptr()) }
            };

            match res {
                Ok(()) => {
                    links.advance()?;
                    buf.set_len(oldlen);
                }

                // Not a symlink; just remove the trailing NUL
                Err(libc::EINVAL) => {
                    buf.pop();
                }

                // In these conditions, components of the path are allowed to not exist/not be
                // accessible/not be a directory
                Err(libc::ENOENT) | Err(libc::EACCES) | Err(libc::ENOTDIR)
                    if flags.contains(RealpathFlags::ALLOW_MISSING) =>
                {
                    buf.pop();
                }

                Err(libc::ENOENT)
                    if flags.contains(RealpathFlags::ALLOW_LAST_MISSING)
                        && stack.is_empty()
                        && path_it.is_empty() =>
                {
                    buf.pop();
                }

                Err(eno) => return Err(eno),
            }
        }
    }

    /// If required, check that `buf` refers to a directory.
    fn maybe_check_isdir(path: &[u8], buf: &mut SliceVec, flags: RealpathFlags) -> Result<(), i32> {
        if (path.ends_with(b"/") || path.ends_with(b"/."))
            && !flags.contains(RealpathFlags::ALLOW_MISSING)
        {
            buf.push(b'\0')?;
            match unsafe { util::check_isdir(buf.as_ptr()) } {
                Ok(()) => (),
                Err(libc::ENOENT) if flags.contains(RealpathFlags::ALLOW_LAST_MISSING) => (),
                Err(eno) => return Err(eno),
            }
            buf.pop();
        }

        Ok(())
    }

    let mut tmp = SliceVec::empty(stack.clear());

    if buf.as_ref() == b"" {
        util::getcwd(&mut buf)?;
        // We know `buf` refers to a directory
    } else if buf.as_ref() == b".." {
        util::getcwd(&mut buf)?;
        buf.make_parent_path()?;
        // We know `buf` refers to a directory
    } else if buf.starts_with(b"../") {
        let mut n = count_leading_dotdot(&buf);
        if &buf[(n * 3)..] == b".." {
            buf.clear();
            n += 1;
            // We know `buf` refers to a directory
        } else {
            buf.remove_range(0..(n * 3 - 1));
            maybe_check_isdir(path, &mut buf, flags)?;
        }

        tmp.clear();
        util::getcwd(&mut tmp)?;

        for _ in 0..n {
            tmp.make_parent_path()?;
        }

        buf.insert_from_slice(0, &tmp)?;
    } else if !buf.starts_with(b"/") {
        debug_assert!(!buf.starts_with(b"./"));

        maybe_check_isdir(path, &mut buf, flags)?;

        tmp.clear();
        util::getcwd(&mut tmp)?;
        debug_assert!(tmp.len() > 0);
        tmp.push(b'/')?;
        buf.insert_from_slice(0, &tmp)?;
    } else if !matches!(buf.as_ref(), b"/" | b"//") {
        // We don't have to check "/" or "//", but we do have to check other paths
        maybe_check_isdir(path, &mut buf, flags)?;
    }

    Ok(buf.len())
}

fn count_leading_dotdot(mut s: &[u8]) -> usize {
    let mut n = 0;
    while s.starts_with(b"../") {
        n += 1;
        s = &s[3..];
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_leading_dotdot() {
        assert_eq!(count_leading_dotdot(b""), 0);
        assert_eq!(count_leading_dotdot(b".."), 0);
        assert_eq!(count_leading_dotdot(b"../a"), 1);
        assert_eq!(count_leading_dotdot(b"../../a"), 2);
        assert_eq!(count_leading_dotdot(b"../a/../b"), 1);
    }

    #[test]
    fn test_normpath_raw() {
        let mut buf = [0; 100];

        let n = normpath_raw(b"/", &mut buf).unwrap();
        assert_eq!(&buf[..n], b"/");

        let n = normpath_raw(b".", &mut buf).unwrap();
        assert_eq!(&buf[..n], b".");

        let n = normpath_raw(b"a/..", &mut buf).unwrap();
        assert_eq!(&buf[..n], b".");

        let n = normpath_raw(b"//a/./b/../c/", &mut buf).unwrap();
        assert_eq!(&buf[..n], b"//a/c");

        assert_eq!(normpath_raw(b"", &mut buf).unwrap_err(), libc::ENOENT);
        assert_eq!(normpath_raw(b"\0", &mut buf).unwrap_err(), libc::EINVAL);
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_normpath() {
        assert_eq!(normpath("/").unwrap().as_os_str(), "/");
        assert_eq!(normpath(".").unwrap().as_os_str(), ".");
        assert_eq!(normpath("a/..").unwrap().as_os_str(), ".");
        assert_eq!(normpath("//a/./b/../c/").unwrap().as_os_str(), "//a/c");

        assert_eq!(normpath("").unwrap_err().raw_os_error(), Some(libc::ENOENT));
        assert_eq!(
            normpath("\0").unwrap_err().raw_os_error(),
            Some(libc::EINVAL)
        );
    }
}
