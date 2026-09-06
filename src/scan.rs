//! Minimal structural recognition; all spans refer to the original bytes.

use std::{fmt, ops::Range};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Blank,
    Comment,
    Assignment,
    Rule,
    Recipe,
    Directive,
    Conditional,
    DefineStart,
    DefineBody,
    DefineEnd,
    Unknown,
}

/// A logical Make line (possibly several physical lines).
#[derive(Debug, Clone)]
pub struct Line {
    pub number: usize,
    pub range: Range<usize>,
    pub kind: LineKind,
    pub format_safe: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsupported {
    pub line: usize,
    pub feature: String,
}

impl fmt::Display for Unsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: unsupported GNU Make feature: {}",
            self.line, self.feature
        )
    }
}

impl std::error::Error for Unsupported {}

pub(crate) fn trim_start(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    &bytes[start..]
}

pub(crate) fn trim(bytes: &[u8]) -> &[u8] {
    let bytes = trim_start(bytes);
    let end = bytes
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(0, |i| i + 1);
    &bytes[..end]
}

pub(crate) fn without_eol(bytes: &[u8]) -> &[u8] {
    bytes
        .strip_suffix(b"\r\n")
        .or_else(|| bytes.strip_suffix(b"\n"))
        .unwrap_or(bytes)
}

fn continued(bytes: &[u8]) -> bool {
    bytes.ends_with(b"\n")
        && without_eol(bytes)
            .iter()
            .rev()
            .take_while(|&&b| b == b'\\')
            .count()
            % 2
            == 1
}

fn fold_make_line(bytes: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    let mut continuation = false;
    for physical in bytes.split_inclusive(|&b| b == b'\n') {
        let mut body = without_eol(physical);
        if continuation {
            body = trim_start(body);
        }
        continuation = continued(physical);
        if continuation {
            body = &body[..body.len() - 1];
            let end = body
                .iter()
                .rposition(|b| !b.is_ascii_whitespace())
                .map_or(0, |i| i + 1);
            result.extend_from_slice(&body[..end]);
            result.push(b' ');
        } else {
            result.extend_from_slice(body);
        }
    }
    result
}

/// Positions outside escaped characters and balanced Make references.
fn syntax_positions(bytes: &[u8]) -> Vec<usize> {
    let mut positions = Vec::new();
    let mut stack = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'\\' {
            i += 2;
            continue;
        }
        if c == b'$' && i + 1 < bytes.len() && matches!(bytes[i + 1], b'(' | b'{') {
            stack.push(if bytes[i + 1] == b'(' { b')' } else { b'}' });
            i += 2;
            continue;
        }
        if let Some(&close) = stack.last() {
            if c == close {
                stack.pop();
            } else if (c == b'(' && close == b')') || (c == b'{' && close == b'}') {
                stack.push(close);
            }
        } else {
            positions.push(i);
            if c == b'#' {
                break;
            }
        }
        i += 1;
    }
    positions
}

/// Stricter than the historical fatal scanner: formatting needs proof that
/// references close, not just a best-effort list of visible punctuation.
/// Values are never expanded. Escaped header text remains conservative.
fn header_positions(bytes: &[u8]) -> Option<Vec<usize>> {
    if std::str::from_utf8(bytes).is_err() {
        return None;
    }
    let mut positions = Vec::new();
    let mut stack = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(&close) = stack.last() {
            if c == b'$' && bytes.get(i + 1).is_some_and(|b| matches!(b, b'(' | b'{')) {
                stack.push(if bytes[i + 1] == b'(' { b')' } else { b'}' });
                i += 2;
                continue;
            }
            if c == close {
                stack.pop();
            } else if (c == b'(' && close == b')') || (c == b'{' && close == b'}') {
                stack.push(close);
            } else if matches!(c, b')' | b'}') && stack.contains(&c) {
                return None;
            }
        } else {
            match c {
                b'#' => break,
                // Do not infer quoting of Make punctuation or dollars from
                // shell escape rules. Continuations were already folded.
                b'\\' => return None,
                b'$' => {
                    match *bytes.get(i + 1)? {
                        b'(' => stack.push(b')'),
                        b'{' => stack.push(b'}'),
                        b'$'
                        | b'@'
                        | b'<'
                        | b'^'
                        | b'?'
                        | b'*'
                        | b'%'
                        | b'|'
                        | b'a'..=b'z'
                        | b'A'..=b'Z'
                        | b'0'..=b'9'
                        | b'_' => {}
                        _ => return None,
                    }
                    i += 2;
                    continue;
                }
                _ if c.is_ascii_control() && c != b'\t' => return None,
                _ => positions.push(i),
            }
        }
        i += 1;
    }
    stack.is_empty().then_some(positions)
}

