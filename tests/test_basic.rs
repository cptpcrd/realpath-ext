use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::prelude::*;
use std::path::{Path, PathBuf};

use realpath::realpath_raw;

fn realpath<P: AsRef<Path>>(path: P) -> io::Result<PathBuf> {
    let mut buf = vec![0; libc::PATH_MAX as usize];

    let n = realpath_raw(path.as_ref().as_os_str().as_bytes(), &mut buf)
        .map_err(io::Error::from_raw_os_error)?;

    buf.truncate(n);
    Ok(OsString::from_vec(buf).into())
}

#[test]
fn test_success() {
    let tmpdir = std::env::temp_dir();
    let exe = std::env::current_exe().unwrap();
    let cwd = std::env::current_dir().unwrap();

    let mut alt_cwd;
    if let Some(fname) = cwd.file_name() {
        alt_cwd = OsString::from("../");
        alt_cwd.push(fname);
    } else {
        alt_cwd = cwd.clone().into();
    }

    for &path in [
        "/",
        ".",
        "..",
        "../..",
        "..//..//../",
        "//bin",
        "///usr/./bin/.",
        "src",
        "/etc/passwd",
        tmpdir.to_str().unwrap(),
        exe.to_str().unwrap(),
        cwd.to_str().unwrap(),
        alt_cwd.to_str().unwrap(),
    ]
    .iter()
    {
        assert_eq!(
            realpath(path).unwrap().as_os_str(),
            fs::canonicalize(path).unwrap().as_os_str()
        );
    }
}

#[test]
fn test_enotdir() {
    let mut path = std::env::current_exe().unwrap().into_os_string();
    path.push("/.");
    assert_eq!(
        realpath(path).unwrap_err().raw_os_error(),
        Some(libc::ENOTDIR)
    );

    assert_eq!(
        realpath("/etc/passwd/").unwrap_err().raw_os_error(),
        Some(libc::ENOTDIR)
    );
}

#[test]
fn test_enoent() {
    assert_eq!(
        realpath("NOEXIST").unwrap_err().raw_os_error(),
        Some(libc::ENOENT)
    );
}
