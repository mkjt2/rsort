//! Shared helpers for integration tests.
#![allow(dead_code)]

use std::path::Path;

/// The typed `cli.toml` model, shared with build.rs.
#[path = "../../src/spec/model.rs"]
pub mod spec;

/// `c` run in `dir`, without inherited variables that pick the locale or
/// change output: tests set what they need. (The environment is not cleared
/// wholesale because coverage tooling relies on it.)
pub fn hermetic(mut c: assert_cmd::Command, dir: &Path) -> assert_cmd::Command {
    for (key, _) in std::env::vars() {
        if key.starts_with("LC_") || key == "LANG" || key.starts_with("GNUSORT_") {
            c.env_remove(key);
        }
    }
    c.current_dir(dir).env_remove("NO_COLOR").env_remove("CI");
    c
}

/// The binary under test, run in `dir` (see [`hermetic`]).
pub fn cmd(dir: &Path) -> assert_cmd::Command {
    hermetic(assert_cmd::cargo::cargo_bin_cmd!(), dir)
}

/// [`cmd`] in the C locale: byte order and ASCII classes.
pub fn c_locale(dir: &Path) -> assert_cmd::Command {
    let mut c = cmd(dir);
    c.env("LC_ALL", "C");
    c
}

/// Sort `input` with `args` in the C locale and return stdout, asserting
/// success and an empty stderr.
pub fn sorted(args: &[&str], input: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let out = c_locale(dir.path())
        .args(args)
        .write_stdin(input)
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    String::from_utf8(out).unwrap()
}

pub fn write(dir: &Path, name: &str, content: impl AsRef<[u8]>) {
    std::fs::write(dir.join(name), content).unwrap();
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
