use crate::slicevec::SliceVec;

#[inline]
pub fn errno_get() -> i32 {
    errno::errno().0
}

#[derive(Debug)]
pub struct SymlinkCounter {
    max: u16,
    cur: u16,
}

impl SymlinkCounter {
    // This is Linux's limit (sysconf(_SC_SYMLOOP_MAX) always fails on glibc, so we need a fallback)
    const DEFAULT_SYMLOOP_MAX: u16 = 40;

    #[inline]
    pub fn new() -> Self {
        use core::convert::TryInto;

        Self {
            max: unsafe { libc::sysconf(libc::_SC_SYMLOOP_MAX) }
                .try_into()
                .unwrap_or(Self::DEFAULT_SYMLOOP_MAX),
            cur: 0,
        }
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
pub struct ComponentStack<'a> {
    buf: &'a mut [u8],
    i: usize,
}

impl<'a> ComponentStack<'a> {
    #[inline]
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self { i: buf.len(), buf }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.i == self.buf.len()
    }

    pub unsafe fn push_readlink(&mut self, path: *const u8) -> Result<(), i32> {
        if self.i == 0 {
            return Err(libc::ENAMETOOLONG);
        }

        match libc::readlink(
            path as *const _,
            self.buf.as_mut_ptr() as *mut libc::c_char,
            self.i,
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
                let len = if self.buf[len - 1] == 0 { len - 1 } else { len };

                debug_assert_ne!(self.buf[len - 1], 0);

                if len >= self.i - 1 {
                    Err(libc::ENAMETOOLONG)
                } else {
                    self.i -= 1;
                    self.buf[self.i] = 0;
                    self.i -= len;
                    self.buf.copy_within(0..len, self.i);

                    Ok(())
                }
            }
        }
    }

    pub fn next(&mut self) -> Option<&[u8]> {
        macro_rules! skip_slashes_nul {
            ($self:expr) => {{
                while let Some((&b'/', _)) = $self.buf[$self.i..].split_first() {
                    $self.i += 1;
                }

                if $self.buf.get($self.i) == Some(&0) {
                    // We've exhausted the first path; advance to the next one
                    $self.i += 1;
                    debug_assert_ne!($self.buf.get($self.i), Some(&0));
                }
            }};
        }

        loop {
            match self.buf[self.i..].split_first() {
                // Empty -> nothing left to iterate over
                None => return None,

                // The first path starts with a slash
                Some((&b'/', rest)) => {
                    // Trim leading slashes from the path
                    self.i += 1;
                    skip_slashes_nul!(self);

                    if rest.first() == Some(&b'/') && rest.get(1) != Some(&b'/') {
                        debug_assert!(rest.starts_with(b"/"));
                        debug_assert!(!rest.starts_with(b"//"));
                        return Some(b"//");
                    } else {
                        debug_assert!(!rest.starts_with(b"/") || rest.starts_with(b"//"));
                        return Some(b"/");
                    }
                }

                // So there's at least one path, and it doesn't start with a slash
                _ => {
                    // The first byte should NOT be NUL; we remove trailing slashes when we hit the end
                    debug_assert_ne!(self.buf[self.i], 0);

                    let component;

                    if let Some(offset) =
                        self.buf[self.i..].iter().position(|&c| c == b'/' || c == 0)
                    {
                        component = &self.buf[self.i..self.i + offset];
                        self.i += offset;
                        skip_slashes_nul!(self);
                    } else {
                        // The entire stack did not contain any slashes or NUL bytes.
                        // This means we've got one component left.
                        component = &self.buf[self.i..];
                        self.i = self.buf.len();
                    };

                    if component != b"" && component != b"." {
                        debug_assert!(!component.contains(&0));
                        break Some(component);
                    }
                }
            }
        }
    }

    pub fn clear(&mut self) -> &mut [u8] {
        self.i = self.buf.len();
        &mut self.buf
    }
}

#[derive(Clone, Debug)]
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

    pub fn is_empty(&self) -> bool {
        match self.0.split_first() {
            None => true,
            Some((&b'/', _)) => false,
            _ => self.clone().next().is_none(),
        }
    }
}

