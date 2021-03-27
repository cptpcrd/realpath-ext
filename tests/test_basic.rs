use std::ffi::OsString;
use std::fs;

use realpath_ext::RealpathFlags;

#[cfg(feature = "std")]
use realpath_ext::realpath;

#[cfg(not(feature = "std"))]
fn realpath<P: AsRef<std::path::Path>>(
    path: P,
    flags: RealpathFlags,
) -> std::io::Result<std::path::PathBuf> {
    use std::os::unix::prelude::*;

    let mut buf = vec![0; libc::PATH_MAX as usize];

    let n = realpath_ext::realpath_raw(path.as_ref().as_os_str().as_bytes(), &mut buf, flags)
        .map_err(std::io::Error::from_raw_os_error)?;

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
        "/bin",
        "///usr/./bin/.",
        "src",
        "/etc/passwd",
        tmpdir.to_str().unwrap(),
        exe.to_str().unwrap(),
        cwd.to_str().unwrap(),
        alt_cwd.to_str().unwrap(),
        #[cfg(all(target_os = "linux", any(target_env = "gnu", target_env = "")))]
        &"./".repeat(libc::PATH_MAX as usize),
    ]
    .iter()
    {
        assert_eq!(
            realpath(path, RealpathFlags::empty()).unwrap().as_os_str(),
            fs::canonicalize(path).unwrap().as_os_str()
        );

        realpath(path, RealpathFlags::IGNORE_SYMLINKS).unwrap();
    }
}

#[test]
fn test_leading_slashes() {
    assert_eq!(
        realpath("/", RealpathFlags::empty()).unwrap().as_os_str(),
        "/"
    );
    assert_eq!(
        realpath("//", RealpathFlags::empty()).unwrap().as_os_str(),
        "//"
    );
    assert_eq!(
        realpath("///", RealpathFlags::empty()).unwrap().as_os_str(),
        "/"
    );

    assert!(!realpath("/", RealpathFlags::empty())
        .unwrap()
        .as_os_str()
        .to_str()
        .unwrap()
        .starts_with("//"));
    assert!(realpath("//", RealpathFlags::empty())
        .unwrap()
        .as_os_str()
        .to_str()
        .unwrap()
        .starts_with("//"));
    assert!(!realpath("///", RealpathFlags::empty())
        .unwrap()
        .as_os_str()
        .to_str()
        .unwrap()
        .starts_with("//"));
}

#[test]
fn test_enotdir() {
    let exe = std::env::current_exe().unwrap().into_os_string();
    let mut path = exe.clone();
    path.push("/.");
    assert_eq!(
        realpath(&path, RealpathFlags::empty())
            .unwrap_err()
            .raw_os_error(),
        Some(libc::ENOTDIR)
    );

    assert_eq!(
        realpath(&path, RealpathFlags::IGNORE_SYMLINKS)
            .unwrap_err()
            .raw_os_error(),
        Some(libc::ENOTDIR)
    );

    assert_eq!(
        realpath(&path, RealpathFlags::ALLOW_LAST_MISSING)
            .unwrap_err()
            .raw_os_error(),
        Some(libc::ENOTDIR)
    );

    let mut path2 = exe;
    path2.push("/a/.");

    assert_eq!(
        realpath(&path2, RealpathFlags::empty())
            .unwrap_err()
            .raw_os_error(),
        Some(libc::ENOTDIR)
    );

    assert_eq!(
        realpath(&path2, RealpathFlags::IGNORE_SYMLINKS)
            .unwrap_err()
            .raw_os_error(),
        Some(libc::ENOTDIR)
    );

    realpath(&path2, RealpathFlags::ALLOW_MISSING).unwrap();

    assert_eq!(
        realpath("/etc/passwd/", RealpathFlags::empty())
            .unwrap_err()
            .raw_os_error(),
        Some(libc::ENOTDIR)
    );
    assert_eq!(
        realpath("/etc/passwd/abc", RealpathFlags::empty())
            .unwrap_err()
            .raw_os_error(),
        Some(libc::ENOTDIR)
    );

    assert_eq!(
        realpath("/etc/passwd/", RealpathFlags::IGNORE_SYMLINKS)
            .unwrap_err()
            .raw_os_error(),
        Some(libc::ENOTDIR)
    );
    assert_eq!(
        realpath("/etc/passwd/abc", RealpathFlags::IGNORE_SYMLINKS)
            .unwrap_err()
            .raw_os_error(),
        Some(libc::ENOTDIR)
    );

    realpath("/etc/passwd/", RealpathFlags::ALLOW_MISSING).unwrap();
    assert_eq!(
        realpath("/etc/passwd/", RealpathFlags::ALLOW_LAST_MISSING)
            .unwrap_err()
            .raw_os_error(),
        Some(libc::ENOTDIR)
    );
    realpath("/etc/passwd/abc", RealpathFlags::ALLOW_MISSING).unwrap();
    assert_eq!(
        realpath("/etc/passwd/abc", RealpathFlags::ALLOW_LAST_MISSING)
            .unwrap_err()
            .raw_os_error(),
        Some(libc::ENOTDIR)
    );
}

#[test]
fn test_enoent() {
    assert_eq!(
        realpath("NOEXIST", RealpathFlags::empty())
            .unwrap_err()
            .raw_os_error(),
        Some(libc::ENOENT)
    );

    realpath("NOEXIST", RealpathFlags::ALLOW_MISSING).unwrap();
    realpath("NOEXIST", RealpathFlags::ALLOW_LAST_MISSING).unwrap();

    realpath("NOEXIST/abc", RealpathFlags::ALLOW_MISSING).unwrap();
    assert_eq!(
        realpath("NOEXIST/abc", RealpathFlags::ALLOW_LAST_MISSING)
            .unwrap_err()
            .raw_os_error(),
        Some(libc::ENOENT)
    );
}
