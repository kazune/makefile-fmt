//! Conservative, byte-preserving formatting for the subset documented in MVP.md.

mod recipe;
pub mod scan;
mod shfmt;

use std::ops::Range;
use std::{ffi::OsStr, fmt};

#[derive(Debug)]
pub enum Error {
    Unsupported(scan::Unsupported),
    Tool(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(error) => error.fmt(f),
            Self::Tool(message) => write!(f, "shfmt: {message}"),
        }
    }
}

impl std::error::Error for Error {}

impl Error {
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Unsupported(_) => 2,
            Self::Tool(_) => 3,
        }
    }
}

/// Read-only transformation. No edits escape if safety or tool checks fail.
/// Success does not certify the external assumptions in MVP.md.
pub fn format(source: &[u8], shfmt_path: impl AsRef<OsStr>) -> Result<Vec<u8>, Error> {
    let lines = scan::scan(source).map_err(Error::Unsupported)?;
    let shfmt = shfmt::Shfmt::new(shfmt_path.as_ref())?;
    let mut edits = assignment_edits(source, &lines);
    for line in &lines {
        if line.kind == scan::LineKind::Recipe && line.format_safe {
            if let Some(replacement) = recipe::format(&source[line.range.clone()], &shfmt)? {
                edits.push(Edit {
                    range: line.range.clone(),
                    replacement,
                });
            }
        }
    }
    edits.sort_by_key(|edit| edit.range.start);
    Ok(apply_edits(source, &edits))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub range: Range<usize>,
    pub replacement: Vec<u8>,
}

/// Apply ordered, non-overlapping edits without regenerating untouched regions.
pub fn apply_edits(source: &[u8], edits: &[Edit]) -> Vec<u8> {
    let mut output = Vec::with_capacity(source.len());
    let mut cursor = 0;
    for edit in edits {
        assert!(cursor <= edit.range.start && edit.range.end <= source.len());
        assert!(edit.range.start <= edit.range.end);
        output.extend_from_slice(&source[cursor..edit.range.start]);
        output.extend_from_slice(&edit.replacement);
        cursor = edit.range.end;
    }
    output.extend_from_slice(&source[cursor..]);
    output
}

/// Only spacing around a literal, simple assignment is changed. RHS bytes,
/// including comments and trailing whitespace, are copied from the source.
pub fn assignment_edits(source: &[u8], lines: &[scan::Line]) -> Vec<Edit> {
    lines
        .iter()
        .filter_map(|line| {
            if line.kind != scan::LineKind::Assignment || !line.format_safe {
                return None;
            }
            let raw = &source[line.range.clone()];
            let body = scan::without_eol(raw);
            if body.contains(&b'\n') || body.ends_with(b"\\") || body.starts_with(b"\t") {
                return None;
            }
            let assignment = scan::assignment(body)?;
            let name = scan::trim(&body[..assignment.operator.start]);
            if name.is_empty()
                || !name
                    .iter()
                    .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(c))
            {
                return None;
            }
            let rhs = scan::trim_start(&body[assignment.operator.end..]);
            // With no value, keep the original suffix: there is no RHS content
            // boundary at which to distinguish leading and trailing whitespace.
            let mut replacement = name.to_vec();
            replacement.push(b' ');
            replacement.extend_from_slice(&body[assignment.operator.clone()]);
            if rhs.is_empty() {
                replacement.extend_from_slice(&body[assignment.operator.end..]);
            } else {
                replacement.push(b' ');
                replacement.extend_from_slice(rhs);
            }
            replacement.extend_from_slice(&raw[body.len()..]);
            (replacement != raw).then(|| Edit {
                range: line.range.clone(),
                replacement,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assignments_preserve_values_and_opaque_bytes() {
        let source = b"CC=gcc\r\nCFLAGS  :=   -O2  # note  \r\nEMPTY=   \r\nBYTES=\xff\r\nexport CC=gcc\r\ntarget: X=1\r\nX = one\\\r\n  two\r\ndefine FOO\r\nX=1\r\nendef\r\nunknown text\r\nLAST=x";
        let expected = b"CC = gcc\r\nCFLAGS := -O2  # note  \r\nEMPTY =   \r\nBYTES = \xff\r\nexport CC=gcc\r\ntarget: X=1\r\nX = one\\\r\n  two\r\ndefine FOO\r\nX=1\r\nendef\r\nunknown text\r\nLAST = x";
        let lines = scan::scan(source).unwrap();
        let output = apply_edits(source, &assignment_edits(source, &lines));
        assert_eq!(output, expected);
        let lines = scan::scan(&output).unwrap();
        assert!(assignment_edits(&output, &lines).is_empty());
    }

    #[test]
    fn longest_assignment_operators() {
        for op in ["=", ":=", "::=", ":::=", "?=", "+=", "!="] {
            let source = format!("VAR{op}  value  \n");
            let lines = scan::scan(source.as_bytes()).unwrap();
            let output = apply_edits(
                source.as_bytes(),
                &assignment_edits(source.as_bytes(), &lines),
            );
            assert_eq!(output, format!("VAR {op} value  \n").as_bytes());
        }
    }
}