/// Header bytes are never rewritten. Only a single literal rule delimiter
/// grants formatting permission to the following TAB recipes.
fn recipe_header_safe(bytes: &[u8], colon: usize) -> Option<bool> {
    let positions = header_positions(bytes)?;
    let colons: Vec<_> = positions
        .iter()
        .copied()
        .filter(|&i| bytes[i] == b':')
        .collect();
    Some(
        colons == [colon]
            && !trim(&bytes[..colon]).is_empty()
            && !positions.iter().any(|&i| b";&=".contains(&bytes[i])),
    )
}

fn uncomment(bytes: &[u8]) -> &[u8] {
    let end = syntax_positions(bytes)
        .into_iter()
        .find(|&i| bytes[i] == b'#')
        .unwrap_or(bytes.len());
    &bytes[..end]
}

#[derive(Debug)]
pub(crate) struct Assignment {
    pub operator: Range<usize>,
}

pub(crate) fn assignment(bytes: &[u8]) -> Option<Assignment> {
    for i in syntax_positions(bytes) {
        for operator in [b":::=".as_slice(), b"::=", b":=", b"?=", b"+=", b"!=", b"="] {
            if bytes[i..].starts_with(operator) {
                let name = trim(unmodified(trim(&bytes[..i])));
                // Spaces outside Make references separate words, not parts
                // of a literal variable name. In particular, '=' in an
                // ifeq/ifneq argument must not turn it into an assignment.
                if name.is_empty()
                    || syntax_positions(name)
                        .into_iter()
                        .any(|p| name[p].is_ascii_whitespace())
                {
                    return None;
                }
                return Some(Assignment {
                    operator: i..i + operator.len(),
                });
            }
        }
        if matches!(bytes[i], b':' | b';' | b'#') {
            return None;
        }
    }
    None
}

fn keyword<'a>(bytes: &'a [u8], word: &[u8]) -> Option<&'a [u8]> {
    let rest = bytes.strip_prefix(word)?;
    (rest.is_empty() || rest[0].is_ascii_whitespace()).then(|| trim_start(rest))
}

fn unmodified(mut bytes: &[u8]) -> &[u8] {
    loop {
        let next = [b"override".as_slice(), b"export", b"private", b"unexport"]
            .into_iter()
            .find_map(|word| keyword(bytes, word));
        match next {
            Some(rest) if !rest.is_empty() => bytes = rest,
            _ => return bytes,
        }
    }
}

fn define_name(bytes: &[u8]) -> Option<&[u8]> {
    let rest = keyword(bytes, b"define")?;
    // `define = value` defines a variable named "define", not a block.
    if assignment(bytes).is_some_and(|a| trim(&bytes[..a.operator.start]) == b"define") {
        return None;
    }
    Some(rest)
}

const VARIABLES: [&[u8]; 3] = [b"SHELL", b".SHELLFLAGS", b".RECIPEPREFIX"];
const SPECIAL: [&[u8]; 5] = [
    b".ONESHELL",
    b".POSIX",
    b"SHELL",
    b".SHELLFLAGS",
    b".RECIPEPREFIX",
];

fn fail(line: usize, feature: &[u8]) -> Unsupported {
    Unsupported {
        line,
        feature: String::from_utf8_lossy(feature).into_owned(),
    }
}

fn check_assignment(bytes: &[u8], line: usize) -> Result<(), Unsupported> {
    if let Some(a) = assignment(bytes) {
        let name = trim(unmodified(trim(&bytes[..a.operator.start])));
        if VARIABLES.contains(&name) {
            return Err(fail(line, name));
        }
        let rhs = trim(&bytes[a.operator.end..]);
        if SPECIAL.contains(&rhs) {
            return Err(fail(
                line,
                format!("literal alias to {}", String::from_utf8_lossy(rhs)).as_bytes(),
            ));
        }
    }
    Ok(())
}

fn explicit_eval(bytes: &[u8]) -> Option<usize> {
    for i in 0..bytes.len().saturating_sub(1) {
        if bytes[i] != b'$' || !matches!(bytes[i + 1], b'(' | b'{') {
            continue;
        }
        let rest = trim_start(&bytes[i + 2..]);
        if let Some(after) = rest.strip_prefix(b"eval")
            && (after
                .first()
                .is_some_and(|c| c.is_ascii_whitespace() || matches!(c, b')' | b'}'))
                || after.starts_with(b"\\\n")
                || after.starts_with(b"\\\r\n"))
        {
            return Some(i);
        }
    }
    None
}

