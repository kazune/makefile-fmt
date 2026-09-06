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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiffTag {
    Equal,
    Delete,
    Insert,
}

#[derive(Clone, Copy, Debug)]
struct DiffLine<'a> {
    tag: DiffTag,
    line: &'a [u8],
    old_before: usize,
    new_before: usize,
}

fn line_ops<'a>(old: &[&'a [u8]], new: &[&'a [u8]]) -> Vec<DiffLine<'a>> {
    let n = old.len();
    let m = new.len();
    let limit = n + m;
    let offset = limit + 1;
    let mut frontier = vec![0isize; offset * 2 + 1];
    let mut trace = Vec::new();
    let mut distance = 0;
    'search: for d in 0..=limit {
        for diagonal in (-(d as isize)..=d as isize).step_by(2) {
            let index = (diagonal + offset as isize) as usize;
            let mut x = if diagonal == -(d as isize)
                || (diagonal != d as isize && frontier[index - 1] < frontier[index + 1])
            {
                frontier[index + 1]
            } else {
                frontier[index - 1] + 1
            };
            let mut y = x - diagonal;
            while x < n as isize && y < m as isize && old[x as usize] == new[y as usize] {
                x += 1;
                y += 1;
            }
            frontier[index] = x;
            if x >= n as isize && y >= m as isize {
                distance = d;
                break 'search;
            }
        }
        trace.push(frontier.clone());
    }

    let mut reversed = Vec::with_capacity(n + m);
    let (mut x, mut y) = (n as isize, m as isize);
    for d in (1..=distance).rev() {
        let diagonal = x - y;
        let previous = &trace[d - 1];
        let index = (diagonal + offset as isize) as usize;
        let previous_diagonal = if diagonal == -(d as isize)
            || (diagonal != d as isize && previous[index - 1] < previous[index + 1])
        {
            diagonal + 1
        } else {
            diagonal - 1
        };
        let previous_x = previous[(previous_diagonal + offset as isize) as usize];
        let previous_y = previous_x - previous_diagonal;
        while x > previous_x && y > previous_y {
            reversed.push((
                DiffTag::Equal,
                old[(x - 1) as usize],
                (x - 1) as usize,
                (y - 1) as usize,
            ));
            x -= 1;
            y -= 1;
        }
        if x == previous_x {
            reversed.push((
                DiffTag::Insert,
                new[(y - 1) as usize],
                previous_x as usize,
                (y - 1) as usize,
            ));
            y -= 1;
        } else {
            reversed.push((
                DiffTag::Delete,
                old[(x - 1) as usize],
                (x - 1) as usize,
                previous_y as usize,
            ));
            x -= 1;
        }
    }
    while x > 0 && y > 0 {
        reversed.push((
            DiffTag::Equal,
            old[(x - 1) as usize],
            (x - 1) as usize,
            (y - 1) as usize,
        ));
        x -= 1;
        y -= 1;
    }
    while x > 0 {
        reversed.push((DiffTag::Delete, old[(x - 1) as usize], (x - 1) as usize, 0));
        x -= 1;
    }
    while y > 0 {
        reversed.push((DiffTag::Insert, new[(y - 1) as usize], 0, (y - 1) as usize));
        y -= 1;
    }
    reversed.reverse();
    let mut old_before = 0;
    let mut new_before = 0;
    reversed
        .into_iter()
        .map(|(tag, line, _, _)| {
            let result = DiffLine {
                tag,
                line,
                old_before,
                new_before,
            };
            match tag {
                DiffTag::Equal => {
                    old_before += 1;
                    new_before += 1;
                }
                DiffTag::Delete => old_before += 1,
                DiffTag::Insert => new_before += 1,
            }
            result
        })
        .collect()
}

/// Produce normal line-oriented unified diff hunks with three lines of
/// context. Matching lines are retained as context instead of being emitted
/// as both a deletion and an insertion.
pub fn diff(path: &Path, before: &[u8], after: &[u8]) -> Vec<u8> {
    if before == after {
        return Vec::new();
    }
    let old: Vec<_> = before.split_inclusive(|&b| b == b'\n').collect();
    let new: Vec<_> = after.split_inclusive(|&b| b == b'\n').collect();
    let changes = line_ops(&old, &new);
    let mut hunks = Vec::<(usize, usize)>::new();
    for (index, change) in changes.iter().enumerate() {
        if change.tag == DiffTag::Equal {
            continue;
        }
        let start = index.saturating_sub(3);
        let end = (index + 4).min(changes.len());
        if let Some((_, current_end)) = hunks.last_mut() {
            if start <= *current_end {
                *current_end = (*current_end).max(end);
                continue;
            }
        }
        hunks.push((start, end));
    }
    if hunks.is_empty() {
        return Vec::new();
    }
    let name = path.to_string_lossy();
    let label = if name.contains(['\n', '\r', '\t', '"', '\\']) {
        format!("{name:?}")
    } else {
        name.into_owned()
    };
    let mut result = format!("--- {label}\n+++ {label}\n").into_bytes();
    for (start, end) in hunks {
        let old_count = changes[start..end]
            .iter()
            .filter(|c| c.tag != DiffTag::Insert)
            .count();
        let new_count = changes[start..end]
            .iter()
            .filter(|c| c.tag != DiffTag::Delete)
            .count();
        let old_start = changes[start].old_before + usize::from(old_count > 0);
        let new_start = changes[start].new_before + usize::from(new_count > 0);
        result.extend_from_slice(
            format!("@@ -{old_start},{old_count} +{new_start},{new_count} @@\n").as_bytes(),
        );
        for change in &changes[start..end] {
            let marker = match change.tag {
                DiffTag::Equal => b' ',
                DiffTag::Delete => b'-',
                DiffTag::Insert => b'+',
            };
            result.push(marker);
            result.extend_from_slice(change.line);
            if !change.line.ends_with(b"\n") {
                result.extend_from_slice(b"\n\\ No newline at end of file\n");
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unified_diff_keeps_context_and_missing_newline_markers() {
        assert_eq!(diff(Path::new("Makefile"), b"# header\nX=1", b"# header\nX = 1"), b"--- Makefile\n+++ Makefile\n@@ -1,2 +1,2 @@\n # header\n-X=1\n\\ No newline at end of file\n+X = 1\n\\ No newline at end of file\n");
        assert_eq!(diff(Path::new("Makefile"), b"A=1\nKEEP = ok\nB=2\n", b"A = 1\nKEEP = ok\nB = 2\n"), b"--- Makefile\n+++ Makefile\n@@ -1,3 +1,3 @@\n-A=1\n+A = 1\n KEEP = ok\n-B=2\n+B = 2\n");
        assert!(diff(Path::new("Makefile"), b"x", b"x").is_empty());
    }
}
