//! Build-time environment diagnostics.
//!
//! Mirrors the test-time environment snapshot (tests/tests/ci/env_snapshot.rs)
//! at build time, where intermittent CI-only failures first show up. Emitted
//! as cargo warnings so the details land in the job log even when passing-test
//! stdout is captured by the test harness.
//!
//! Read-only and std-only, mirroring the snapshot test.

use std::env;
use std::fs;
use std::path::Path;
use std::thread::available_parallelism;

/// World-readable files that describe host-level build behavior.
const SNAPSHOT_FILES: &[&str] = &[
    "/proc/sys/kernel/hostname",
    "/proc/meminfo",
    "/proc/self/cgroup",
    "/proc/net/route",
    "/etc/hosts",
    "/etc/resolv.conf",
];

/// Well-known runner and toolchain paths.
const PROBE_PATHS: &[&str] = &[
    "/home/runner",
    "/actions-runner",
    "/var/run/docker.sock",
    "/var/run/secrets/kubernetes.io/serviceaccount",
];

/// Environment variables that commonly differ between CI and local builds.
const SNAPSHOT_ENV_KEYS: &[&str] = &[
    "CI",
    "GITHUB_ACTIONS",
    "GITHUB_JOB",
    "GITHUB_RUN_ID",
    "GITHUB_WORKSPACE",
    "RUNNER_NAME",
    "RUNNER_ENVIRONMENT",
    "RUSTUP_TOOLCHAIN",
    "HOME",
];

/// Maximum number of warning lines emitted per file.
const MAX_LINES_PER_FILE: usize = 8;

fn emit(line: String) {
    println!("cargo:warning=[env-snapshot] {line}");
}

fn snapshot_file(path: &str) {
    match fs::read_to_string(path) {
        Ok(contents) => {
            emit(format!("file {path}:"));
            for line in contents.lines().take(MAX_LINES_PER_FILE) {
                emit(format!("  {line}"));
            }
        }
        Err(err) => emit(format!("file {path}: unavailable ({err})")),
    }
}

fn snapshot_path_probe(path: &str) {
    let state = if Path::new(path).exists() {
        "present"
    } else {
        "absent"
    };
    emit(format!("path {path}: {state}"));
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    emit(format!(
        "cpus: {}",
        available_parallelism().map(|n| n.get()).unwrap_or(1)
    ));

    for key in SNAPSHOT_ENV_KEYS {
        match env::var(key) {
            Ok(value) => emit(format!("env {key}={value}")),
            Err(_) => emit(format!("env {key}=<unset>")),
        }
    }

    for path in SNAPSHOT_FILES {
        snapshot_file(path);
    }

    for path in PROBE_PATHS {
        snapshot_path_probe(path);
    }
}
