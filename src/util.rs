use tinyvec::SliceVec;

#[cfg(any(target_os = "linux", target_os = "dragonfly"))]
pub use libc::__errno_location as errno_ptr;

#[cfg(any(target_os = "freebsd", target_os = "macos"))]
pub use libc::__error as errno_ptr;

#[cfg(any(target_os = "android", target_os = "netbsd", target_os = "openbsd"))]
pub use libc::__errno as errno_ptr;

#[inline]
pub fn errno_get() -> i32 {
    unsafe { *errno_ptr() }
}

#[derive(Debug)]
pub struct SymlinkCounter {
    max: u16,
    cur: u16,
}

impl SymlinkCounter {
    #[inline]
    pub fn new() -> Self {
        let max = match unsafe { libc::sysconf(libc::_SC_SYMLOOP_MAX) } {
            max if (0..=u16::MAX as _).contains(&max) => max as u16,
            _ => 40,
        };

        Self { max, cur: 0 }
    }

    #[inline]
    pub fn advance(&mut self) -> Result<(), i32> {
        if self.cur >= self.max {
            Err(libc::ELOOP)
        } else {
            self.cur += 1;
            Ok(())
        }
    }
}

#[derive(Debug)]
pub struct ComponentIter<'a>(&'a [u8]);

impl<'a> ComponentIter<'a> {
    #[inline]
    pub fn new(path: &'a [u8]) -> Result<Self, i32> {
        if path.is_empty() {
            Err(libc::ENOENT)
        } else if path.contains(&0) {
            Err(libc::EINVAL)
        } else {
            Ok(Self(path))
        }
    }
}

impl<'a> Iterator for ComponentIter<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        match self.0.split_first() {
            // Empty -> nothing left to iterate over
            None => None,

            // Starts with a slash -> trim leading slashes from the new path and return a slash
            Some((&b'/', path)) => {
                self.0 = strip_leading_slashes(path);
                Some(&[b'/'])
            }

            // So it's not empty and it doesn't start with a slash
            _ => loop {
                let component = match self.0.iter().position(|&c| c == b'/') {
                    // We were able to find a slash inside it
                    // Return the part up to the slash, then
                    Some(index) => {
                        let (component, rest) = self.0.split_at(index);
                        self.0 = strip_leading_slashes(rest);
                        component
                    }

                    // Exhausted
                    None if self.0.is_empty() => break None,

                    // No slashes -> only one component left
                    None => core::mem::take(&mut self.0),
                };

                if component != b"." {
                    break Some(component);
                }
            },
        }
    }
}

#[inline]
pub unsafe fn readlink(path: *const u8, buf: &mut [u8]) -> Result<usize, i32> {
    match libc::readlink(
        path as *const _,
        buf.as_mut_ptr() as *mut libc::c_char,
        buf.len(),
    ) {
        -1 => Err(errno_get()),

        len => {
            debug_assert!(len > 0);

            let len = len as usize;

            // POSIX doesn't specify whether or not the returned string is nul-terminated.

            // On OSes other than Linux/macOS/*BSD, it *might* be. Let's check.
            #[cfg(not(any(
                target_os = "linux",
                target_os = "android",
                target_os = "freebsd",
                target_os = "dragonfly",
                target_os = "openbsd",
                target_os = "netbsd",
                target_os = "macos",
                target_os = "ios",
            )))]
            let len = if buf[len - 1] == 0 { len - 1 } else { len };

            debug_assert_ne!(buf[len - 1], 0);

            if len >= buf.len() {
                Err(libc::ENAMETOOLONG)
            } else {
                Ok(len)
            }
        }
    }
}

pub unsafe fn check_isdir(path: *const u8) -> Result<(), i32> {
    let mut buf = core::mem::MaybeUninit::uninit();
    if libc::stat(path as *const _, buf.as_mut_ptr()) < 0 {
        Err(errno_get())
    } else if buf.assume_init().st_mode & libc::S_IFMT == libc::S_IFDIR {
        Ok(())
    } else {
        Err(libc::ENOTDIR)
    }
}

pub fn getcwd(buf: &mut SliceVec<u8>) -> Result<(), i32> {
    let extra = buf.grab_spare_slice_mut();

    if extra.is_empty() {
        Err(libc::ENAMETOOLONG)
    } else if unsafe { libc::getcwd(extra.as_mut_ptr() as *mut _, extra.len()) }.is_null() {
        Err(errno_get())
    } else {
        let index = extra.iter().position(|&ch| ch == 0).unwrap();
        debug_assert!(index > 0);
        buf.set_len(buf.len() + index);
        Ok(())
    }
}

