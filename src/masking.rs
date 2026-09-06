//! An unevaluated Make-dollar token tape and a provenance witness for `$$`.

struct Token {
    marker: Vec<u8>,
    original: Vec<u8>,
    shell_dollar: bool,
}

pub(crate) struct Masked {
    pub shell: Vec<u8>,
    pub witness: Vec<u8>,
    pub has_shell_dollars: bool,
    namespace: Vec<u8>,
    tokens: Vec<Token>,
}

fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|part| part == needle)
}

/// Keep nested references opaque. Quotes/backslashes do not escape Make's
/// delimiters. Ambiguous mixed nesting and multiline references are skipped.
fn expression_end(source: &[u8], start: usize) -> Option<usize> {
    let mut stack = vec![source[start + 1]];
    let mut i = start + 2;
    while let Some(&open) = stack.last() {
        let c = *source.get(i)?;
        if matches!(c, b'\r' | b'\n') {
            return None;
        }
        if c == b'$' && source.get(i + 1).is_some_and(|c| matches!(c, b'(' | b'{')) {
            stack.push(source[i + 1]);
            i += 2;
            continue;
        }
        if c == open {
            stack.push(open);
        } else if c == if open == b'(' { b')' } else { b'}' } {
            stack.pop();
        } else if matches!(c, b')' | b'}')
            && stack
                .iter()
                .any(|&open| c == if open == b'(' { b')' } else { b'}' })
        {
            return None;
        }
        i += 1;
    }
    Some(i)
}

impl Masked {
    pub fn new(source: &[u8]) -> Option<Self> {
        let namespace = (0usize..)
            .map(|salt| format!("__MAKEFMT_{salt}_").into_bytes())
            .find(|prefix| !contains(source, prefix))?;
        let mut masked = Self {
            shell: Vec::new(),
            witness: Vec::new(),
            has_shell_dollars: false,
            namespace,
            tokens: Vec::new(),
        };
        let mut i = 0;
        while i < source.len() {
            if source[i] != b'$' {
                masked.shell.push(source[i]);
                masked.witness.push(source[i]);
                i += 1;
                continue;
            }
            let next = *source.get(i + 1)?;
            let end = match next {
                b'$' | b'@' | b'<' | b'^' | b'?' | b'*' | b'%' => i + 2,
                b'(' | b'{' => expression_end(source, i)?,
                _ => return None,
            };
            let mut marker = masked.namespace.clone();
            marker.extend_from_slice(format!("{}__", masked.tokens.len()).as_bytes());
            let shell_dollar = next == b'$';
            if shell_dollar {
                masked.shell.push(b'$');
                masked.has_shell_dollars = true;
            } else {
                masked.shell.extend_from_slice(&marker);
            }
            masked.witness.extend_from_slice(&marker);
            masked.tokens.push(Token {
                marker,
                original: source[i..end].to_vec(),
                shell_dollar,
            });
            i = end;
        }
        Some(masked)
    }

    /// Reconstruct from an independently formatted witness. After replacing
    /// only dollar markers with '$', it must equal the actual shell output.
    /// This gives each output dollar an origin, including adjacent dollars,
    /// literal dollars in quotes, and escaped dollars. No output-wide dollar
    /// doubling is performed. The witness may fail to parse: callers skip it.
    pub fn restore(&self, shell_output: &[u8], witness_output: &[u8]) -> Option<Vec<u8>> {
        if witness_output.contains(&b'$') {
            return None;
        }
        if witness_output
            .windows(self.namespace.len())
            .filter(|w| *w == self.namespace)
            .count()
            != self.tokens.len()
        {
            return None;
        }
        let mut positions = Vec::new();
        for token in &self.tokens {
            let mut matches = witness_output
                .windows(token.marker.len())
                .enumerate()
                .filter(|(_, w)| *w == token.marker);
            let (start, _) = matches.next()?;
            if matches.next().is_some() {
                return None;
            }
            positions.push((start, token));
        }
        positions.sort_by_key(|(start, _)| *start);
        let mut shell = Vec::new();
        let mut restored = Vec::new();
        let mut cursor = 0;
        for (start, token) in positions {
            if start < cursor {
                return None;
            }
            shell.extend_from_slice(&witness_output[cursor..start]);
            restored.extend_from_slice(&witness_output[cursor..start]);
            if token.shell_dollar {
                shell.push(b'$');
            } else {
                shell.extend_from_slice(&token.marker);
            }
            restored.extend_from_slice(&token.original);
            cursor = start + token.marker.len();
        }
        shell.extend_from_slice(&witness_output[cursor..]);
        restored.extend_from_slice(&witness_output[cursor..]);
        (shell == shell_output).then_some(restored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_list_is_read_left_to_right() {
        for source in [
            b"$(CC) ${FLAGS} $@ $< $^ $? $* $%".as_slice(),
            b"echo \"$$HOME\"",
            b"echo $$$$",
            b"echo $$$@",
        ] {
            let masked = Masked::new(source).unwrap();
            assert_eq!(
                masked.restore(&masked.shell, &masked.witness).unwrap(),
                source
            );
        }
        for source in [
            b"$".as_slice(),
            b"$$$",
            b"$$$$$",
            b"$|",
            b"$0",
            b"$x",
            b"$(unclosed",
            b"${unclosed",
            b"$(outer ${inner)",
            b"$(multi\\\nline)",
        ] {
            assert!(Masked::new(source).is_none(), "accepted {source:?}");
        }
        assert_eq!(Masked::new(b"echo $$$$").unwrap().shell, b"echo $$");
        assert_eq!(
            Masked::new(b"echo \"$$HOME\"").unwrap().shell,
            b"echo \"$HOME\""
        );
    }

    #[test]
    fn nested_expressions_and_quoted_fragments_are_opaque() {
        let source =
            b"echo pre$(subst a,b,$(NAME))post '${VAR_$(KEY)}' \"$(call f,${X})\" $(value $x)";
        let masked = Masked::new(source).unwrap();
        assert_eq!(masked.tokens.len(), 4);
        assert_eq!(
            masked.restore(&masked.shell, &masked.witness).unwrap(),
            source
        );
    }

    #[test]
    fn markers_cannot_collide_with_original_text() {
        let source = b"echo __MAKEFMT_0_0__ __MAKEFMT_1_ $(X) $(X) $$HOME";
        let masked = Masked::new(source).unwrap();
        assert_eq!(masked.namespace, b"__MAKEFMT_2_");
        assert_eq!(
            masked.restore(&masked.shell, &masked.witness).unwrap(),
            source
        );
    }

    #[test]
    fn corrupted_or_untracked_output_is_not_restored() {
        let masked = Masked::new(b"echo $(X) $$HOME").unwrap();
        assert!(masked.restore(&masked.shell, b"echo HOME").is_none());
        let mut duplicate = masked.witness.clone();
        duplicate.extend_from_slice(&masked.tokens[0].marker);
        assert!(masked.restore(&masked.shell, &duplicate).is_none());
        let mut malformed = masked.witness.clone();
        malformed[5] = b'x';
        assert!(masked.restore(&masked.shell, &malformed).is_none());
        let mut unknown = masked.witness.clone();
        unknown.extend_from_slice(b" __MAKEFMT_0_unknown__");
        assert!(masked.restore(&masked.shell, &unknown).is_none());
        let mut untracked = masked.witness.clone();
        untracked.extend_from_slice(b" $NEW");
        assert!(masked.restore(&masked.shell, &untracked).is_none());
        let mut shell = masked.shell.clone();
        shell.extend_from_slice(b" $NEW");
        assert!(masked.restore(&shell, &masked.witness).is_none());
    }
}
