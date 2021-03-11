//#![no_std]

use tinyvec::SliceVec;

mod util;

use util::{ComponentIter, SymlinkCounter};

pub fn realpath_raw(path: &[u8], buf: &mut [u8]) -> Result<usize, i32> {
    let mut tmp = [0; libc::PATH_MAX as usize + 1];
    let mut tmp = SliceVec::from_slice_len(&mut tmp, 0);

    let mut buf = SliceVec::from_slice_len(buf, 0);
    util::sv_reserve(&mut buf, 1)?;
    buf.push(b'.');

    let mut links = SymlinkCounter::new();

    realpath_into(path, &mut buf, &mut tmp, &mut links)?;

    let mut isdir = false;

    if buf.as_ref() == b"." {
        buf.clear();
        util::getcwd(&mut buf)?;
        isdir = true;
    } else if buf.as_ref() == b".." {
        buf.clear();
        util::getcwd(&mut buf)?;
        util::sv_parent(&mut buf)?;
        isdir = true;
    } else if buf.starts_with(b"../") {
        tmp.clear();
        util::getcwd(&mut tmp)?;

        let mut n = count_leading_dotdot(&mut buf);
        util::sv_strip_nfront(&mut buf, n * 3);
        if buf.as_ref() == b".." {
            buf.clear();
            n += 1;
            isdir = true;
        }

        for _ in 0..n {
            util::sv_parent(&mut tmp)?;
        }

        util::sv_prepend(&mut buf, &tmp)?;
    } else if !buf.starts_with(b"/") {
        if buf.starts_with(b"./") {
            util::sv_strip_nfront(&mut buf, 1);
        }

        tmp.clear();
        util::getcwd(&mut tmp)?;
        debug_assert!(tmp.len() > 0);
        util::sv_prepend(&mut buf, &tmp)?;
    }

    // If a) we haven't proven it's a directory, b) the original path ended with a slash (or `/.`),
    // and c) the path didn't resolve to "/", then we need to check if it's a directory.
    if !isdir && (path.ends_with(b"/") || path.ends_with(b"/.")) && buf.as_ref() != b"/" {
        util::sv_reserve(&mut buf, 1)?;
        buf.push(b'\0');
        unsafe {
            util::check_isdir(buf.as_ptr())?;
        }
        buf.pop();
    }

    Ok(buf.len())
}

fn realpath_into(
    path: &[u8],
    buf: &mut SliceVec<u8>,
    tmp: &mut SliceVec<u8>,
    links: &mut SymlinkCounter,
) -> Result<(), i32> {
    for component in ComponentIter::new(path)? {
        debug_assert!(!buf.is_empty());

        if component == b"/" {
            buf.truncate(1);
            buf[0] = b'/';
        } else if component == b".." {
            util::sv_parent(buf)?;
        } else {
            let oldlen = buf.len();

            if buf.as_ref() == b"/" {
                util::sv_reserve(buf, component.len() + 1)?;
            } else {
                util::sv_reserve(buf, component.len() + 2)?;
                buf.push(b'/');
            }
            buf.extend_from_slice(component);
            buf.push(b'\0');

            tmp.set_len(tmp.capacity());

            match unsafe { util::readlink(buf.as_ptr(), tmp) } {
                Ok(n) => {
                    links.advance()?;

                    let target = &tmp[..n];
                    if target.first() == Some(&b'/') {
                        buf.truncate(1);
                    }

                    let (target, rest) = tmp.split_at_mut(n);
                    buf.set_len(oldlen);
                    realpath_into(target, buf, &mut SliceVec::from_slice_len(rest, 0), links)?;
                }

                // Not a symlink; just discard the trailing NUL
                Err(libc::EINVAL) => drop(buf.pop()),

                Err(eno) => return Err(eno),
            }
        }
    }

    Ok(())
}

fn count_leading_dotdot(mut s: &[u8]) -> usize {
    let mut n = 0;
    while s.starts_with(b"../") {
        n += 1;
        s = &s[3..];
    }
    n
}