pub fn strip_leading_slashes(mut s: &[u8]) -> &[u8] {
    while let Some((&b'/', rest)) = s.split_first() {
        s = rest;
    }
    s
}

#[inline]
pub fn sv_reserve(s: &SliceVec<u8>, extra: usize) -> Result<(), i32> {
    if s.capacity() - s.len() >= extra {
        Ok(())
    } else {
        Err(libc::ENAMETOOLONG)
    }
}

#[inline]
pub fn sv_setlen(s: &mut SliceVec<u8>, len: usize) -> Result<(), i32> {
    if s.capacity() >= len {
        s.set_len(len);
        Ok(())
    } else {
        Err(libc::ENAMETOOLONG)
    }
}

pub fn sv_parent(s: &mut SliceVec<u8>) -> Result<(), i32> {
    if s.as_ref() == b".." || s.as_ref().ends_with(b"/..") {
        sv_reserve(s, 3)?;
        s.extend_from_slice(b"/..");
        return Ok(());
    } else if s.as_ref() == b"." {
        sv_reserve(s, 1)?;
        s.push(b'.');
        return Ok(());
    }

    match s.iter().rposition(|&ch| ch == b'/') {
        // Only one slash!
        Some(0) => s.truncate(1),

        Some(i) => {
            // It should NOT end with a slash
            debug_assert_ne!(i, s.len() - 1);
            s.truncate(i);
        }

        // No slashes!
        None => {
            debug_assert!(!matches!(s.as_ref(), b"."));
            s.clear();
            sv_setlen(s, 1)?;
            s[0] = b'.';
        }
    }

    Ok(())
}

pub fn sv_insert(buf: &mut SliceVec<u8>, i: usize, extra: &[u8]) -> Result<(), i32> {
    if extra.is_empty() {
        return Ok(());
    }

    let buflen = buf.len();
    let extralen = extra.len();
    sv_setlen(buf, buflen + extralen)?;

    buf.copy_within(i..buflen, i + extralen);

    buf[i..extralen + i].copy_from_slice(extra);

    Ok(())
}

pub fn sv_prepend(buf: &mut SliceVec<u8>, extra: &[u8]) -> Result<(), i32> {
    sv_insert(buf, 0, extra)
}

