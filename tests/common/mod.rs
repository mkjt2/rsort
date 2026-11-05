//! Shared helpers for integration tests.
#![allow(dead_code)]

use std::path::Path;

/// The typed `cli.toml` model, shared with build.rs.
#[path = "../../src/spec/model.rs"]
pub mod spec;

/// The binary under test, run in `dir`, without variables that change its
/// output. (The environment is not cleared wholesale because coverage tooling
/// relies on it.)
pub fn cmd(dir: &Path) -> assert_cmd::Command {
    let mut c = assert_cmd::cargo::cargo_bin_cmd!();
    c.current_dir(dir).env_remove("NO_COLOR").env_remove("CI");
    c
}

/// Parse stdout as a single JSON document.
pub fn stdout_json(output: &std::process::Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout is not JSON ({e}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}
