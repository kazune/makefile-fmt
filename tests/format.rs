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
        ("$(OUTDIR)/foo.o: foo.c", false),
        ("foo.o: $(SRC)", false),
        ("%.o: %.c", false),
    ] {
        let input = format!("{header}\n{command}\n\nclean:\n\trm -rf $(OUTDIR)\n");
        let lines = makefile_fmt::scan::scan(input.as_bytes()).unwrap();
        assert_eq!(lines[1].kind, makefile_fmt::scan::LineKind::Recipe);
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
