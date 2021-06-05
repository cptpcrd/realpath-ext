#![forbid(unsafe_code)]

use core::fmt;
use core::ops::{Bound, Deref, DerefMut, RangeBounds};

pub struct SliceVec<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl<'a> SliceVec<'a> {
    #[inline]
    pub fn empty(buf: &'a mut [u8]) -> Self {
        Self { buf, len: 0 }
    }

    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.buf[..self.len]
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.buf.len()
    }

    #[inline]
    pub fn set_len(&mut self, new_len: usize) {
        assert!(self.len <= self.capacity());
        self.len = new_len;
    }

    #[inline]
    pub fn truncate(&mut self, max_len: usize) {
        self.len = core::cmp::min(max_len, self.len);
    }

    #[inline]
    pub fn clear(&mut self) {
        self.len = 0;
    }

    #[inline]
    pub fn push(&mut self, val: u8) -> Result<(), i32> {
        if let Some(ptr) = self.buf.get_mut(self.len) {
            self.len += 1;
            *ptr = val;
            Ok(())
        } else {
            Err(libc::ENAMETOOLONG)
        }
    }

    #[inline]
    pub fn pop(&mut self) {
        self.len = self.len.saturating_sub(1);
    }

    #[inline]
    pub fn extend_from_slice(&mut self, src: &[u8]) -> Result<(), i32> {
        if let Some(dest) = self.buf.get_mut(self.len..self.len + src.len()) {
            self.len += src.len();
            dest.copy_from_slice(src);
            Ok(())
        } else {
            Err(libc::ENAMETOOLONG)
        }
    }

    #[inline]
    pub fn replace(&mut self, src: &[u8]) -> Result<(), i32> {
        if let Some(dest) = self.buf.get_mut(..src.len()) {
            self.len = src.len();
            dest.copy_from_slice(src);
            Ok(())
        } else {
            Err(libc::ENAMETOOLONG)
        }
    }

    #[inline]
    pub fn insert_from_slice(&mut self, i: usize, src: &[u8]) -> Result<(), i32> {
        if !src.is_empty() {
            if self.len + src.len() > self.capacity() {
                return Err(libc::ENAMETOOLONG);
            }

            self.buf.copy_within(i..self.len, i + src.len());
            self.buf[i..i + src.len()].copy_from_slice(src);
            self.len += src.len();
        }

        Ok(())
    }

    #[inline]
    pub fn remove_range<R: RangeBounds<usize>>(&mut self, range: R) {
        let start = match range.start_bound() {
            Bound::Included(&i) => i,
            Bound::Excluded(&i) => i + 1,
            Bound::Unbounded => 0,
        };

        let end = match range.end_bound() {
            Bound::Included(&i) => i + 1,
            Bound::Excluded(&i) => i,
            Bound::Unbounded => self.len,
        };

        self.buf.copy_within(end..(self.len), start);
        self.len -= end - start;
    }

    pub fn make_parent_path(&mut self) -> Result<(), i32> {
        if self.is_empty() {
            self.extend_from_slice(b"..")
        } else if self.as_ref() == b".." || self.ends_with(b"/..") {
            self.extend_from_slice(b"/..")
        } else {
            debug_assert_ne!(self.as_ref(), b".");

            match self.iter().rposition(|&ch| ch == b'/') {
                // "/<...>"; truncate to "/"
                Some(0) => self.truncate(1),

                // "//<...>"; truncate to "//"
                Some(1) if self[0] == b'/' => self.truncate(2),

                // Trim from the last slash onward
                Some(i) => self.truncate(i),

                // No slashes!
                None => self.clear(),
            }

            Ok(())
        }
    }
}

impl AsRef<[u8]> for SliceVec<'_> {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self
    }
}

impl AsMut<[u8]> for SliceVec<'_> {
    #[inline]
    fn as_mut(&mut self) -> &mut [u8] {
        self
    }
}

