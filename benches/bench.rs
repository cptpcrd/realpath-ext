use std::env;
use std::fs;
use std::os::unix::prelude::*;
use std::path::Path;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

fn bench(c: &mut Criterion) {
    let exe = env::current_exe().unwrap();
    let cwd = env::current_dir().unwrap();

    let mut buf = [0; libc::PATH_MAX as usize];

    let mut group = c.benchmark_group("realpath");

    for (ident, path) in [
        ("root", "/".as_ref()),
        ("dot", ".".as_ref()),
        ("2-parent", "../../".as_ref()),
        ("bin", "/bin".as_ref()),
        ("bin (dir)", "/bin/".as_ref()),
        ("passwd", "/etc/passwd".as_ref()),
        ("exe", exe.as_ref()),
        ("cwd", cwd.as_ref()),
    ]
    .iter()
    {
        let path = Path::as_os_str(*path);

        group.bench_with_input(
            BenchmarkId::new("realpath::realpath_raw", ident),
            path,
            |b, i| {
                b.iter(|| {
                    let n = realpath::realpath_raw(i.as_bytes(), &mut buf).unwrap();
                    black_box(&buf[..n]);
                })
            },
        );

        #[cfg(feature = "std")]
        group.bench_with_input(
            BenchmarkId::new("realpath::realpath", ident),
            path,
            |b, i| {
                b.iter(|| {
                    let path = realpath::realpath(i).unwrap();
                    black_box(path);
                })
            },
        );

        group.bench_with_input(BenchmarkId::new("fs::canonicalize", ident), path, |b, i| {
            b.iter(|| {
                let path = fs::canonicalize(i).unwrap();
                black_box(path);
            })
        });
    }

    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
