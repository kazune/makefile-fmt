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
            let expected = format!("{name} = value\nall:\n\t@printf '%s\\n' '$({name})'\n");
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
