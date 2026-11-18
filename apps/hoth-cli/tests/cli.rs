use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hoth"))
        .args(args)
        .output()
        .expect("failed to run hoth")
}

#[test]
fn no_arguments_print_the_foundation_banner() {
    let output = run(&[]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "HOTH — High-Throughput Orbit Telemetry Harness (foundation scaffold)\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn help_lists_available_options() {
    for flag in ["-h", "--help"] {
        let output = run(&[flag]);
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("Usage: hoth [OPTIONS]"));
        assert!(stdout.contains("-h, --help"));
        assert!(stdout.contains("-V, --version"));
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn version_comes_from_the_package_manifest() {
    for flag in ["-V", "--version"] {
        let output = run(&[flag]);
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            concat!("hoth ", env!("CARGO_PKG_VERSION"), "\n")
        );
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn unsupported_arguments_fail_with_usage_guidance() {
    for args in [&["run"][..], &["--invalid"], &["--help", "extra"]] {
        let output = run(args);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("hoth --help"));
    }
}

#[test]
fn catalog_preview_needs_no_credentials_or_database_access() {
    let output = Command::new(env!("CARGO_BIN_EXE_hoth"))
        .env_remove("SPACE_TRACK_IDENTITY")
        .env_remove("SPACE_TRACK_PASSWORD")
        .args([
            "catalog",
            "bootstrap",
            "--start",
            "2021-11",
            "--end",
            "2026-11",
            "--database",
            ":",
            "--dry-run",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("2021-11 through 2026-11 (end exclusive)"));
    assert!(stdout.contains("No requests made"));
    assert!(output.stderr.is_empty());
}

#[test]
fn live_catalog_rejects_unscoped_requests_before_authentication() {
    let output = run(&[
        "catalog",
        "bootstrap",
        "--start",
        "2020-01",
        "--end",
        "2020-02",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires explicit NORAD IDs"));
}
