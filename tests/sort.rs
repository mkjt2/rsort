//! rsort behavior beyond the recorded [[case]]s: inputs and outputs, check and
//! merge modes, and error reporting. Ordering semantics live in tests/keys.rs.

mod common;

use common::cmd;
use predicates::prelude::*;
use std::path::Path;

/// The binary in `dir` with byte-order collation.
fn sort(dir: &Path) -> assert_cmd::Command {
    let mut c = cmd(dir);
    c.current_dir(dir)
        .env("LC_ALL", "C")
        .env_remove("LC_COLLATE")
        .env_remove("LANG");
    c
}

fn write(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).unwrap();
}

// --- input and output -------------------------------------------------------

#[test]
fn empty_input_prints_nothing() {
    let dir = tempfile::tempdir().unwrap();
    sort(dir.path())
        .write_stdin("")
        .assert()
        .success()
        .stdout("");
}

#[test]
fn empty_lines_are_lines() {
    let dir = tempfile::tempdir().unwrap();
    sort(dir.path())
        .write_stdin("b\n\na\n")
        .assert()
        .success()
        .stdout("\na\nb\n");
}

#[test]
fn files_are_concatenated_and_dash_is_stdin() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "f", "c\n");
    sort(dir.path())
        .args(["f", "-"])
        .write_stdin("b\na\n")
        .assert()
        .success()
        .stdout("a\nb\nc\n");
}

#[test]
fn output_file_may_be_an_input() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "list", "b\nc\na\n");
    sort(dir.path())
        .args(["-o", "list", "list"])
        .assert()
        .success()
        .stdout("");
    let got = std::fs::read_to_string(dir.path().join("list")).unwrap();
    assert_eq!(got, "a\nb\nc\n");
}

#[test]
fn output_long_form_writes_the_file() {
    let dir = tempfile::tempdir().unwrap();
    sort(dir.path())
        .arg("--output=out")
        .write_stdin("b\na\n")
        .assert()
        .success()
        .stdout("");
    let got = std::fs::read_to_string(dir.path().join("out")).unwrap();
    assert_eq!(got, "a\nb\n");
}

#[test]
fn zero_terminated_adds_a_final_nul() {
    let dir = tempfile::tempdir().unwrap();
    sort(dir.path())
        .arg("--zero-terminated")
        .write_stdin("b\0a")
        .assert()
        .success()
        .stdout("a\0b\0");
}

#[test]
fn files0_from_reads_nul_separated_names() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "a.txt", "z\n");
    write(dir.path(), "b.txt", "y\n");
    write(dir.path(), "list", "a.txt\0b.txt\0");
    sort(dir.path())
        .arg("--files0-from=list")
        .assert()
        .success()
        .stdout("y\nz\n");
}

#[test]
fn files0_from_dash_reads_names_from_stdin() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "a.txt", "b\na\n");
    sort(dir.path())
        .args(["--files0-from", "-"])
        .write_stdin("a.txt")
        .assert()
        .success()
        .stdout("a\nb\n");
}

#[test]
fn files0_from_with_operands_is_a_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "list", "a.txt\0");
    sort(dir.path())
        .args(["--files0-from=list", "a.txt"])
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicate::str::starts_with("rsort: "))
        .stderr(predicate::str::contains("--files0-from"));
}

#[test]
fn files0_from_rejects_empty_names() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "list", "\0");
    sort(dir.path())
        .arg("--files0-from=list")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("empty file name"));
}

#[test]
fn ignored_flags_change_nothing() {
    let dir = tempfile::tempdir().unwrap();
    sort(dir.path())
        .args([
            "--buffer-size=1G",
            "--temporary-directory=/nonexistent",
            "--parallel=8",
            "--batch-size=2",
            "--compress-program=gzip",
            "--mergesort",
            "--qsort",
            "--heapsort",
        ])
        .write_stdin("b\na\n")
        .assert()
        .success()
        .stdout("a\nb\n");
}

// --- unique and stable ------------------------------------------------------

#[test]
fn unique_keeps_one_of_each_line() {
    let dir = tempfile::tempdir().unwrap();
    sort(dir.path())
        .arg("--unique")
        .write_stdin("\n\nb\nb\n")
        .assert()
        .success()
        .stdout("\nb\n");
}

#[test]
fn reverse_long_form() {
    let dir = tempfile::tempdir().unwrap();
    sort(dir.path())
        .arg("--reverse")
        .write_stdin("a\nc\nb\n")
        .assert()
        .success()
        .stdout("c\nb\na\n");
}

// --- check ------------------------------------------------------------------

#[test]
fn check_names_the_file_and_line() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "f", "a\nc\nb\n");
    sort(dir.path())
        .args(["-c", "f"])
        .assert()
        .code(1)
        .stdout("")
        .stderr("rsort: f:3: disorder: b\n");
}

