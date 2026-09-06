use std::{
    io::Write,
    process::{Command, Output, Stdio},
    sync::Once,
};

fn format(source: &[u8]) -> Vec<u8> {
    makefile_fmt::format(source, "shfmt")
        .expect("tests require the fork of shfmt with --explicit-semicolons on PATH")
}

#[test]
fn fixtures_and_idempotency() {
    let fixtures: &[(&[u8], &[u8])] = &[
        (
            include_bytes!("fixtures/basic.mk"),
            include_bytes!("fixtures/basic.expected.mk"),
        ),
        (
            include_bytes!("fixtures/opaque.mk"),
            include_bytes!("fixtures/opaque.mk"),
        ),
        (b"", b""),
        (
            include_bytes!("fixtures/masking.mk"),
            include_bytes!("fixtures/masking.expected.mk"),
        ),
        (
            include_bytes!("fixtures/headers.mk"),
            include_bytes!("fixtures/headers.expected.mk"),
        ),
        (
            b"all:\n\techo    one\n\techo    two\n",
            b"all:\n\techo one;\n\techo two;\n",
        ),
        (
            b"all:\n\techo    one\n\techo    two",
            b"all:\n\techo one;\n\techo two;",
        ),
        (b"X=   \nY=\xff  \n", b"X =   \nY = \xff  \n"),
        (
            b"all:\n\techo one && \\\n\techo two\n",
            b"all:\n\techo one && \\\n\t\techo two;\n",
        ),
        (b"all:\n\techo    x \\ \n", b"all:\n\techo x \\ ;\n"),
    ];
    for &(input, expected) in fixtures {
        let output = format(input);
        assert_eq!(
            String::from_utf8_lossy(&output),
            String::from_utf8_lossy(expected),
            "input: {:?}",
            String::from_utf8_lossy(input)
        );
        assert_eq!(output, expected);
        assert_eq!(format(&output), output, "not idempotent");
    }
}

#[test]
fn command_position_expansion_is_masked_only_under_format_safe_rules() {
    let command = "\t$(CC) $(CFLAGS) $< -o $@ $(LDFLAGS) $(LDLIBS)";
    for (header, format_safe) in [
        ("foo.o: foo.c", true),
        ("$(OUTDIR)/foo.o: foo.c", true),
        ("foo.o: $(SRC)", true),
        ("%.o: %.c", true),
        ("$(OUTDIR)/%: %.c | $(OUTDIR)", true),
        ("foo.o:: foo.c", false),
        ("foo.o &: foo.c", false),
        ("foo.o &:: foo.c", false),
        ("foo.o: %.o: %.c", false),
        ("foo.o: foo.c ; echo inline", false),
        ("foo.o: CFLAGS = -O2", false),
    ] {
        let input = format!("{header}\n{command}\n\nclean:\n\trm -rf $(OUTDIR)\n");
        let lines = makefile_fmt::scan::scan(input.as_bytes()).unwrap();
        if format_safe {
            assert_eq!(lines[1].kind, makefile_fmt::scan::LineKind::Recipe);
        }
        assert_eq!(lines[1].format_safe, format_safe, "{header}");
        let suffix = if format_safe { ";" } else { "" };
        let expected = format!("{header}\n{command}{suffix}\n\nclean:\n\trm -rf $(OUTDIR);\n");
        let output = format(input.as_bytes());
        assert_eq!(output, expected.as_bytes(), "{header}");
        assert_eq!(format(&output), output, "not idempotent: {header}");
    }
}

