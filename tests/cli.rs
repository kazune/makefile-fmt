use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

static COUNTER: AtomicU64 = AtomicU64::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "makefile-fmt-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, source: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, source).unwrap();
        path
    }
    fn run(&self, args: &[&str], tool_path: Option<&Path>) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_makefile-fmt"));
        command.current_dir(&self.0).args(args);
        if let Some(path) = tool_path {
            command.env("PATH", path);
        }
        command.output().unwrap()
    }
    fn fake_shfmt(&self, body: &[u8]) {
        let path = self.write("shfmt", body);
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn stdout_check_diff_and_write() {
    let workspace = Workspace::new();
    let input = workspace.write("Makefile", b"X=1\nall:\n\t@echo    hi\n");
    let original = fs::read(&input).unwrap();
    let expected = b"X = 1\nall:\n\t@echo hi;\n";
    let stdout = workspace.run(&["Makefile"], None);
    assert!(
        stdout.status.success(),
        "{}",
        String::from_utf8_lossy(&stdout.stderr)
    );
    assert_eq!(stdout.stdout, expected);
    assert_eq!(fs::read(&input).unwrap(), original);
    let check = workspace.run(&["--check", "Makefile"], None);
    assert_eq!(check.status.code(), Some(1));
    assert!(check.stdout.is_empty());
    let diff = workspace.run(&["--diff", "Makefile"], None);
    assert_eq!(diff.status.code(), Some(0));
    assert!(diff.stdout.starts_with(b"--- Makefile\n+++ Makefile\n@@"));
    assert_eq!(fs::read(&input).unwrap(), original);
    let write = workspace.run(&["-w", "Makefile"], None);
    assert!(write.status.success());
    assert!(write.stdout.is_empty());
    assert_eq!(fs::read(&input).unwrap(), expected);
    assert!(
        workspace
            .run(&["--check", "Makefile"], None)
            .status
            .success()
    );
    assert!(
        workspace
            .run(&["--diff", "Makefile"], None)
            .stdout
            .is_empty()
    );
}

#[test]
fn unsupported_never_writes_or_outputs_a_partial_result() {
    let workspace = Workspace::new();
    for feature in [
        ".ONESHELL:",
        ".POSIX:",
        "SHELL=sh",
        ".SHELLFLAGS=-ec",
        ".RECIPEPREFIX=>",
        "include x",
        "-include x",
        "sinclude x",
        "load x",
        "-load x",
        "X = SHELL",
        "# ${eval x}",
        "define FOO\n$(eval .ONESHELL:)\nendef",
        "all:\n\t$(eval SHELL=sh)",
    ] {
        let source = format!("X=1\r\n{feature}\r\n");
        let path = workspace.write("Makefile", source.as_bytes());
        for mode in ["-w", "--check", "--diff"] {
            let output = workspace.run(&[mode, "Makefile"], None);
            assert_eq!(output.status.code(), Some(2), "{feature}");
            assert!(output.stdout.is_empty());
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("unsupported GNU Make feature")
            );
            assert_eq!(fs::read(&path).unwrap(), source.as_bytes());
        }
    }
}

#[test]
fn unavailable_or_incompatible_shfmt_never_writes() {
    let workspace = Workspace::new();
    let input = workspace.write("Makefile", b"X=1\n");
    for fake in [
        None,
        Some(b"#!/bin/sh\nexit 2\n".as_slice()),
        Some(b"#!/bin/sh\n/bin/cat\n".as_slice()),
    ] {
        if let Some(body) = fake {
            workspace.fake_shfmt(body);
        }
        let output = workspace.run(&["-w", "Makefile"], Some(&workspace.0));
        assert_eq!(output.status.code(), Some(3));
        assert!(output.stdout.is_empty());
        assert_eq!(fs::read(&input).unwrap(), b"X=1\n");
    }
}

#[test]
fn later_subprocess_failure_discards_earlier_edits() {
    let workspace = Workspace::new();
    let input = workspace.write("Makefile", b"X=1\nall:\n\techo hi\n");
    workspace.fake_shfmt(b"#!/bin/sh\nread -r line\nif [ \"$line\" = 'echo makefile_fmt_probe' ]; then\n /bin/cat >/dev/null\n printf 'echo makefile_fmt_probe;\\nif true; then\\n\\techo ok;\\nfi;\\n'\nelse\n exit 2\nfi\n");
    let output = workspace.run(&["-w", "Makefile"], Some(&workspace.0));
    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    assert_eq!(fs::read(input).unwrap(), b"X=1\nall:\n\techo hi\n");
}

#[test]
fn editorconfig_does_not_change_formatting() {
    let workspace = Workspace::new();
    workspace.write("Makefile", include_bytes!("fixtures/basic.mk"));
    workspace.write(".editorconfig", b"root = true\n[*]\nindent_style = space\nindent_size = 8\nshell_variant = bash\nexplicit_semicolons = false\nsimplify = true\nbinary_next_line = true\n");
    let output = workspace.run(&["Makefile"], None);
    assert!(output.status.success());
    assert_eq!(output.stdout, include_bytes!("fixtures/basic.expected.mk"));
}

#[test]
fn writes_preserve_permissions_and_symlinks() {
    let workspace = Workspace::new();
    let input = workspace.write("actual.mk", b"X=1");
    fs::set_permissions(&input, fs::Permissions::from_mode(0o640)).unwrap();
    symlink("actual.mk", workspace.0.join("Makefile")).unwrap();
    let output = workspace.run(&["-w", "Makefile"], None);
    assert!(output.status.success());
    assert_eq!(fs::read(&input).unwrap(), b"X = 1");
    assert_eq!(
        fs::metadata(&input).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert!(
        fs::symlink_metadata(workspace.0.join("Makefile"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_dir(&workspace.0).unwrap().count(), 2);
}

#[test]
fn write_refuses_to_break_hardlinks() {
    let workspace = Workspace::new();
    let input = workspace.write("Makefile", b"X=1\n");
    fs::hard_link(&input, workspace.0.join("alias.mk")).unwrap();
    assert_eq!(
        workspace.run(&["-w", "Makefile"], None).status.code(),
        Some(3)
    );
    assert_eq!(fs::read(input).unwrap(), b"X=1\n");
}

#[test]
fn help_and_usage_do_not_require_shfmt() {
    let workspace = Workspace::new();
    assert!(
        workspace
            .run(&["--help"], Some(&workspace.0))
            .status
            .success()
    );
    assert!(
        workspace
            .run(&["--version"], Some(&workspace.0))
            .status
            .success()
    );
    for args in [
        vec![],
        vec!["--wat"],
        vec!["-w", "--check", "Makefile"],
        vec!["a", "b"],
    ] {
        assert_eq!(workspace.run(&args, None).status.code(), Some(2));
    }
    assert_eq!(workspace.run(&["missing"], None).status.code(), Some(3));
    workspace.write("-Makefile", b"X=1\n");
    assert!(workspace.run(&["--", "-Makefile"], None).status.success());
}
