//! Behaviour of the built-in commands and global conventions
//! (version, usage errors, --generate).

mod common;

use common::cmd;
use common::stdout_json;
use predicates::prelude::*;

#[test]
fn version_goes_to_stdout() {
    let spec = common::spec::Spec::get();
    let Some(flag) = spec.cli.version_flags.first() else {
        return; // no version flag
    };
    let home = tempfile::tempdir().unwrap();
    cmd(home.path())
        .arg(flag)
        .assert()
        .success()
        .stdout(format!(
            "{} {}\n",
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION")
        ))
        .stderr("");
}

#[test]
fn usage_errors_are_not_colored_without_a_tty() {
    let home = tempfile::tempdir().unwrap();
    // Parse errors happen before --color is known, so they follow NO_COLOR/TTY.
    cmd(home.path())
        .arg("--definitely-not-a-flag")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("\u{1b}[").not());
}

#[test]
fn generate_schema_man_and_completions() {
    let home = tempfile::tempdir().unwrap();
    let out = cmd(home.path())
        .args(["--generate", "schema"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let doc = stdout_json(&out);
    assert_eq!(doc["name"], env!("CARGO_PKG_NAME"));
    cmd(home.path())
        .args(["--generate", "man"])
        .assert()
        .success()
        .stdout(predicate::str::contains(".TH"));
    cmd(home.path())
        .args(["--generate", "complete-zsh"])
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_NAME")));
    cmd(home.path())
        .args(["--generate", "nope"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("complete-bash"));
}
