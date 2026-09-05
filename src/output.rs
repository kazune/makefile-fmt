use std::{
    fs::{self, Metadata, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Stage a complete result next to the resolved input, then atomically rename.
/// No source file is opened for writing during scanning or formatting.
pub fn replace(
    path: &Path,
    original: &[u8],
    replacement: &[u8],
    metadata: &Metadata,
) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() > 1 {
            return Err(io::Error::other(
                "refusing to replace a file with multiple hard links",
            ));
        }
    }
    let directory = path
        .parent()
        .ok_or_else(|| io::Error::other("input has no parent directory"))?;
    let (temporary, mut file) = (0..32)
        .find_map(|_| {
            let path = directory.join(format!(
                ".makefile-fmt.{}.{}.tmp",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(file) => Some(Ok((Temporary(path), file))),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => None,
                Err(e) => Some(Err(e)),
            }
        })
        .unwrap_or_else(|| Err(io::Error::other("cannot allocate temporary output")))?;
    file.write_all(replacement)?;
    file.set_permissions(metadata.permissions())?;
    file.sync_all()?;
    drop(file);
    if fs::read(path)? != original {
        return Err(io::Error::other(
            "input changed while formatting; refusing to overwrite",
        ));
    }
    fs::rename(&temporary.0, path)?;
    Ok(())
}

/// A single unified hunk with three lines of outer context, without an
/// unbounded LCS matrix for large input files.
pub fn diff(path: &Path, before: &[u8], after: &[u8]) -> Vec<u8> {
    if before == after {
        return Vec::new();
    }
    let old: Vec<_> = before.split_inclusive(|&b| b == b'\n').collect();
    let new: Vec<_> = after.split_inclusive(|&b| b == b'\n').collect();
    let common_start = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let common_end = old[common_start..]
        .iter()
        .rev()
        .zip(new[common_start..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let start = common_start.saturating_sub(3);
    let old_end = (old.len() - common_end + 3).min(old.len());
    let new_end = (new.len() - common_end + 3).min(new.len());
    let old_start = if old_end == start { start } else { start + 1 };
    let new_start = if new_end == start { start } else { start + 1 };
    let name = path.to_string_lossy();
    let label = if name.contains(['\n', '\r', '\t', '"', '\\']) {
        format!("{name:?}")
    } else {
        name.into_owned()
    };
    let mut result = format!(
        "--- {label}\n+++ {label}\n@@ -{old_start},{} +{new_start},{} @@\n",
        old_end - start,
        new_end - start
    )
    .into_bytes();
    let mut emit = |marker, line: &[u8]| {
        result.push(marker);
        result.extend_from_slice(line);
        if !line.ends_with(b"\n") {
            result.extend_from_slice(b"\n\\ No newline at end of file\n");
        }
    };
    for line in &old[start..common_start] {
        emit(b' ', line);
    }
    for line in &old[common_start..old.len() - common_end] {
        emit(b'-', line);
    }
    for line in &new[common_start..new.len() - common_end] {
        emit(b'+', line);
    }
    for line in &old[old.len() - common_end..old_end] {
        emit(b' ', line);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unified_diff_keeps_context_and_missing_newline_markers() {
        assert_eq!(diff(Path::new("Makefile"), b"# header\nX=1", b"# header\nX = 1"), b"--- Makefile\n+++ Makefile\n@@ -1,2 +1,2 @@\n # header\n-X=1\n\\ No newline at end of file\n+X = 1\n\\ No newline at end of file\n");
        assert!(diff(Path::new("Makefile"), b"x", b"x").is_empty());
    }
}