pub fn sv_strip_nfront(buf: &mut SliceVec<u8>, n: usize) {
    if n > 0 {
        let len = buf.len();
        buf.copy_within(n..len, 0);
        buf.set_len(buf.len() - n);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_leading_slashes() {
        assert_eq!(strip_leading_slashes(b""), b"");
        assert_eq!(strip_leading_slashes(b"/"), b"");
        assert_eq!(strip_leading_slashes(b"//"), b"");

        assert_eq!(strip_leading_slashes(b"abc"), b"abc");
        assert_eq!(strip_leading_slashes(b"abc/"), b"abc/");
        assert_eq!(strip_leading_slashes(b"abc/def"), b"abc/def");

        assert_eq!(strip_leading_slashes(b"/abc"), b"abc");
        assert_eq!(strip_leading_slashes(b"/abc/"), b"abc/");
        assert_eq!(strip_leading_slashes(b"/abc/def/"), b"abc/def/");
        assert_eq!(strip_leading_slashes(b"//abc/def/"), b"abc/def/");
    }

    #[test]
    fn test_component_iter() {
        fn check_it(it: ComponentIter, res: &[&[u8]]) {
            let mut res = res.iter();

            for component in it {
                assert_eq!(res.next().cloned(), Some(component));
            }

            assert_eq!(res.len(), 0, "{:?}", res);
        }

        assert_eq!(ComponentIter::new(b"").unwrap_err(), libc::ENOENT);
        assert_eq!(ComponentIter::new(b"\0").unwrap_err(), libc::EINVAL);

        check_it(ComponentIter::new(b"/").unwrap(), &[b"/"]);
        check_it(ComponentIter::new(b"/abc").unwrap(), &[b"/", b"abc"]);
        check_it(ComponentIter::new(b"/abc/").unwrap(), &[b"/", b"abc"]);

        check_it(ComponentIter::new(b"./abc/").unwrap(), &[b"abc"]);
        check_it(ComponentIter::new(b"/./abc/.").unwrap(), &[b"/", b"abc"]);
        check_it(
            ComponentIter::new(b"/../abc/..").unwrap(),
            &[b"/", b"..", b"abc", b".."],
        );
    }

    #[test]
    fn test_sv_parent() {
        let mut buf = [0; 100];
        let mut buf = SliceVec::from_slice_len(&mut buf, 0);

        buf.clear();
        buf.extend_from_slice(b".");
        sv_parent(&mut buf).unwrap();
        assert_eq!(buf.as_ref(), b"..");

        buf.clear();
        buf.extend_from_slice(b"abc");
        sv_parent(&mut buf).unwrap();
        assert_eq!(buf.as_ref(), b".");

        buf.clear();
        buf.extend_from_slice(b"abc");
        sv_parent(&mut buf).unwrap();
        assert_eq!(buf.as_ref(), b".");

        buf.clear();
        buf.extend_from_slice(b"/abc/def");
        sv_parent(&mut buf).unwrap();
        assert_eq!(buf.as_ref(), b"/abc");

        buf.clear();
        buf.extend_from_slice(b"/abc");
        sv_parent(&mut buf).unwrap();
        assert_eq!(buf.as_ref(), b"/");

        buf.clear();
        buf.extend_from_slice(b"/");
        sv_parent(&mut buf).unwrap();
        assert_eq!(buf.as_ref(), b"/");

        buf.clear();
        buf.extend_from_slice(b"..");
        sv_parent(&mut buf).unwrap();
        assert_eq!(buf.as_ref(), b"../..");

        buf.clear();
        buf.extend_from_slice(b"../..");
        sv_parent(&mut buf).unwrap();
        assert_eq!(buf.as_ref(), b"../../..");
    }

    #[test]
    fn test_sv_reserve() {
        let mut buf = [0; 10];
        let buf = SliceVec::from_slice_len(&mut buf, 0);

        sv_reserve(&buf, 0).unwrap();
        sv_reserve(&buf, 10).unwrap();
        sv_reserve(&buf, 11).unwrap_err();
    }

    #[test]
    fn test_sv_setlen() {
        let mut buf = [0; 10];
        let mut buf = SliceVec::from_slice_len(&mut buf, 0);

        sv_setlen(&mut buf, 0).unwrap();
        sv_setlen(&mut buf, 10).unwrap();
        sv_setlen(&mut buf, 11).unwrap_err();
    }

    #[test]
    fn test_sv_insert() {
        let mut buf = [0; 10];
        let mut buf = SliceVec::from_slice_len(&mut buf, 0);

        buf.clear();
        sv_insert(&mut buf, 0, b"abc").unwrap();
        assert_eq!(buf.as_ref(), b"abc");

        buf.clear();
        buf.extend_from_slice(b"abc");
        sv_insert(&mut buf, 0, b"").unwrap();
        assert_eq!(buf.as_ref(), b"abc");

        buf.clear();
        buf.extend_from_slice(b"def");
        sv_insert(&mut buf, 0, b"abc").unwrap();
        assert_eq!(buf.as_ref(), b"abcdef");

        buf.clear();
        buf.extend_from_slice(b"abc");
        sv_insert(&mut buf, 1, b"").unwrap();
        assert_eq!(buf.as_ref(), b"abc");

        buf.clear();
        buf.extend_from_slice(b"def");
        sv_insert(&mut buf, 1, b"abc").unwrap();
        assert_eq!(buf.as_ref(), b"dabcef");

        buf.clear();
        buf.extend_from_slice(b"def");
        sv_insert(&mut buf, 3, b"abc").unwrap();
        assert_eq!(buf.as_ref(), b"defabc");
    }

    #[test]
    fn test_sv_strip_nfront() {
        let mut buf = [0; 10];
        let mut buf = SliceVec::from_slice_len(&mut buf, 0);

        buf.clear();
        buf.extend_from_slice(b"abcdef");
        sv_strip_nfront(&mut buf, 0);
        assert_eq!(buf.as_ref(), b"abcdef");

        buf.clear();
        buf.extend_from_slice(b"abcdef");
        sv_strip_nfront(&mut buf, 1);
        assert_eq!(buf.as_ref(), b"bcdef");

        buf.clear();
        buf.extend_from_slice(b"abcdef");
        sv_strip_nfront(&mut buf, 6);
        assert_eq!(buf.as_ref(), b"");
    }
}
