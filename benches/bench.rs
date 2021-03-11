use std::env;
use std::fs;
use std::os::unix::prelude::*;

use criterion::{black_box, criterion_group, criterion_main, Criterion};

use realpath::realpath_raw;

fn bench(c: &mut Criterion) {
    let exe = env::current_exe().unwrap();
    let cwd = env::current_dir().unwrap();

    let mut buf = [0; libc::PATH_MAX as usize];
    c.bench_function("realpath_raw dot", |b| {
        b.iter(|| {
            let n = realpath_raw(b".", &mut buf).unwrap();
            black_box(&buf[..n]);
        })
    });

    c.bench_function("fs::canonicalize dot", |b| {
        b.iter(|| {
            let path = fs::canonicalize(".").unwrap();
            black_box(path);
        })
    });

    c.bench_function("realpath_raw /bin", |b| {
        b.iter(|| {
            let n = realpath_raw(b"/bin", &mut buf).unwrap();
            black_box(&buf[..n]);
        })
    });

    c.bench_function("fs::canonicalize /bin", |b| {
        b.iter(|| {
            let path = fs::canonicalize("/bin").unwrap();
            black_box(path);
        })
    });

    c.bench_function("realpath_raw exe", |b| {
        b.iter(|| {
            let n = realpath_raw(exe.as_os_str().as_bytes(), &mut buf).unwrap();
            black_box(&buf[..n]);
        })
    });

    c.bench_function("fs::canonicalize exe", |b| {
        b.iter(|| {
            let path = fs::canonicalize(&exe).unwrap();
            black_box(path);
        })
    });

    c.bench_function("realpath_raw cwd", |b| {
        b.iter(|| {
            let n = realpath_raw(cwd.as_os_str().as_bytes(), &mut buf).unwrap();
            black_box(&buf[..n]);
        })
    });

    c.bench_function("fs::canonicalize cwd", |b| {
        b.iter(|| {
            let path = fs::canonicalize(&cwd).unwrap();
            black_box(path);
        })
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