impl<'a> Deref for SliceVec<'a> {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl<'a> DerefMut for SliceVec<'a> {
    #[inline]
    fn deref_mut(&mut self) -> &mut [u8] {
        self.as_mut_slice()
    }
}

impl fmt::Debug for SliceVec<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_list().entries(self.as_slice()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic() {
        let mut buf = [0; 20];
        let mut buf = SliceVec::empty(&mut buf);

        buf.extend_from_slice(b"abc").unwrap();
        assert_eq!(buf.as_ref(), b"abc");

        buf.insert_from_slice(2, b"def").unwrap();
        assert_eq!(buf.as_ref(), b"abdefc");

        buf.push(b'g').unwrap();
        assert_eq!(buf.as_ref(), b"abdefcg");

        buf.replace(b"hijklmn").unwrap();
        assert_eq!(buf.as_ref(), b"hijklmn");

        buf.remove_range(1..2);
        assert_eq!(buf.as_ref(), b"hjklmn");

        buf.remove_range(2..=4);
        assert_eq!(buf.as_ref(), b"hjn");

        buf.clear();
        buf.insert_from_slice(0, b"opq").unwrap();
        assert_eq!(buf.as_ref(), b"opq");

        buf.pop();
        assert_eq!(buf.as_ref(), b"op");
        buf.pop();
        assert_eq!(buf.as_ref(), b"o");
        buf.pop();
        assert_eq!(buf.as_ref(), b"");
    }

    #[test]
    fn test_remove_range() {
        let mut buf = [0; 20];
        let mut buf = SliceVec::empty(&mut buf);

        buf.extend_from_slice(b"abcdefghi").unwrap();

        buf.remove_range(0..2);
        assert_eq!(buf.as_ref(), b"cdefghi");

        buf.remove_range(0..=1);
        assert_eq!(buf.as_ref(), b"efghi");

        buf.remove_range(..=1);
        assert_eq!(buf.as_ref(), b"ghi");

        buf.remove_range(1..);
        assert_eq!(buf.as_ref(), b"g");

        buf.remove_range(..);
        assert_eq!(buf.as_ref(), b"");
    }

    #[test]
    fn test_overflow() {
        let mut buf = [0; 3];
        let mut buf = SliceVec::empty(&mut buf);

        buf.extend_from_slice(b"abc").unwrap();
        assert_eq!(buf.as_ref(), b"abc");

        assert_eq!(buf.push(b'd').unwrap_err(), libc::ENAMETOOLONG);
        assert_eq!(buf.extend_from_slice(b"d").unwrap_err(), libc::ENAMETOOLONG);
        assert_eq!(
            buf.insert_from_slice(2, b"d").unwrap_err(),
            libc::ENAMETOOLONG
        );

        buf.pop();

        assert_eq!(
            buf.insert_from_slice(2, b"de").unwrap_err(),
            libc::ENAMETOOLONG
        );
        buf.insert_from_slice(2, b"d").unwrap();

        assert_eq!(buf.replace(b"abcd").unwrap_err(), libc::ENAMETOOLONG);
        assert_eq!(buf.as_ref(), b"abd");
        buf.replace(b"efg").unwrap();
        assert_eq!(buf.as_ref(), b"efg");
    }

    #[test]
    fn test_make_parent_path() {
        let mut buf = [0; 20];
        let mut buf = SliceVec::empty(&mut buf);

        buf.replace(b"").unwrap();
        buf.make_parent_path().unwrap();
        assert_eq!(buf.as_ref(), b"..");

        buf.replace(b"abc").unwrap();
        buf.make_parent_path().unwrap();
        assert_eq!(buf.as_ref(), b"");

        buf.replace(b"abc").unwrap();
        buf.make_parent_path().unwrap();
        assert_eq!(buf.as_ref(), b"");

        buf.replace(b"/abc/def").unwrap();
        buf.make_parent_path().unwrap();
        assert_eq!(buf.as_ref(), b"/abc");

        buf.replace(b"/abc").unwrap();
        buf.make_parent_path().unwrap();
        assert_eq!(buf.as_ref(), b"/");

        buf.replace(b"/").unwrap();
        buf.make_parent_path().unwrap();
        assert_eq!(buf.as_ref(), b"/");

        buf.replace(b"//abc/def").unwrap();
        buf.make_parent_path().unwrap();
        assert_eq!(buf.as_ref(), b"//abc");

        buf.replace(b"//abc").unwrap();
        buf.make_parent_path().unwrap();
        assert_eq!(buf.as_ref(), b"//");

        buf.replace(b"//").unwrap();
        buf.make_parent_path().unwrap();
        assert_eq!(buf.as_ref(), b"//");

        buf.replace(b"..").unwrap();
        buf.make_parent_path().unwrap();
        assert_eq!(buf.as_ref(), b"../..");

        buf.replace(b"../..").unwrap();
        buf.make_parent_path().unwrap();
        assert_eq!(buf.as_ref(), b"../../..");
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_debug() {
        let mut buf = [0; 20];
        let mut buf = SliceVec::empty(&mut buf);

        assert_eq!(format!("{:?}", buf), "[]");

        buf.replace(&[0, 1, 2]).unwrap();
        assert_eq!(format!("{:?}", buf), "[0, 1, 2]");
    }
}
