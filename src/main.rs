mod output;

use std::{
    env,
    ffi::OsString,
    fs,
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
};

const USAGE: &str = "Usage: makefile-fmt [-w | --check | --diff] [--] FILE\n\n\
Format a GNU Makefile using the subset in MVP.md and MVP-0.2.md.\n\
Requires forked shfmt with --explicit-semicolons on PATH.\n\n\
  -w         Replace FILE after all checks succeed\n\
  --check    Exit 1 if formatting would change FILE\n\
  --diff     Print a unified diff (exit 0 on success)\n\
  --help     Show this help\n\
  --version  Show the version\n";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Stdout,
    Write,
    Check,
    Diff,
}

fn run() -> Result<u8, (u8, String)> {
    let mut mode = Mode::Stdout;
    let mut path: Option<PathBuf> = None;
    let mut options = true;
    for arg in env::args_os().skip(1) {
        if options && arg == "--" {
            options = false;
            continue;
        }
        if options && (arg == "--help" || arg == "--version") {
            let text = if arg == "--help" {
                USAGE.to_owned()
            } else {
                format!("makefile-fmt {}\n", env!("CARGO_PKG_VERSION"))
            };
            io::stdout()
                .lock()
                .write_all(text.as_bytes())
                .map_err(|e| (3, e.to_string()))?;
            return Ok(0);
        }
        let selected = if options {
            match arg.to_str() {
                Some("-w") => Some(Mode::Write),
                Some("--check") => Some(Mode::Check),
                Some("--diff") => Some(Mode::Diff),
                _ => None,
            }
        } else {
            None
        };
        if let Some(selected) = selected {
            if mode != Mode::Stdout {
                return Err((2, "choose only one of -w, --check, --diff".into()));
            }
            mode = selected;
        } else if options && arg.to_string_lossy().starts_with('-') {
            return Err((
                2,
                format!("unknown option: {}\n{USAGE}", arg.to_string_lossy()),
            ));
        } else if path.replace(PathBuf::from(arg)).is_some() {
            return Err((2, "expected exactly one input file".into()));
        }
    }
    let path = path.ok_or_else(|| (2, USAGE.to_owned()))?;
    let io_error = |e: io::Error| (3, format!("{}: {e}", path.display()));
    let resolved = fs::canonicalize(&path).map_err(io_error)?;
    let metadata = fs::metadata(&resolved).map_err(io_error)?;
    if !metadata.is_file() {
        return Err((3, format!("{}: expected a regular file", path.display())));
    }
    let source = fs::read(&resolved).map_err(io_error)?;
    let formatted = makefile_fmt::format(&source, OsString::from("shfmt"))
        .map_err(|e| (e.exit_code(), format!("{}:{e}", path.display())))?;
    let changed = source != formatted;
    match mode {
        Mode::Check => return Ok(u8::from(changed)),
        Mode::Stdout => io::stdout()
            .lock()
            .write_all(&formatted)
            .map_err(io_error)?,
        Mode::Diff if changed => io::stdout()
            .lock()
            .write_all(&output::diff(&path, &source, &formatted))
            .map_err(io_error)?,
        Mode::Write if changed => {
            output::replace(&resolved, &source, &formatted, &metadata).map_err(io_error)?
        }
        _ => {}
    }
    Ok(0)
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err((code, message)) => {
            let _ = writeln!(io::stderr().lock(), "makefile-fmt: {message}");
            ExitCode::from(code)
        }
    }
}
