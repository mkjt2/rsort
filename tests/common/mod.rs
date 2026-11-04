//! Shared helpers for integration tests.
#![allow(dead_code)]

use std::path::Path;

/// `<NAME>_` prefix used by this CLI's environment variables.
pub fn env_prefix() -> String {
    format!(
        "{}_",
        env!("CARGO_PKG_NAME").to_uppercase().replace('-', "_")
    )
}

/// The binary under test with a hermetic environment: config and home point
/// at `home`, and variables that change output are removed. (The environment
/// is not cleared wholesale because coverage tooling relies on it.)
pub fn cmd(home: &Path) -> assert_cmd::Command {
    let mut c = assert_cmd::cargo::cargo_bin_cmd!();
    for (key, _) in std::env::vars() {
        if key.starts_with(&env_prefix()) {
            c.env_remove(key);
        }
    }
    c.env_remove("NO_COLOR")
        .env_remove("RUST_LOG")
        .env_remove("CI")
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"));
    c
}

/// Validate `json` against the `output_schema` declared for `path` in cli.toml.
pub fn assert_matches_schema(path: &str, json: &serde_json::Value) {
    let spec = rsort::spec::Spec::get();
    let command = spec
        .command(path)
        .unwrap_or_else(|| panic!("no command `{path}` in cli.toml"));
    let schema = command
        .output_schema
        .as_ref()
        .unwrap_or_else(|| panic!("command `{path}` declares no output_schema"));
    let schema = serde_json::to_value(schema).unwrap();
    let validator = jsonschema::validator_for(&schema).expect("output_schema is valid JSON Schema");
    let errors: Vec<String> = validator.iter_errors(json).map(|e| e.to_string()).collect();
    assert!(
        errors.is_empty(),
        "`{path}` output violates its schema: {errors:?}\n{json}"
    );
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

/// Parse stderr as the `{"error": ...}` envelope.
pub fn stderr_error(output: &std::process::Output) -> serde_json::Value {
    let v: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap_or_else(|e| {
        panic!(
            "stderr is not JSON ({e}): {}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    v["error"].clone()
}
