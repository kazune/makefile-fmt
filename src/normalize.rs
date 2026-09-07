//! File-level whitespace normalization that does not rewrite Make values.

use crate::{Edit, scan};

/// Remove leading blank lines and every blank line after the first in a
/// consecutive run. `define` bodies are deliberately excluded because their
/// blank lines are value bytes.
pub(crate) fn blank_line_edits(lines: &[scan::Line]) -> Vec<Edit> {
    let mut previous_was_blank = false;
    let mut before_content = true;
    let mut edits = Vec::new();
    for line in lines {
        if line.kind == scan::LineKind::Blank {
            if before_content || previous_was_blank {
                edits.push(Edit {
                    range: line.range.clone(),
                    replacement: Vec::new(),
                });
            }
            previous_was_blank = true;
        } else {
            before_content = false;
            previous_was_blank = false;
        }
    }
    edits
}

/// Give a non-empty file exactly one final line ending. Keep the style of its
/// last existing line ending, defaulting to LF when the source had none.
pub(crate) fn final_newline(mut source: Vec<u8>) -> Vec<u8> {
    if source.is_empty() {
        return source;
    }
    let ending = match source.iter().rposition(|&byte| byte == b'\n') {
        Some(index) if index > 0 && source[index - 1] == b'\r' => b"\r\n".as_slice(),
        _ => b"\n".as_slice(),
    };
    while source.ends_with(b"\r\n") || source.ends_with(b"\n") {
        let length = if source.ends_with(b"\r\n") { 2 } else { 1 };
        source.truncate(source.len() - length);
    }
    source.extend_from_slice(ending);
    source
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_newline_is_normalized_without_changing_its_style() {
        assert_eq!(final_newline(b"X".to_vec()), b"X\n");
        assert_eq!(final_newline(b"X\n\n".to_vec()), b"X\n");
        assert_eq!(final_newline(b"X\r\n\r\n".to_vec()), b"X\r\n");
        assert_eq!(final_newline(Vec::new()), b"");
    }
}
