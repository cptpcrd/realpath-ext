#![cfg_attr(not(feature = "std"), no_std)]

mod slicevec;
mod util;

use slicevec::SliceVec;
use util::{ComponentStack, SymlinkCounter};

#[cfg(feature = "std")]
pub fn realpath<P: AsRef<std::path::Path>>(path: P) -> std::io::Result<std::path::PathBuf> {
    use std::os::unix::prelude::*;

    let mut buf = util::zeroed_vec(libc::PATH_MAX as usize);

    let len = realpath_raw(path.as_ref().as_os_str().as_bytes(), &mut buf)
        .map_err(std::io::Error::from_raw_os_error)?;
    buf.truncate(len);

    Ok(std::ffi::OsString::from_vec(buf).into())
}

pub fn realpath_raw(path: &[u8], buf: &mut [u8]) -> Result<usize, i32> {
    let mut stack = [0; libc::PATH_MAX as usize + 100];
    let mut stack = ComponentStack::new(&mut stack);
    stack.push(path)?;

    let mut buf = SliceVec::empty(buf);
    buf.push(b'.')?;

    let mut links = SymlinkCounter::new();

    while let Some(component) = stack.next() {
        debug_assert!(!buf.is_empty());

        if component == b"/" {
            buf.replace(b"/")?;
        } else if component == b".." {
            buf.make_parent_path()?;
        } else {
            let oldlen = buf.len();

            if buf.as_ref() != b"/" {
                buf.push(b'/')?;
            }
            buf.extend_from_slice(component)?;
            buf.push(b'\0')?;

            match unsafe { stack.push_readlink(buf.as_ptr()) } {
                Ok(()) => {
                    links.advance()?;
                    buf.set_len(oldlen);
                }

                // Not a symlink; just remove the trailing NUL
                Err(libc::EINVAL) => {
                    buf.pop();
                }

                Err(eno) => return Err(eno),
            }
        }
    }

    let mut isdir = false;

    let mut tmp = SliceVec::empty(stack.clear());

    if buf.as_ref() == b"." {
        util::getcwd(&mut buf)?;
        isdir = true;
    } else if buf.as_ref() == b".." {
        util::getcwd(&mut buf)?;
        buf.make_parent_path()?;
        isdir = true;
    } else if buf.starts_with(b"../") {
        tmp.clear();
        util::getcwd(&mut tmp)?;

        let mut n = count_leading_dotdot(&buf);
        if &buf[(n * 3)..] == b".." {
            buf.clear();
            n += 1;
            isdir = true;
        } else {
            buf.remove_range(0..(n * 3 - 1));
        }

        for _ in 0..n {
            tmp.make_parent_path()?;
        }

        buf.insert_from_slice(0, &tmp)?;
    } else if !buf.starts_with(b"/") {
        if buf.starts_with(b"./") {
            buf.remove_range(0..1);
        }

        tmp.clear();
        util::getcwd(&mut tmp)?;
        debug_assert!(tmp.len() > 0);
        buf.insert_from_slice(0, &tmp)?;
    }

    // If a) we haven't proven it's a directory, b) the original path ended with a slash (or `/.`),
    // and c) the path didn't resolve to "/", then we need to check if it's a directory.
    if !isdir && (path.ends_with(b"/") || path.ends_with(b"/.")) && buf.as_ref() != b"/" {
        buf.push(b'\0')?;
        unsafe {
            util::check_isdir(buf.as_ptr())?;
        }
        buf.pop();
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
