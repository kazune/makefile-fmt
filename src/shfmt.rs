use std::{
    ffi::{OsStr, OsString},
    io::Write,
    process::{Command, Output, Stdio},
    thread,
};

use crate::Error;

pub(crate) struct Shfmt {
    executable: OsString,
}

impl Shfmt {
    pub fn new(executable: &OsStr) -> Result<Self, Error> {
        let formatter = Self {
            executable: executable.to_owned(),
        };
        // Exercise the required behavior, rather than trusting --help/version
        // or accepting a binary that silently ignores the option.
        let probe = formatter.run(b"echo makefile_fmt_probe\nif true; then\necho ok\nfi\n")?;
        if !probe.status.success()
            || probe.stdout != b"echo makefile_fmt_probe;\nif true; then\n\techo ok;\nfi;\n"
        {
            return Err(Error::Tool(format!(
                "required POSIX/explicit-semicolon capability check failed: {}",
                String::from_utf8_lossy(&probe.stderr).trim()
            )));
        }
        Ok(formatter)
    }

    fn run(&self, input: &[u8]) -> Result<Output, Error> {
        let mut child = Command::new(&self.executable)
            // Explicit parser/printer flags disable EditorConfig in shfmt.
            .args([
                "-ln=posix",
                "-i=0",
                "-s=false",
                "-mn=false",
                "-bn=false",
                "-ci=false",
                "-sr=false",
                "-kp=false",
                "-fn=false",
                "-bl=false",
                "--explicit-semicolons",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                Error::Tool(format!(
                    "cannot start {}: {e}",
                    self.executable.to_string_lossy()
                ))
            })?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        // Drain stdout/stderr while supplying input; large recipes must not
        // deadlock when the child's pipe buffers fill.
        let (output, written) = thread::scope(|scope| {
            let writer = scope.spawn(move || stdin.write_all(input));
            let output = child.wait_with_output();
            (output, writer.join().expect("stdin writer did not panic"))
        });
        let output = output.map_err(|e| Error::Tool(format!("cannot collect output: {e}")))?;
        if output.status.success() {
            written.map_err(|e| Error::Tool(format!("cannot send input: {e}")))?;
        }
        Ok(output)
    }

    pub fn format(&self, input: &[u8]) -> Result<Option<Vec<u8>>, Error> {
        let output = self.run(input)?;
        if output.status.success() {
            return Ok(Some(output.stdout));
        }
        if output.status.code() == Some(1) {
            return Ok(None);
        }
        Err(Error::Tool(format!(
            "formatter failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}
