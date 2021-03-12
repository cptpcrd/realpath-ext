use crate::slicevec::SliceVec;

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
    pub fn push(&mut self, path: &[u8]) -> Result<(), i32> {
        if path.is_empty() {
            Err(libc::ENOENT)
        } else if path.contains(&0) {
            Err(libc::EINVAL)
        } else {
            if self.i != self.buf.len() {
                self.i -= 1;
                self.buf[self.i] = 0;
            }

            if let Some(newi) = self.i.checked_sub(path.len()) {
                self.buf[newi..self.i].copy_from_slice(path);
                self.i = newi;
                Ok(())
            } else {
                if self.buf.get(self.i) == Some(&0) {
                    self.i += 1;
                }
                Err(libc::ENAMETOOLONG)
            }
        }
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
                Some((&b'/', _)) => {
                    self.i += 1;
                    skip_slashes_nul!(self);
                    return Some(&[b'/']);
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

pub fn getcwd(buf: &mut SliceVec) -> Result<(), i32> {
    buf.set_len(buf.capacity());

    if buf.is_empty() {
        Err(libc::ENAMETOOLONG)
    } else if unsafe { libc::getcwd(buf.as_mut_ptr() as *mut _, buf.len()) }.is_null() {
        Err(errno_get())
    } else if buf[0] != b'/' {
        Err(libc::ENOENT)
    } else {
        buf.set_len(buf.iter().position(|&ch| ch == 0).unwrap());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_component_stack() {
        let mut buf = [0; 100];
        let mut stack = ComponentStack::new(&mut buf);

        assert_eq!(stack.push(b"").unwrap_err(), libc::ENOENT);
        assert_eq!(stack.push(b"\0").unwrap_err(), libc::EINVAL);
        assert_eq!(stack.push(&[b'a'; 101]).unwrap_err(), libc::ENAMETOOLONG);

        assert_eq!(stack.next(), None);

        stack.push(b".").unwrap();
        stack.push(b"/").unwrap();
        stack.push(b"//").unwrap();
        stack.push(b"/abc/./def/").unwrap();
        stack.push(b"ghi/").unwrap();
        stack.push(b"/jkl").unwrap();

        assert_eq!(stack.next().unwrap(), b"/");
        assert_eq!(stack.next().unwrap(), b"jkl");
        assert_eq!(stack.next().unwrap(), b"ghi");
        assert_eq!(stack.next().unwrap(), b"/");
        assert_eq!(stack.next().unwrap(), b"abc");
        assert_eq!(stack.next().unwrap(), b"def");
        assert_eq!(stack.next().unwrap(), b"/");
        assert_eq!(stack.next().unwrap(), b"/");
    }
}
