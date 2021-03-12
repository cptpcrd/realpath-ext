use std::env;
use std::io::prelude::*;
use std::os::unix::prelude::*;

use realpath_ext::{realpath, RealpathFlags};

#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
enum MissingBehavior {
    Error,
    IgnoreLast,
    Ignore,
}

fn print_help() {
    eprintln!("Usage: realpath-ext [-e | -m] [-L | -P | -s] [-q] [-z]");
}

fn main() {
    let args = env::args_os().skip(1).collect::<Vec<_>>();

    let mut quiet = false;
    let mut missing = MissingBehavior::IgnoreLast;
    let mut logical = false;
    let mut no_symlinks = false;
    let mut zero = false;

    let mut files = Vec::new();

    let mut it = args.iter();
    while let Some(arg_os) = it.next() {
        let arg = arg_os.as_bytes();

        if arg == b"--" {
            files.extend(it.by_ref());
        } else if arg.starts_with(b"--") {
            match &arg[2..] {
                b"canonicalize-existing" => missing = MissingBehavior::Error,
                b"canonicalize-missing" => missing = MissingBehavior::Ignore,
                b"logical" => {
                    logical = true;
                    no_symlinks = false;
                }
                b"physical" => {
                    logical = false;
                    no_symlinks = false;
                }
                b"quiet" => quiet = true,
                b"strip" | b"no-symlinks" => no_symlinks = true,
                b"zero" => zero = true,

                b"help" => {
                    print_help();
                    return;
                }

                _ => {
                    eprintln!("Unknown option {:?}", arg_os);
                    std::process::exit(1);
                }
            }
        } else if arg.starts_with(b"-") {
            for &ch in arg[1..].iter() {
                match ch {
                    b'e' => missing = MissingBehavior::Error,
                    b'm' => missing = MissingBehavior::Ignore,
                    b'L' => {
                        logical = true;
                        no_symlinks = false;
                    }
                    b'P' => {
                        logical = false;
                        no_symlinks = false;
                    }
                    b'q' => quiet = true,
                    b's' => no_symlinks = true,
                    b'z' => zero = true,

                    _ => {
                        eprintln!("realpath-ext: Unknown option '{}'", char::from(ch));
                        std::process::exit(1);
                    }
                }
            }
        } else {
            files.push(arg_os);
        }
    }

    if files.is_empty() {
        print_help();
        std::process::exit(1);
    }

    let mut flags = RealpathFlags::empty();

    match missing {
        MissingBehavior::IgnoreLast => flags |= RealpathFlags::ALLOW_LAST_MISSING,
        MissingBehavior::Ignore => flags |= RealpathFlags::ALLOW_MISSING,
        MissingBehavior::Error => (),
    }

    if no_symlinks {
        flags |= RealpathFlags::IGNORE_SYMLINKS;
    }

    let mut error = false;

    for path in files.into_iter() {
        match (|mut path| {
            if logical {
                path = realpath(path, flags | RealpathFlags::IGNORE_SYMLINKS)?;
            }

            path = realpath(path, flags)?;

            Ok(path)
        })(path.into())
        {
            Ok(path) => {
                let stdout = std::io::stdout();
                let mut stdout = stdout.lock();

                stdout.write_all(path.as_os_str().as_bytes()).unwrap();
                stdout.write_all(if zero { b"\0" } else { b"\n" }).unwrap();
            }

            Err(e) => {
                let _: std::io::Error = e;
                if !quiet {
                    eprintln!("realpath-ext: {:?}: {}", path, e);
                }
                error = true;
            }
        }
    }

    if error {
        std::process::exit(1);
    }
}