impl<'a> Iterator for ComponentIter<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        match self.0.split_first() {
            // Empty -> nothing left to iterate over
            None => None,

            // Starts with a slash
            Some((&b'/', rest)) => {
                // Trim leading slashes from the path
                self.0 = strip_leading_slashes(rest);

                if rest.len() - self.0.len() == 1 {
                    // This means that at the start of this method, `self.0` started with exactly 2
                    // slashes. One was removed by split_first(), and the other by
                    // strip_leading_slashes().
                    debug_assert!(rest.starts_with(b"/"));
                    debug_assert!(!rest.starts_with(b"//"));
                    Some(b"//")
                } else {
                    debug_assert!(!rest.starts_with(b"/") || rest.starts_with(b"//"));
                    Some(b"/")
                }
            }

            // So it's not empty and it doesn't start with a slash
            _ => loop {
                let component = match self.0.iter().position(|&c| c == b'/') {
                    // We were able to find a slash inside it
                    // Return the part up to the slash, then strip the rest
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

#[inline]
pub unsafe fn readlink_empty(path: *const u8) -> Result<(), i32> {
    if libc::readlink(path as *const _, &mut 0, 1) < 0 {
        Err(errno_get())
    } else {
        Ok(())
    }
}

pub fn getcwd(buf: &mut SliceVec) -> Result<(), i32> {
    buf.set_len(buf.capacity());

    #[cfg(target_family = "unix")]
    use libc::getcwd;
    #[cfg(target_os = "wasi")]
    extern "C" {
        fn getcwd(buf: *mut libc::c_char, size: libc::size_t) -> *mut libc::c_char;
    }

    if unsafe { getcwd(buf.as_mut_ptr() as *mut _, buf.len()) }.is_null() {
        Err(match errno_get() {
            libc::EINVAL | libc::ERANGE => libc::ENAMETOOLONG,
            eno => eno,
        })
    } else if buf[0] != b'/' {
        Err(libc::ENOENT)
    } else {
        buf.set_len(buf.iter().position(|&ch| ch == 0).unwrap());
        Ok(())
    }
}

pub fn strip_leading_slashes(mut s: &[u8]) -> &[u8] {
    while let Some((&b'/', rest)) = s.split_first() {
        s = rest;
    }
    s
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
    fn test_component_stack() {
        let mut buf = [0; 100];
        let mut stack = ComponentStack::new(&mut buf);

        assert_eq!(stack.next(), None);

        let data = b"pqr/\0mno\0/jkl\0ghi/\0///abc/./def/\0//\0/\0.";
        stack.i = stack.buf.len() - data.len();
        stack.buf[stack.i..].copy_from_slice(data);

        assert_eq!(stack.next().unwrap(), b"pqr");
        assert_eq!(stack.next().unwrap(), b"mno");
        assert_eq!(stack.next().unwrap(), b"/");
        assert_eq!(stack.next().unwrap(), b"jkl");
        assert_eq!(stack.next().unwrap(), b"ghi");
        assert_eq!(stack.next().unwrap(), b"/");
        assert_eq!(stack.next().unwrap(), b"abc");
        assert_eq!(stack.next().unwrap(), b"def");
        assert_eq!(stack.next().unwrap(), b"//");
        assert_eq!(stack.next().unwrap(), b"/");
        assert_eq!(stack.next(), None);

        let data = b"abc";
        stack.i = stack.buf.len() - data.len();
        stack.buf[stack.i..].copy_from_slice(data);

        assert_eq!(stack.next().unwrap(), b"abc");
        assert_eq!(stack.next(), None);

        let mut stack = ComponentStack::new(&mut []);
        assert_eq!(
            unsafe { stack.push_readlink(b"/\0".as_ptr()) }.unwrap_err(),
            libc::ENAMETOOLONG,
        );
    }

    #[test]
    fn test_component_iter() {
        fn check_it(mut it: ComponentIter, res: &[&[u8]]) {
            let mut res = res.iter();

            while let Some(component) = it.next() {
                assert_eq!(res.next().cloned(), Some(component));
                assert_eq!(it.is_empty(), res.len() == 0);
            }

            assert!(it.is_empty());
            assert_eq!(res.len(), 0, "{:?}", res);
        }

        assert_eq!(ComponentIter::new(b"").unwrap_err(), libc::ENOENT);
        assert_eq!(ComponentIter::new(b"\0").unwrap_err(), libc::EINVAL);

        check_it(ComponentIter::new(b"/").unwrap(), &[b"/"]);
        check_it(ComponentIter::new(b"/abc").unwrap(), &[b"/", b"abc"]);
        check_it(ComponentIter::new(b"/abc/").unwrap(), &[b"/", b"abc"]);

        check_it(ComponentIter::new(b"//").unwrap(), &[b"//"]);
        check_it(ComponentIter::new(b"//abc").unwrap(), &[b"//", b"abc"]);
        check_it(ComponentIter::new(b"//abc/").unwrap(), &[b"//", b"abc"]);

        check_it(ComponentIter::new(b"./abc/").unwrap(), &[b"abc"]);
        check_it(ComponentIter::new(b"/./abc/.").unwrap(), &[b"/", b"abc"]);
        check_it(
            ComponentIter::new(b"/../abc/..").unwrap(),
            &[b"/", b"..", b"abc", b".."],
        );
    }

    #[test]
    fn test_getcwd_toolong() {
        assert_eq!(
            getcwd(&mut SliceVec::empty(&mut [])).unwrap_err(),
            libc::ENAMETOOLONG
        );
        assert_eq!(
            getcwd(&mut SliceVec::empty(&mut [0])).unwrap_err(),
            libc::ENAMETOOLONG
        );
    }

    #[test]
    fn test_check_isdir() {
        unsafe {
            assert_eq!(check_isdir(b"\0".as_ptr()).unwrap_err(), libc::ENOENT);
            assert_eq!(
                check_isdir(b"/bin/sh\0".as_ptr()).unwrap_err(),
                libc::ENOTDIR
            );
            check_isdir(b"/\0".as_ptr()).unwrap();
        }
    }

    #[test]
    fn test_readlink_empty() {
        unsafe {
            assert_eq!(readlink_empty(b"\0".as_ptr()).unwrap_err(), libc::ENOENT);
            assert_eq!(readlink_empty(b"/\0".as_ptr()).unwrap_err(), libc::EINVAL);
        }
    }
}