#[test]
fn crlf_and_final_newline_are_preserved() {
    let input = include_str!("fixtures/basic.mk").replace('\n', "\r\n");
    let expected = include_str!("fixtures/basic.expected.mk").replace('\n', "\r\n");
    for (input, expected) in [
        (input.as_bytes(), expected.as_bytes()),
        (
            &input.as_bytes()[..input.len() - 2],
            &expected.as_bytes()[..expected.len() - 2],
        ),
    ] {
        let output = format(input);
        assert_eq!(output, expected);
        assert_eq!(format(&output), output);
    }
    let mixed = b"X=1\r\nall:\n\tif true; then \\\r\n\techo x; \\\n\tfi\r\nY=2";
    assert_eq!(
        format(mixed),
        b"X = 1\r\nall:\n\tif true; then \\\r\n\techo x; \\\n\tfi\r\nY = 2"
    );
}

fn execute_make(source: &[u8]) -> Output {
    static VERSION_CHECK: Once = Once::new();
    VERSION_CHECK.call_once(|| {
        let version = Command::new("make")
            .arg("--version")
            .output()
            .expect("GNU Make 4.4.1 must be on PATH");
        assert!(version.status.success());
        assert_eq!(
            version.stdout.split(|&b| b == b'\n').next(),
            Some(b"GNU Make 4.4.1".as_slice()),
            "semantic tests require GNU Make 4.4.1 on PATH"
        );
    });
    let mut child = Command::new("make")
        .args(["-rR", "-s", "-f", "-", "all"])
        .env_remove("MAKEFLAGS")
        .env_remove("MFLAGS")
        .env_remove("MAKEFILES")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(source).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn make_441_assignment_directive_boundaries() {
    for name in [
        "include", "-include", "sinclude", "load", "-load", "define", "endef", "ifdef", "ifndef",
        "ifeq", "ifneq", "else", "endif", "export", "unexport", "override", "private", "undefine",
        "vpath",
    ] {
        for separator in ["=", " = "] {
            let input = format!("{name}{separator}value\nall:\n\t@printf '%s\\n' '$({name})'\n");
            let formatted = format(input.as_bytes());
            let expected = format!("{name} = value\nall:\n\t@printf '%s\\n' '$({name})';\n");
            assert_eq!(formatted, expected.as_bytes());
            let before = execute_make(input.as_bytes());
            let after = execute_make(&formatted);
            assert!(
                before.status.success(),
                "{name}: {}",
                String::from_utf8_lossy(&before.stderr)
            );
            assert!(
                after.status.success(),
                "{name}: {}",
                String::from_utf8_lossy(&after.stderr)
            );
            assert_eq!(before.stdout, b"value\n");
            assert_eq!(after.stdout, before.stdout);
            assert_eq!(format(&formatted), formatted);
        }
    }
}

#[test]
fn make_441_define_body_boundaries_preserve_the_entire_value() {
    for body in [
        "define = value\nendef\nX=1",
        "endef#literal\nX=1",
        "override define INNER\nX=1",
    ] {
        let input =
            format!("define OUTER\n{body}\nendef\nY=2\n$(info $(value OUTER))\nall: ; @true\n");
        let expected = input.replace("Y=2", "Y = 2");
        let formatted = format(input.as_bytes());
        assert_eq!(formatted, expected.as_bytes());
        let before = execute_make(input.as_bytes());
        let after = execute_make(&formatted);
        assert!(
            before.status.success(),
            "{}",
            String::from_utf8_lossy(&before.stderr)
        );
        assert!(
            after.status.success(),
            "{}",
            String::from_utf8_lossy(&after.stderr)
        );
        assert_eq!(before.stdout, format!("{body}\n").as_bytes());
        assert_eq!(after.stdout, before.stdout);
        assert_eq!(format(&formatted), formatted);
    }
}

#[test]
fn make_441_conditional_arguments_are_not_assignments() {
    let input = b"ifeq (a=b,a=b)\nX=1\nelse\nX=2\nendif\nY=3\nall:\n\t@printf '%s\\n' '$(X)'\n";
    let formatted = format(input);
    assert_eq!(
        formatted,
        b"ifeq (a=b,a=b)\nX=1\nelse\nX=2\nendif\nY = 3\nall:\n\t@printf '%s\\n' '$(X)';\n"
    );
    let before = execute_make(input);
    let after = execute_make(&formatted);
    assert!(before.status.success() && after.status.success());
    assert_eq!(before.stdout, b"1\n");
    assert_eq!(after.stdout, before.stdout);
    assert_eq!(format(&formatted), formatted);
}

#[test]
fn make_441_assignment_operators_preserve_values() {
    for operator in ["=", ":=", "::=", ":::=", "?=", "+=", "!="] {
        let input = format!(
            "X=seed\nX{operator}   {}  \nall:\n\t@printf '%s\\n' '$(X)'\n",
            if operator == "!=" {
                "printf value"
            } else {
                "value"
            }
        );
        let formatted = format(input.as_bytes());
        let before = execute_make(input.as_bytes());
        let after = execute_make(&formatted);
        assert!(
            before.status.success(),
            "{operator}: {}",
            String::from_utf8_lossy(&before.stderr)
        );
        assert!(
            after.status.success(),
            "{operator}: {}",
            String::from_utf8_lossy(&after.stderr)
        );
        assert_eq!(after.stdout, before.stdout);
        assert_eq!(format(&formatted), formatted);
    }
}

#[test]
fn formatting_preserves_make_execution() {
    let cases: &[&[u8]] = &[
        include_bytes!("fixtures/basic.mk"),
        b"all:\n\t@cd /\n\t@pwd\n\t@exit 0\n\t@echo separate\n",
        b"all:\n\t@printf '%s\\n' one | \\\n\tcat\n\t@false || \\\n\techo fallback\n",
        b"all:\n\t@if true; then \\\n\techo yes; \\\n\telse \\\n\techo no; \\\n\tfi\n",
        b"all:\n\t@case one in \\\n\tone) echo matched;; \\\n\t*) echo missed;; \\\n\tesac\n",
        b"all:\n\t@f() { echo called; }; f\n",
        b"all:\n\t@printf '%s\\n' a\\\nb\n",
        b"all:\n\t@echo    x \\ \n",
        b"all:\n\t@-false\n\t@echo survives\n",
        b"X=  one  \nall:\n\t@printf '%s\\n' '$(X)'\n",
    ];
    for &input in cases {
        let formatted = format(input);
        let before = execute_make(input);
        let after = execute_make(&formatted);
        assert!(
            before.status.success(),
            "invalid fixture: {}",
            String::from_utf8_lossy(&before.stderr)
        );
        assert_eq!(
            after.status.code(),
            before.status.code(),
            "{}",
            String::from_utf8_lossy(&after.stderr)
        );
        assert_eq!(
            after.stdout,
            before.stdout,
            "input: {}\nformatted: {}",
            String::from_utf8_lossy(input),
            String::from_utf8_lossy(&formatted)
        );
        assert_eq!(format(&formatted), formatted);
    }
}

#[test]
fn parse_failure_preserves_only_that_command() {
    let input = b"X=1\nall:\n\tfor\n\techo    ok\n";
    assert_eq!(format(input), b"X = 1\nall:\n\tfor\n\techo ok;\n");
}

#[test]
fn escaped_eval_call_patterns_are_conservatively_fatal() {
    let shell_eval = b"all:\n\t@printf '%s\\n' \"$$(eval echo hi)\"\n";
    // GNU Make passes this to the shell as command substitution, not Make
    // eval. The formatter nevertheless rejects the raw eval call pattern.
    let execution = execute_make(shell_eval);
    assert!(
        execution.status.success(),
        "{}",
        String::from_utf8_lossy(&execution.stderr)
    );
    assert_eq!(execution.stdout, b"hi\n");
    for source in [
        shell_eval.as_slice(),
        b"# $$(eval echo hi)\n",
        b"define FOO\n$${eval text}\nendef\n",
        b"all:\n\t$$(eval echo hi)\n",
        b"all:\n\t$$$$(eval echo hi)\n",
    ] {
        // Fatal detection must precede both tool checks and dollar masking.
        let error = makefile_fmt::format(source, "/nonexistent/shfmt").unwrap_err();
        assert_eq!(error.exit_code(), 2);
        assert!(
            matches!(error, makefile_fmt::Error::Unsupported(ref unsupported) if unsupported.feature == "eval")
        );
    }
    assert!(makefile_fmt::scan::scan(b"X = eval\nall:\n\techo evaluate $$(evaluate hi)\n").is_ok());
}

#[test]
fn unsupported_precedes_tool_configuration_checks() {
    let error =
        makefile_fmt::format(b"X=1\n.ONESHELL:\n", "/nonexistent/makefile-fmt-shfmt").unwrap_err();
    assert_eq!(error.exit_code(), 2);
    assert!(error.to_string().starts_with("2:"));
}

#[test]
fn masking_preserves_make_execution_and_shell_dollars() {
    let cases: &[&[u8]] = &[
        b"NAME=hello\nall:\n\t@printf '%s\\n' '$(NAME)' \"${NAME}\" pre$(NAME)post\n",
        b"all:\n\t@value=hello; printf '%s\\n' \"$$value\" '$$value' \\$$value\n",
        b"NAME=hello\nall:\n\t@printf '%s\\n' \"$$HOME\" '$(NAME)' '${NAME}' '$$$$' __MAKEFMT_0_0__\n",
        b"NAME=hello\nall:\n\t@if test -n \"$(NAME)\"; then \\\n\tprintf '%s\\n' \"$$HOME\"; \\\n\tfi\n",
        b"NAME=hello\nall:\n\t@printf '%s\\n' '$(subst h,j,$(NAME))' '${NAME}'\n",
        b"KEY=one\nVAR_one=hello\nall:\n\t@printf '%s\\n' '${VAR_$(KEY)}'\n",
        b"CC=printf\nFLAG=%s\\n\nall:\n\t@$(CC) '$(FLAG)' hello\n",
        b".PHONY: all input\ninput:\nall: input\n\t@printf '%s\\n' '$@' '$<' '$^' '$?'\n",
        b"all:\n\t@for item in a b; do \\\n\tprintf '%s\\n' \"$$item\"; \\\n\tdone\n",
    ];
    for &input in cases {
        let formatted = format(input);
        assert_ne!(formatted, input, "fixture must exercise formatting");
        let before = execute_make(input);
        let after = execute_make(&formatted);
        assert!(
            before.status.success(),
            "{}",
            String::from_utf8_lossy(&before.stderr)
        );
        assert!(
            after.status.success(),
            "{}",
            String::from_utf8_lossy(&after.stderr)
        );
        assert_eq!(
            after.stdout,
            before.stdout,
            "{}",
            String::from_utf8_lossy(&formatted)
        );
        assert_eq!(format(&formatted), formatted);
    }
}

#[test]
fn argument_lists_and_empty_arguments_preserve_syntactic_roles() {
    for (flags, expected_args) in [
        (
            "-O2 -Wall",
            "<-O2>\n<-Wall>\n<-c>\n<__makefmt_role_input.c>\n",
        ),
        ("", "<-c>\n<__makefmt_role_input.c>\n"),
    ] {
        // Keep the compile-shaped invocation, but print each argument instead
        // of invoking a compiler or writing an object file. The function is
        // literal shell syntax; neither CC nor CFLAGS generates that syntax.
        let input = format!(
            "CC = cc_probe\nCFLAGS = {flags}\n.PHONY: all __makefmt_role_input.c\nall: __makefmt_role_input.c\n\t@cc_probe() {{ printf '<%s>\\n' \"$$@\"; }}; $(CC) $(CFLAGS) -c $<\n__makefmt_role_input.c:\n"
        );
        let formatted = format(input.as_bytes());
        assert_ne!(formatted, input.as_bytes());
        assert!(String::from_utf8_lossy(&formatted).contains("$(CC) $(CFLAGS) -c $<;"));
        assert_eq!(format(&formatted), formatted);
        let before = execute_make(input.as_bytes());
        let after = execute_make(&formatted);
        assert!(
            before.status.success(),
            "{}",
            String::from_utf8_lossy(&before.stderr)
        );
        assert!(
            after.status.success(),
            "{}",
            String::from_utf8_lossy(&after.stderr)
        );
        assert_eq!(before.stdout, expected_args.as_bytes());
        assert_eq!(after.stdout, before.stdout);
        assert_eq!(after.stderr, before.stderr);
    }
}

#[test]
fn noop_expansion_branches_preserve_make_execution() {
    for (variable, active_value, expression) in [
        ("X", "1", "$(if $(X),echo ok,:)"),
        ("X", "1", "$(if $(X),echo ok,: nothing to do)"),
        ("CMD", "echo ok", "$(if $(CMD),$(CMD),: nothing to do)"),
    ] {
        for value in ["", active_value] {
            let input = format!("{variable} = {value}\nall:\n\t@{expression}\n");
            let formatted = format(input.as_bytes());
            assert!(formatted.ends_with(format!("\t@{expression};\n").as_bytes()));
            assert_eq!(format(&formatted), formatted);
            let before = execute_make(input.as_bytes());
            let after = execute_make(&formatted);
            assert!(
                before.status.success(),
                "{}",
                String::from_utf8_lossy(&before.stderr)
            );
            assert!(
                after.status.success(),
                "{}",
                String::from_utf8_lossy(&after.stderr)
            );
            assert_eq!(
                before.stdout,
                if value.is_empty() {
                    b"".as_slice()
                } else {
                    b"ok\n"
                }
            );
            assert_eq!(after.stdout, before.stdout);
            assert_eq!(after.stderr, before.stderr);
        }
    }
}

#[test]
fn command_disappearance_is_an_unchecked_input_precondition() {
    for value in ["", ":", ": nothing to do", "echo optional"] {
        let input = format!("OPTIONAL = {value}\nall:\n\t@(echo ok; $(OPTIONAL))\n");
        // This test documents the guarantee boundary, not a new fatal/skip
        // requirement. The formatter does not inspect OPTIONAL's value.
        let formatted = format(input.as_bytes());
        assert_ne!(formatted, input.as_bytes());
        assert!(String::from_utf8_lossy(&formatted).contains("$(OPTIONAL);"));
        assert_eq!(format(&formatted), formatted);
        let before = execute_make(input.as_bytes());
        let after = execute_make(&formatted);
        assert!(
            before.status.success(),
            "{}",
            String::from_utf8_lossy(&before.stderr)
        );
        if value.is_empty() {
            // Removing this list element leaves a bare semicolon in the
            // subshell. Successful formatting cannot certify subset membership.
            assert_eq!(before.stdout, b"ok\n");
            assert!(!after.status.success());
            assert!(!after.stderr.is_empty());
        } else {
            assert!(
                after.status.success(),
                "{}",
                String::from_utf8_lossy(&after.stderr)
            );
            assert_eq!(after.stdout, before.stdout);
            assert_eq!(after.stderr, before.stderr);
        }
    }
}

#[test]
fn masking_crlf_and_final_newline_are_preserved() {
    let input = include_str!("fixtures/masking.mk").replace('\n', "\r\n");
    let expected = include_str!("fixtures/masking.expected.mk").replace('\n', "\r\n");
    for (input, expected) in [
        (input.as_bytes(), expected.as_bytes()),
        (
            &input.as_bytes()[..input.len() - 2],
            &expected.as_bytes()[..expected.len() - 2],
        ),
    ] {
        let formatted = format(input);
        assert_eq!(formatted, expected);
        assert_eq!(format(&formatted), formatted);
    }
}

#[test]
fn unsupported_dollars_and_existing_unsafe_commands_are_preserved() {
    for command in [
        "echo $$$",
        "echo $|",
        "echo $0",
        "echo $x",
        "echo $(BROKEN",
        "echo ${BROKEN",
        "echo $(outer ${inner)",
        "echo $(X) # comment",
        "cat <<EOF $(X)",
        "echo `date` $(X)",
        "echo '",
        "echo '$(X)\\\n\tx'",
        "echo $$(printf x)",
        "echo $(multi\\\n\tline)",
    ] {
        let input = format!("X=1\nall:\n\t{command}\n\techo    valid\n");
        let expected = format!("X = 1\nall:\n\t{command}\n\techo valid;\n");
        assert_eq!(format(input.as_bytes()), expected.as_bytes(), "{command}");
        assert_eq!(format(expected.as_bytes()), expected.as_bytes());
    }
}

#[test]
fn rule_header_corpus_preserves_make_441_execution_and_header_bytes() {
    // These recipes only print: no compiler, mkdir, or rm is executed. The
    // pattern prerequisites have explicit empty rules, so no files are needed.
    let cases = [
        "TARGETS = __makefmt_v03_a __makefmt_v03_b\nall: $(TARGETS)\n$(TARGETS):\n\t@printf    '%s\\n' '$@'\n",
        "all: __makefmt_v03.o\n%.o: %.c\n\t@printf    '%s\\n' '$<' '$@' '$*'\n__makefmt_v03.c:\n",
        "all: | __makefmt_v03_order\n\t@printf    '%s\\n' '$@'\n__makefmt_v03_order:\n\t@printf    '%s\\n' order\n",
        "OUTDIR = __makefmt_v03_dir\nCC = printf\nCFLAGS = %s\\n\nLDFLAGS = link\nLDLIBS = libs\nall: $(OUTDIR)/__makefmt_v03\n$(OUTDIR):\n\t@printf    '%s\\n' '$@'\n$(OUTDIR)/%: %.c | $(OUTDIR)\n\t@$(CC)    '$(CFLAGS)' $< -o $@ $(LDFLAGS) $(LDLIBS)\n__makefmt_v03.c:\n",
        "NAMES = __makefmt_v03.in\nall: ${NAMES:%.in=%.out}\n$(NAMES:%.in=%.out):\n\t@printf    '%s\\n' '$@'\n",
        "NAME = __makefmt_v03\nall: $(subst :;=,,$(NAME):;=)\n$(subst :;=,,$(NAME):;=):\n\t@printf    '%s\\n' '$@'\n",
        "TARGETS = __makefmt_v03_a __makefmt_v03_b\nall: $(TARGETS)\n$(word \\\n1,$(TARGETS)) \\\n$(word 2,$(TARGETS)):\n\t@printf    '%s\\n' '$@'\n",
        "TARGET = __makefmt_v03\nall: ${TARGET} # $(unclosed : ; = &\n${TARGET}:\n# header and recipe can have intervening comments\n\n\t@printf    '%s\\n' '$@'\n",
    ];
    for (case, source) in cases.iter().enumerate() {
        let expected: String = source
            .split_inclusive('\n')
            .map(|line| {
                if line.starts_with('\t') {
                    format!("{};\n", line.trim_end_matches('\n').replace("    ", " "))
                } else {
                    line.to_owned()
                }
            })
            .collect();
        for eol in ["\n", "\r\n"] {
            for final_newline in [true, false] {
                let input = source.replace('\n', eol);
                let expected = expected.replace('\n', eol);
                let (input, expected) = if final_newline {
                    (input.as_str(), expected.as_str())
                } else {
                    (
                        input.strip_suffix(eol).unwrap(),
                        expected.strip_suffix(eol).unwrap(),
                    )
                };
                let output = format(input.as_bytes());
                assert_ne!(
                    output,
                    input.as_bytes(),
                    "must exercise recipe formatting: {case}"
                );
                assert_eq!(
                    output,
                    expected.as_bytes(),
                    "only recipe bytes may change: {case}"
                );
                assert_eq!(format(&output), output, "not idempotent: {case}");
                let before = execute_make(input.as_bytes());
                let after = execute_make(&output);
                assert!(
                    before.status.success(),
                    "case {case}: {}",
                    String::from_utf8_lossy(&before.stderr)
                );
                assert!(
                    after.status.success(),
                    "case {case}: {}",
                    String::from_utf8_lossy(&after.stderr)
                );
                assert_eq!(after.stdout, before.stdout, "case {case}");
                assert_eq!(after.stderr, before.stderr, "case {case}");
            }
        }
    }
}

#[test]
fn ambiguous_headers_and_opaque_contexts_preserve_recipes() {
    for block in [
        "$(UNCLOSED: dep\n\techo    untouched\n",
        "$(UNCLOSED: dep\n\techo target: dep\n\techo    untouched\n",
        "unknown syntax\n\techo target: dep\n\techo    untouched\n",
        "foo: $(UNCLOSED\n\techo    untouched\n",
        "foo: ${UNCLOSED\n\techo    untouched\n",
        "foo: $(outer ${inner)}\n\techo    untouched\n",
        "foo\\:bar: dep\n\techo    untouched\n",
        "foo: dep\\#literal\n\techo    untouched\n",
        "foo: dep \\\n other ; echo inline\n\techo    untouched\n",
        "foo: X = value\n\techo    untouched\n",
        "$(OBJS): private X := value\n\techo    untouched\n",
        "$(OBJS): private X := value\n\techo target: dep\n\techo    untouched\n",
        "ifeq (1,1)\n$(OBJS):\n\techo    untouched\nelse\n%.o: %.c\n\techo    untouched\nendif\n",
        "ifdef X\n$(OBJS):\nelse\n%.o: %.c\nendif\n\techo    untouched\n",
        "define FOO\n$(OBJS):\n\techo    untouched\nendef\n",
        "$(OBJS): ; echo inline\n\techo    untouched\n",
        "$(OBJS): %.o: %.c\n\techo    untouched\n",
        "$(OBJS) &:: dep\n\techo    untouched\n",
        "$(OBJS):\nunknown syntax\n\techo    untouched\n",
    ] {
        let input = format!("{block}next:\n\techo    formatted\n");
        let expected = format!("{block}next:\n\techo formatted;\n");
        let output = format(input.as_bytes());
        assert_eq!(output, expected.as_bytes(), "{block}");
        assert_eq!(format(&output), output);
    }
}

#[test]
fn complex_headers_do_not_bypass_recipe_safety_or_fatal_checks() {
    let source = "$(OUTDIR)/%: %.c | $(OUTDIR)\n\techo $x\n\tcat <<EOF\n\techo `date`\n\techo $(X) # comment\n\techo 'unclosed\n\techo $$(date)\n\tfor\n\techo    $(X)\n";
    assert_eq!(
        format(source.as_bytes()),
        source.replace("echo    $(X)", "echo $(X);").as_bytes()
    );
    for statement in [
        ".ONESHELL:",
        ".POSIX:",
        "SHELL = sh",
        ".SHELLFLAGS = -ec",
        ".RECIPEPREFIX = >",
        "include other.mk",
        "-include other.mk",
        "sinclude other.mk",
        "load other.so",
        "-load other.so",
        "X = .ONESHELL",
        "$(OUTDIR): SHELL := sh",
        "$(OUTDIR): .SHELLFLAGS = -ec",
        "$(OUTDIR): .RECIPEPREFIX = >",
        "# $(eval .ONESHELL:)",
        "define FOO\n${eval SHELL = sh}\nendef",
        "$(OUTDIR):\n\t$(eval SHELL = sh)",
    ] {
        let input = format!("{source}{statement}\n");
        assert_eq!(
            makefile_fmt::format(input.as_bytes(), "/nonexistent/shfmt")
                .unwrap_err()
                .exit_code(),
            2,
            "{statement}"
        );
    }
}