#[test]
fn check_quiet_and_silent_report_nothing() {
    let dir = tempfile::tempdir().unwrap();
    for mode in ["--check=quiet", "--check=silent", "--check-silent"] {
        sort(dir.path())
            .arg(mode)
            .write_stdin("b\na\n")
            .assert()
            .code(1)
            .stdout("")
            .stderr("");
    }
}

#[test]
fn check_rejects_unknown_modes() {
    let dir = tempfile::tempdir().unwrap();
    sort(dir.path())
        .arg("--check=loud")
        .write_stdin("a\n")
        .assert()
        .code(2);
}

#[test]
fn check_ignores_output() {
    let dir = tempfile::tempdir().unwrap();
    sort(dir.path())
        .args(["-c", "-o", "out"])
        .write_stdin("a\nb\n")
        .assert()
        .success()
        .stdout("");
    assert!(!dir.path().join("out").exists());
}

#[test]
fn check_with_merge_is_a_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    sort(dir.path())
        .args(["-c", "-m"])
        .write_stdin("a\n")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn check_forms_combine_and_any_silent_one_wins() {
    // Like sort: -c, -C and --check may be mixed, and once any of them asks
    // for silence the check stays silent.
    let dir = tempfile::tempdir().unwrap();
    for args in [
        &["-C", "-c"][..],
        &["-c", "-C"],
        &["-cC"],
        &["--check=quiet", "--check=diagnose-first"],
        &["--check=silent", "-c"],
    ] {
        sort(dir.path())
            .args(args)
            .write_stdin("b\na\n")
            .assert()
            .code(1)
            .stderr("");
    }
    sort(dir.path())
        .args(["-c", "--check=diagnose-first"])
        .write_stdin("b\na\n")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("disorder"));
}

#[test]
fn check_of_empty_input_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    sort(dir.path())
        .arg("-c")
        .write_stdin("")
        .assert()
        .success();
}

// --- merge ------------------------------------------------------------------

#[test]
fn merge_interleaves_without_sorting() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "m1", "b\na\n");
    write(dir.path(), "m2", "c\n");
    sort(dir.path())
        .args(["-m", "m1", "m2"])
        .assert()
        .success()
        .stdout("b\na\nc\n");
}

#[test]
fn merge_prefers_earlier_files_on_ties() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "t1", "a 1\n");
    write(dir.path(), "t2", "a 2\n");
    sort(dir.path())
        .args(["-m", "-s", "-k1,1", "t2", "t1"])
        .assert()
        .success()
        .stdout("a 2\na 1\n");
}

#[test]
fn merge_unique_drops_duplicates_across_files() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "m3", "a\nb\n");
    write(dir.path(), "m4", "a\nc\n");
    sort(dir.path())
        .args(["--merge", "--unique", "m3", "m4"])
        .assert()
        .success()
        .stdout("a\nb\nc\n");
}

#[test]
fn merge_reverse_and_output() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "m5", "c\na\n");
    write(dir.path(), "m6", "b\n");
    sort(dir.path())
        .args(["-mr", "-o", "m5", "m5", "m6"])
        .assert()
        .success();
    let got = std::fs::read_to_string(dir.path().join("m5")).unwrap();
    assert_eq!(got, "c\nb\na\n");
}

#[test]
fn merge_reads_stdin_once() {
    // As sort: stdin merges with files, and a second `-` is already empty.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "m7", "b\n");
    sort(dir.path())
        .args(["-m", "-", "m7"])
        .write_stdin("a\nc\n")
        .assert()
        .success()
        .stdout("a\nb\nc\n");
    sort(dir.path())
        .args(["-m", "-", "-"])
        .write_stdin("a\nc\n")
        .assert()
        .success()
        .stdout("a\nc\n");
}

// --- errors -----------------------------------------------------------------

#[test]
fn unreadable_inputs_name_the_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("d")).unwrap();
    sort(dir.path())
        .arg("d")
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicate::str::starts_with("rsort: d: "))
        .stderr(predicate::str::contains("os error").not());
}

#[test]
fn unwritable_output_names_the_file() {
    let dir = tempfile::tempdir().unwrap();
    sort(dir.path())
        .args(["-o", "no/such/dir/out"])
        .write_stdin("a\n")
        .assert()
        .code(2)
        .stderr("rsort: no/such/dir/out: No such file or directory\n");
}

#[test]
fn missing_listed_file_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    sort(dir.path())
        .arg("--files0-from=nope")
        .assert()
        .code(2)
        .stderr("rsort: nope: No such file or directory\n");
}

#[test]
fn help_and_version_are_long_only() {
    let dir = tempfile::tempdir().unwrap();
    sort(dir.path())
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--human-numeric-sort"));
}
