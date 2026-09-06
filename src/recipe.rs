use crate::{Error, masking::Masked, scan::without_eol, shfmt::Shfmt};

struct Command {
    prefix: Vec<u8>,
    shell: Vec<u8>,
    eol: &'static [u8],
    final_newline: bool,
}

/// A lexical allow-list, not a shell parser. shfmt handles shell grammar.
/// Only unquoted backslash-newline pairs may cross physical recipe lines.
fn safe_shell(shell: &[u8]) -> bool {
    if std::str::from_utf8(shell).is_err()
        || shell
            .iter()
            .any(|b| b"`#".contains(b) || (b.is_ascii_control() && !b"\t\n".contains(b)))
        || shell.windows(2).any(|w| w == b"<<")
    {
        return false;
    }
    let mut quote = None;
    let mut i = 0;
    while i < shell.len() {
        let c = shell[i];
        if c == b'\n' && quote.is_some() {
            return false;
        }
        if c == b'\\' {
            let Some(&next) = shell.get(i + 1) else {
                return false;
            };
            if next == b'\n' && quote.is_some() {
                return false;
            }
            if quote != Some(b'\'') {
                i += 2;
                continue;
            }
        }
        if matches!(c, b'\'' | b'"') {
            if quote == Some(c) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(c);
            }
        }
        i += 1;
    }
    quote.is_none()
}

fn extract(raw: &[u8]) -> Option<Command> {
    let physical: Vec<_> = raw.split_inclusive(|&b| b == b'\n').collect();
    let first = *physical.first()?;
    let eol: &[u8] = if first.ends_with(b"\r\n") {
        b"\r\n"
    } else {
        b"\n"
    };
    let mut shell = Vec::new();
    let mut prefix = Vec::new();
    for (index, line) in physical.iter().enumerate() {
        if line.ends_with(b"\n") && !line.ends_with(eol) {
            return None;
        }
        if eol == b"\n" && line.ends_with(b"\r\n") {
            return None;
        }
        let mut body = without_eol(line);
        if index == 0 {
            body = body.strip_prefix(b"\t")?;
            let end = body
                .iter()
                .position(|b| !b" \t@-+".contains(b))
                .unwrap_or(body.len());
            prefix.extend_from_slice(&body[..end]);
            body = &body[end..];
        } else {
            // GNU Make removes exactly one leading TAB from a continuation.
            body = body.strip_prefix(b"\t").unwrap_or(body);
        }
        if index + 1 < physical.len() && !body.ends_with(b"\\") {
            return None;
        }
        shell.extend_from_slice(body);
        if line.ends_with(b"\n") {
            shell.push(b'\n');
        }
    }
    // A dangling continuation at EOF is not a complete command.
    if without_eol(raw).ends_with(b"\\") || !safe_shell(&shell) {
        return None;
    }
    Some(Command {
        prefix,
        shell,
        eol,
        final_newline: raw.ends_with(b"\n"),
    })
}

fn rebuild(command: &Command, formatted: &[u8]) -> Option<Vec<u8>> {
    if formatted.is_empty() || !formatted.ends_with(b"\n") || !safe_shell(formatted) {
        return None;
    }
    let lines: Vec<_> = formatted[..formatted.len() - 1]
        .split(|&b| b == b'\n')
        .collect();
    if lines
        .iter()
        .any(|line| line.is_empty() || line.ends_with(b"\\"))
    {
        return None;
    }
    let mut result = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        result.push(b'\t');
        if index == 0 {
            result.extend_from_slice(&command.prefix);
        }
        result.extend_from_slice(line);
        if index + 1 < lines.len() {
            result.extend_from_slice(b" \\");
            result.extend_from_slice(command.eol);
        } else if command.final_newline {
            result.extend_from_slice(command.eol);
        }
    }
    Some(result)
}

pub(crate) fn format(raw: &[u8], shfmt: &Shfmt) -> Result<Option<Vec<u8>>, Error> {
    let Some(command) = extract(raw) else {
        return Ok(None);
    };
    let Some(formatted) = format_shell(&command.shell, shfmt)? else {
        return Ok(None);
    };
    let Some(rebuilt) = rebuild(&command, &formatted) else {
        return Ok(None);
    };
    if rebuilt == raw {
        return Ok(None);
    }
    // Reconstructed Make continuations must round-trip to the same shfmt
    // result. Otherwise preserve the original command, including its layout.
    let Some(round_trip) = extract(&rebuilt) else {
        return Ok(None);
    };
    if format_shell(&round_trip.shell, shfmt)?.as_deref() != Some(formatted.as_slice()) {
        return Ok(None);
    }
    Ok(Some(rebuilt))
}

fn format_shell(shell: &[u8], shfmt: &Shfmt) -> Result<Option<Vec<u8>>, Error> {
    let Some(masked) = Masked::new(shell) else {
        return Ok(None);
    };
    if !safe_shell(&masked.shell) || !safe_shell(&masked.witness) {
        return Ok(None);
    }
    let Some(formatted) = shfmt.format(&masked.shell)? else {
        return Ok(None);
    };
    let witness = if masked.has_shell_dollars {
        let Some(witness) = shfmt.format(&masked.witness)? else {
            return Ok(None);
        };
        witness
    } else {
        formatted.clone()
    };
    Ok(masked.restore(&formatted, &witness))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexical_skip_boundaries() {
        for shell in [
            b"echo `date`\n".as_slice(),
            b"echo x # comment\n",
            b"cat <<EOF\n",
            b"echo 'unclosed\n",
            b"echo 'a\\\nb'\n",
            b"echo \"a\\\nb\"\n",
            b"echo 'a\nb'\n",
            b"echo \xff\n",
        ] {
            assert!(!safe_shell(shell), "accepted {shell:?}");
        }
        for shell in [
            b"echo 'a b'\n".as_slice(),
            b"echo a\\\nb\n",
            b"if true; then \\\necho yes; \\\nfi\n",
            b"printf '%s\\n' ok\n",
        ] {
            assert!(safe_shell(shell), "rejected {shell:?}");
        }
    }

    #[test]
    fn extraction_keeps_make_and_shell_boundaries() {
        let command = extract(b"\t@-+if true; then \\\r\n\t\techo yes; \\\r\nfi").unwrap();
        assert_eq!(command.prefix, b"@-+");
        assert_eq!(command.shell, b"if true; then \\\n\techo yes; \\\nfi");
        let rebuilt = rebuild(&command, b"if true; then\n\techo yes;\nfi;\n").unwrap();
        assert_eq!(
            rebuilt,
            b"\t@-+if true; then \\\r\n\t\techo yes; \\\r\n\tfi;"
        );
        assert!(extract(b"\techo hi \\\n").is_none());
    }
}
