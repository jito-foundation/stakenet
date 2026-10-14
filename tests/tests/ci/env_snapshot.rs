//! Read-only snapshot of the test environment.
//!
//! The integration suite intermittently fails on CI hosts in ways that do not
//! reproduce locally (for example, several `Build` failures on 2026-10-07).
//! The first question in those cases is whether the host differed: CPU count,
//! available memory, cgroup limits, DNS configuration, or which CI service
//! paths are present. This snapshot captures those details in the job log so
//! they can be correlated with the failure.
//!
//! The test is intentionally read-only: it reads a small set of world-readable
//! files, prints the results with an `[env-snapshot]` prefix, and always
//! passes. It only uses `std`, so it adds no dependencies to the workspace.

use std::env;
use std::fs;
use std::path::Path;
use std::thread::available_parallelism;

/// World-readable files that describe host-level test behavior.
const SNAPSHOT_FILES: &[&str] = &[
    "/proc/sys/kernel/hostname",
    "/proc/meminfo",
    "/proc/self/limits",
    "/proc/self/cgroup",
    "/proc/net/route",
    "/etc/hosts",
    "/etc/resolv.conf",
];

/// Well-known runner and toolchain paths. Recording whether these exist helps
/// distinguish CI hosts from developer machines.
const PROBE_PATHS: &[&str] = &[
    "/home/runner",
    "/actions-runner",
    "/var/run/docker.sock",
    "/var/run/secrets/kubernetes.io/serviceaccount",
];

/// Environment variables that commonly differ between CI and local runs.
const SNAPSHOT_ENV_KEYS: &[&str] = &[
    "CI",
    "GITHUB_ACTIONS",
    "GITHUB_JOB",
    "GITHUB_RUN_ID",
    "GITHUB_RUN_ATTEMPT",
    "GITHUB_WORKSPACE",
    "RUNNER_NAME",
    "RUNNER_ENVIRONMENT",
    "RUSTUP_TOOLCHAIN",
    "RUSTFLAGS",
    "HOME",
];

/// Maximum number of lines printed per file. These files are small; the cap
/// only guards against surprises.
const MAX_LINES_PER_FILE: usize = 16;

fn snapshot_file(path: &str) {
    match fs::read_to_string(path) {
        Ok(contents) => {
            println!("[env-snapshot] file {path}:");
            for line in contents.lines().take(MAX_LINES_PER_FILE) {
                println!("[env-snapshot]   {line}");
            }
        }
        Err(err) => println!("[env-snapshot] file {path}: unavailable ({err})"),
    }
}

fn snapshot_path_probe(path: &str) {
    let state = if Path::new(path).exists() {
        "present"
    } else {
        "absent"
    };
    println!("[env-snapshot] path {path}: {state}");
}

#[test]
fn ci_environment_snapshot() {
    let in_ci = env::var("GITHUB_ACTIONS").as_deref() == Ok("true");

    println!(
        "[env-snapshot] cpus: {}",
        available_parallelism().map(|n| n.get()).unwrap_or(1)
    );

    for key in SNAPSHOT_ENV_KEYS {
        match env::var(key) {
            Ok(value) => println!("[env-snapshot] env {key}={value}"),
            Err(_) => println!("[env-snapshot] env {key}=<unset>"),
        }
    }

    for path in SNAPSHOT_FILES {
        snapshot_file(path);
    }

    for path in PROBE_PATHS {
        snapshot_path_probe(path);
    }

    // The value of this test is the captured output, not the assertions, so
    // they are intentionally minimal: every host must have a home directory,
    // and CI hosts (all Linux) must expose /proc.
    assert!(
        env::var("HOME").is_ok() || env::var("USERPROFILE").is_ok(),
        "no home directory set; environment snapshot incomplete"
    );
    if in_ci {
        assert!(
            Path::new("/proc/self/limits").exists(),
            "CI host is missing /proc; environment snapshot incomplete"
        );
    }
}