/// Scan before generating any edits. A conditional region remains opaque for
/// formatting; both branches are nevertheless inspected for fatal features.
pub fn scan(source: &[u8]) -> Result<Vec<Line>, Unsupported> {
    if let Some(at) = explicit_eval(source) {
        return Err(fail(
            source[..at].iter().filter(|&&b| b == b'\n').count() + 1,
            b"eval",
        ));
    }
    let physical: Vec<_> = source.split_inclusive(|&b| b == b'\n').collect();
    let mut lines = Vec::new();
    let mut offset = 0;
    let mut i = 0;
    let mut define_depth = 0usize;
    // (rule context on entry, all completed branches have a rule, has else).
    let mut conditionals = Vec::new();
    let mut after_rule = false;
    let mut safe_rule = false;
    while i < physical.len() {
        let start = offset;
        let number = i + 1;
        let is_recipe = define_depth == 0 && after_rule && physical[i].starts_with(b"\t");
        loop {
            offset += physical[i].len();
            let more = continued(physical[i]);
            i += 1;
            if !more || i == physical.len() {
                break;
            }
        }
        let raw = &source[start..offset];
        let folded = fold_make_line(raw);
        let text = trim(uncomment(&folded));
        let clean = unmodified(text);
        let mut format_safe = conditionals.is_empty();
        let kind;
        if define_depth > 0 {
            // TAB-prefixed text in a define is literal recipe text, including
            // apparent define/endef keywords. Inside the body GNU Make 4.4.1
            // counts bare keywords, even `define = value`, without parsing
            // assignments, modifiers, or stripping comments from the line.
            let body = trim_start(&folded);
            if !raw.starts_with(b"\t") && keyword(body, b"define").is_some() {
                define_depth += 1;
            } else if !raw.starts_with(b"\t") && keyword(body, b"endef").is_some() {
                define_depth -= 1;
            }
            kind = if define_depth == 0 {
                LineKind::DefineEnd
            } else {
                LineKind::DefineBody
            };
            format_safe = false;
        } else if is_recipe {
            kind = LineKind::Recipe;
            format_safe &= safe_rule;
        } else if text.is_empty() {
            kind = if trim_start(&folded).starts_with(b"#") {
                LineKind::Comment
            } else {
                LineKind::Blank
            };
        } else if let Some(name) = define_name(clean) {
            let name = assignment(name).map_or(name, |a| trim(&name[..a.operator.start]));
            if VARIABLES.contains(&name) {
                return Err(fail(number, name));
            }
            define_depth = 1;
            after_rule = false;
            kind = LineKind::DefineStart;
            format_safe = false;
        } else if assignment(text).is_some() {
            check_assignment(text, number)?;
            kind = LineKind::Assignment;
            after_rule = false;
        } else if [b"ifdef".as_slice(), b"ifndef", b"ifeq", b"ifneq"]
            .into_iter()
            .any(|w| keyword(text, w).is_some())
        {
            conditionals.push((after_rule, true, false));
            kind = LineKind::Conditional;
            safe_rule = false;
            format_safe = false;
        } else if keyword(text, b"else").is_some() || keyword(text, b"endif").is_some() {
            if keyword(text, b"endif").is_some() {
                if let Some((entry, branches, has_else)) = conditionals.pop() {
                    after_rule &= branches && (has_else || entry);
                }
            } else if let Some((entry, branches, has_else)) = conditionals.last_mut() {
                *branches &= after_rule;
                after_rule = *entry;
                // An `else if...` chain may still have an implicit empty branch.
                *has_else |= keyword(text, b"else").is_some_and(|tail| tail.is_empty());
            }
            kind = LineKind::Conditional;
            safe_rule = false;
            format_safe = false;
        } else if let Some(word) = [
            b"include".as_slice(),
            b"-include",
            b"sinclude",
            b"load",
            b"-load",
        ]
        .into_iter()
        .find(|w| keyword(text, w).is_some())
        {
            return Err(fail(number, word));
        } else if let Some(colon) = syntax_positions(text)
            .into_iter()
            .find(|&p| text[p] == b':')
        {
            let targets = trim(&text[..colon]);
            let literal_targets = targets.strip_suffix(b"&").unwrap_or(targets);
            for target in literal_targets.split(|b| b.is_ascii_whitespace()) {
                if [b".ONESHELL".as_slice(), b".POSIX"].contains(&target) {
                    return Err(fail(number, target));
                }
            }
            let rest = trim_start(&text[colon + 1..]);
            let variable_part = trim_start(rest.strip_prefix(b":").unwrap_or(rest));
            check_assignment(variable_part, number)?;
            let header_safe = recipe_header_safe(trim(&folded), colon);
            kind = if header_safe.is_some() {
                LineKind::Rule
            } else {
                LineKind::Unknown
            };
            after_rule = assignment(variable_part).is_none();
            // Keep structural fatal recognition separate from permission to
            // format recipes. Header references may name files/lists, but the
            // supported subset forbids them from generating Make grammar.
            safe_rule = format_safe
                // A TAB line after an ambiguous header must not establish a
                // new formatting context from punctuation in shell text.
                && !raw.starts_with(b"\t")
                && header_safe == Some(true)
                && !without_eol(raw).ends_with(b"\\")
                && after_rule;
        } else {
            kind = if [b"export".as_slice(), b"unexport", b"undefine"]
                .into_iter()
                .any(|w| keyword(clean, w).is_some())
            {
                LineKind::Directive
            } else {
                LineKind::Unknown
            };
            if let Some(name) = keyword(clean, b"undefine")
                && VARIABLES.contains(&trim(name))
            {
                return Err(fail(number, trim(name)));
            }
            after_rule = false;
            safe_rule = false;
            format_safe = false;
        }
        lines.push(Line {
            number,
            range: start..offset,
            kind,
            format_safe,
        });
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_global_features_and_aliases() {
        for input in [
            ".ONESHELL:",
            ".ONESHELL&:",
            ".POSIX&::",
            ".POSIX: x",
            "x .ONESHELL: y",
            "SHELL=/bin/sh",
            "override SHELL := sh",
            "foo: private SHELL != echo sh",
            "foo:: override SHELL := /bin/sh",
            "foo:: .SHELLFLAGS = -ec",
            "define SHELL",
            "override define .SHELLFLAGS :=",
            ".RECIPEPREFIX=>",
            ".SHELLFLAGS=-ec",
            "include x",
            "-include x",
            "sinclude x",
            "load x",
            "-load x",
            "X = .ONESHELL",
            "X := .POSIX # comment",
            "NAME = SHELL",
            "NAME = .SHELLFLAGS",
            "NAME = .RECIPEPREFIX",
            "undefine SHELL",
            "override undefine .SHELLFLAGS",
        ] {
            assert!(scan(input.as_bytes()).is_err(), "missed {input:?}");
        }
    }

    #[test]
    fn eval_is_context_independent() {
        for context in [
            "# $(eval x)\n",
            "foo:\n\t${eval SHELL=x}\n",
            "define FOO\n$(eval .ONESHELL:)\nendef\n",
            "X = $(eval\\\n x)\n",
        ] {
            assert_eq!(scan(context.as_bytes()).unwrap_err().feature, "eval");
        }
        assert!(scan(b"F = eval\n$($(F) ...)\nX = $(evaluate)\n").is_ok());
    }

    #[test]
    fn respects_structural_context_and_preserves_spans() {
        let input = b"# .ONESHELL:\nfoo:\n\techo SHELL=x\n\techo hi \\\ninclude literal\ndefine FOO\ninclude foo.mk\n.ONESHELL:\ndefine BAR\nSHELL=x\nendef\nendef\n$(OBJS): common.h\nHELP = use your SHELL to run commands\n";
        let lines = scan(input).unwrap();
        let joined: Vec<u8> = lines
            .iter()
            .flat_map(|l| input[l.range.clone()].iter().copied())
            .collect();
        assert_eq!(joined, input);
        assert_eq!(lines[3].kind, LineKind::Recipe);
        assert_eq!(lines[4].number, 6);
    }

    #[test]
    fn checks_both_conditional_branches() {
        for input in [
            "ifeq (0,1)\n.ONESHELL:\nendif\n",
            "ifeq (0,1)\nfoo:\nelse\nSHELL=x\nendif\n",
        ] {
            assert!(scan(input.as_bytes()).is_err());
        }
        let lines = scan(b"ifeq (0,1)\nX=1\nelse\nX=2\nendif\nY=3\n").unwrap();
        assert!(!lines[1].format_safe);
        assert!(!lines[3].format_safe);
        assert!(lines[5].format_safe);
        let lines = scan(b"ifeq (a=b,a=b)\nX=1\nelse\nX=2\nendif\nY=3\n").unwrap();
        assert_eq!(lines[0].kind, LineKind::Conditional);
        assert!(!lines[1].format_safe);
        assert!(!lines[3].format_safe);
        assert!(lines[5].format_safe);
    }

    #[test]
    fn continuations_are_not_new_make_statements() {
        assert!(scan(b"X = text \\\nSHELL=x\n# note \\\ninclude ignored\n").is_ok());
        assert!(scan(b"SHELL \\\n := sh\n").is_err());
        assert!(scan(b"X = \\\n .ONESHELL\n").is_err());
        assert!(scan(b"foo \\\n .ONESHELL:\n").is_err());
    }

    #[test]
    fn assignments_and_expansions_do_not_become_directives() {
        assert!(
            scan(
                b"include = value\nSHELL_HELP = x\nX = text SHELL\n$(OBJS): common.h\n\techo hi\n"
            )
            .is_ok()
        );
        assert!(scan(b"foo: X = SHELL\n").is_err());
        assert!(scan(b"foo: ; echo SHELL=x\n").is_ok());
        assert!(scan(b"define = value\nSHELL=x\n").is_err());
    }

    #[test]
    fn header_ownership_requires_balanced_structural_delimiters() {
        for header in [
            "$(OUTDIR):",
            "${OUTDIR}/%: %.c | $(OUTDIR)",
            "$(OBJS:%.c=%.o): ${DEPS}",
            "$(subst :;=,x,$(NAME)): $(call deps,${KEY})",
            "foo: # $(unclosed : ; = &",
            "foo \\\nbar: dep \\\n other",
        ] {
            let input = format!("{header}\n\techo hi\n");
            let lines = scan(input.as_bytes()).unwrap();
            assert_eq!(lines[0].kind, LineKind::Rule, "{header}");
            assert_eq!(lines[1].kind, LineKind::Recipe, "{header}");
            assert!(lines[1].format_safe, "{header}");
            assert_eq!(
                &input.as_bytes()[lines[0].range.clone()],
                format!("{header}\n").as_bytes()
            );
        }
        for header in [
            "foo:: dep",
            "foo &: dep",
            "foo &:: dep",
            "foo: %.o: %.c",
            "foo: ; echo inline",
            "foo: X = value",
            "foo: private X := value",
            "foo: dep=literal",
            "foo\\:bar: dep",
            "foo: dep\\#literal",
            "$(UNCLOSED: dep",
            "foo: ${UNCLOSED",
            "foo: $(outer ${inner)}",
            "foo: $",
            "foo: $:",
        ] {
            let input = format!("{header}\n\techo hi\nnext:\n\techo ok\n");
            let lines = scan(input.as_bytes()).unwrap();
            assert!(!lines[1].format_safe, "{header}");
            assert!(lines[3].format_safe, "{header}");
        }
        let lines = scan(b"foo: $(UNCLOSED\n\techo hi\n").unwrap();
        assert_eq!(lines[0].kind, LineKind::Unknown);
        assert!(!lines[1].format_safe);
    }

    #[test]
    fn branch_rule_context_is_not_carried_into_other_branches() {
        assert!(scan(b"ifdef X\nfoo:\nelse\n\tSHELL=x\nendif\n").is_err());
        assert!(scan(b"ifdef X\nfoo:\nendif\n\tSHELL=x\n").is_err());
        assert!(scan(b"ifdef X\nfoo:\nelse\nbar:\nendif\n\techo SHELL=x\n").is_ok());
    }

    #[test]
    fn define_body_uses_literal_keyword_boundaries() {
        for input in [
            b"define OUTER\ndefine = value\nendef\nX=1\nendef\nY=2\n".as_slice(),
            b"define OUTER\nendef#literal\nX=1\nendef\nY=2\n",
            b"define OUTER\noverride define INNER\nX=1\nendef\nY=2\n",
        ] {
            let lines = scan(input).unwrap();
            for line in &lines[..lines.len() - 1] {
                assert!(
                    !line.format_safe,
                    "{}",
                    String::from_utf8_lossy(&input[line.range.clone()])
                );
            }
            assert_eq!(lines.last().unwrap().kind, LineKind::Assignment);
            assert!(lines.last().unwrap().format_safe);
        }
    }
}
