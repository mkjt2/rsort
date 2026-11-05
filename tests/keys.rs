//! Ordering modes and key fields. Expected outputs were taken from macOS
//! /usr/bin/sort with LC_ALL=C unless a comment says otherwise.

mod common;

use common::cmd;
use predicates::prelude::*;
use std::path::Path;

fn sort(dir: &Path) -> assert_cmd::Command {
    let mut c = cmd(dir);
    c.current_dir(dir)
        .env("LC_ALL", "C")
        .env_remove("LC_COLLATE")
        .env_remove("LANG");
    c
}

/// Sort `input` with `args` and return stdout, asserting success.
fn sorted(args: &[&str], input: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let out = sort(dir.path())
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

fn usage_error(args: &[&str], fragment: &str) {
    let dir = tempfile::tempdir().unwrap();
    sort(dir.path())
        .args(args)
        .write_stdin("a\n")
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicate::str::contains(fragment));
}

// --- ordering modes ---------------------------------------------------------

#[test]
fn numeric_accepts_only_minus_digits_and_point() {
    let input = "+5\n5\n-\n-0\n0\n.5\n1,000\n 5\n-.5\n1e2\n0x10\n";
    assert_eq!(
        sorted(&["-n"], input),
        "-.5\n+5\n-\n-0\n0\n0x10\n.5\n1,000\n1e2\n 5\n5\n"
    );
}

#[test]
fn numeric_compares_long_numbers_exactly() {
    let input = "100000000000000000000000000001\n100000000000000000000000000000.5\n-007\n-7.000\n";
    assert_eq!(
        sorted(&["-n"], input),
        "-007\n-7.000\n100000000000000000000000000000.5\n100000000000000000000000000001\n"
    );
}

#[test]
fn general_numeric_orders_non_numbers_nan_then_values() {
    let input = "0x10\n1e\n  -inf\n+3\n9\ninfinity\n-nan\n1e400\n.5e1\nabc\n";
    assert_eq!(
        sorted(&["-g"], input),
        "abc\n-nan\n  -inf\n1e\n+3\n.5e1\n9\n0x10\ninfinity\n1e400\n"
    );
}

#[test]
fn general_numeric_puts_overflow_beyond_infinity() {
    assert_eq!(sorted(&["-g"], "-inf\n-1e400\n"), "-1e400\n-inf\n");
    assert_eq!(sorted(&["-g"], "0x1p3\n7\n0x1.8\n"), "0x1.8\n7\n0x1p3\n");
}

#[test]
fn human_numeric_orders_sign_then_suffix_then_value() {
    let input = "1k\n1K\n1.5M\n1Ki\n2\n-1M\n1M\n0.5G\nK\n";
    assert_eq!(
        sorted(&["-h"], input),
        "-1M\nK\n2\n1K\n1Ki\n1k\n1M\n1.5M\n0.5G\n"
    );
    assert_eq!(
        sorted(&["--human-numeric-sort"], "-1K\n-2K\n-1M\n-0\n0K\n1\n-1\n"),
        "-1M\n-2K\n-1K\n-1\n-0\n0K\n1\n"
    );
}

#[test]
fn month_matches_three_letter_prefixes() {
    let input = "january\n jan\nJa\nfeb2\nxyz\n\nFEBRUARY\n";
    assert_eq!(
        sorted(&["-M"], input),
        "\nJa\nxyz\n jan\njanuary\nFEBRUARY\nfeb2\n"
    );
}

#[test]
fn version_sort_handles_hidden_files_tildes_and_suffixes() {
    let input = ".hidden\n\na~1\na1~\na1\na\n.\n..\nb.tar.gz\nb-1.tar.gz\n";
    assert_eq!(
        sorted(&["-V"], input),
        "\n.\n..\n.hidden\na~1\na\na1~\na1\nb.tar.gz\nb-1.tar.gz\n"
    );
}

#[test]
fn version_sort_compares_equal_versions_as_bytes() {
    // sort's vcmp: when the parts before the suffixes are equal, the lines
    // compare as bytes (GNU would compare `~` and digits as versions).
    assert_eq!(sorted(&["-V"], "foo.a~\nfoo.a\n"), "foo.a\nfoo.a~\n");
    assert_eq!(sorted(&["-V"], "a.b2\na.b10\n"), "a.b10\na.b2\n");
    assert_eq!(
        sorted(&["-V"], "f.tar.gz~\nf.tar.gz\n"),
        "f.tar.gz\nf.tar.gz~\n"
    );
}

#[test]
fn version_sort_stops_at_a_nul() {
    assert_eq!(
        sorted(&["-V"], "sort.o\nsort.h\0z\n"),
        "sort.h\0z\nsort.o\n"
    );
    assert_eq!(sorted(&["-V"], "a10\na2\0x\n"), "a2\0x\na10\n");
}

#[test]
fn dictionary_fold_and_nonprinting() {
    assert_eq!(
        sorted(&["-df"], "B-b\nb_a\na c\n#a\n"),
        "#a\na c\nb_a\nB-b\n"
    );
    assert_eq!(sorted(&["-i"], "a\u{1}c\nab\naa\n"), "aa\nab\na\u{1}c\n");
}

#[test]
fn ignore_leading_blanks_on_whole_lines() {
    assert_eq!(sorted(&[], " b\na\n"), " b\na\n");
    assert_eq!(sorted(&["-b"], " b\na\n"), "a\n b\n");
}

#[test]
fn sort_type_is_the_same_as_the_mode_flags() {
    assert_eq!(sorted(&["--sort=numeric"], "10\n9\n"), "9\n10\n");
    assert_eq!(sorted(&["-n", "--sort=numeric"], "10\n9\n"), "9\n10\n");
    assert_eq!(sorted(&["--sort", "human-numeric"], "1M\n2K\n"), "2K\n1M\n");
    assert_eq!(sorted(&["--sort=month"], "feb\njan\n"), "jan\nfeb\n");
    assert_eq!(sorted(&["--sort=general-numeric"], "1e2\n3\n"), "3\n1e2\n");
}

#[test]
fn h_and_v_are_modes_not_help_or_version() {
    assert_eq!(sorted(&["-h"], "1M\n2K\n"), "2K\n1M\n");
    assert_eq!(sorted(&["-V"], "a10\na9\n"), "a9\na10\n");
}

#[test]
fn sort_type_adds_its_mode_and_the_first_by_priority_wins() {
    // Like sort, --sort never conflicts: it turns its mode on next to the
    // others, and the comparison is the first of -n -h -g -M -R -V that is on.
    assert_eq!(sorted(&["--sort=numeric", "-g"], "1e2\n3\n"), "1e2\n3\n");
    assert_eq!(sorted(&["-V", "--sort=month"], "feb\njan\n"), "jan\nfeb\n");
    assert_eq!(
        sorted(&["--sort=month", "--sort=numeric"], "10\n9\n"),
        "9\n10\n"
    );
    assert_eq!(
        sorted(&["-k1", "--sort=numeric", "-g"], "1e2\n3\n"),
        "1e2\n3\n"
    );
    // The month flag still uppercases the key: 1m becomes 1M (mega).
    assert_eq!(sorted(&["-h"], "1m\n2\n"), "1m\n2\n");
    assert_eq!(sorted(&["-h", "--sort=month"], "1m\n2\n"), "2\n1m\n");
}

#[test]
fn conflicting_modes_are_usage_errors() {
    usage_error(&["-n", "-M"], "cannot be used with");
    usage_error(&["-k1,1ng"], "cannot be used with");
}

// --- random -----------------------------------------------------------------

#[test]
fn random_with_a_seed_is_repeatable_and_groups_equal_keys() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("seed"), "fixed seed\n").unwrap();
    let input: String = (0..50)
        .map(|i| format!("k{}\nK{}\n", i % 7, i % 7))
        .collect();
    let run = || {
        let out = sort(dir.path())
            .args(["-fR", "--random-source", "seed"])
            .write_stdin(input.clone())
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        String::from_utf8(out).unwrap()
    };
    let first = run();
    assert_eq!(first, run(), "same seed, same output");
    let keys: Vec<String> = first.lines().map(str::to_uppercase).collect();
    let mut groups = keys.clone();
    groups.dedup();
    assert_eq!(groups.len(), 7, "each key's lines are adjacent: {keys:?}");
}

#[test]
fn random_without_a_seed_still_groups_and_keeps_every_line() {
    let input = "b\na\nb\nc\na\n";
    let out = sorted(&["--random-sort"], input);
    let mut lines: Vec<&str> = out.lines().collect();
    let mut groups = lines.clone();
    groups.dedup();
    assert_eq!(groups.len(), 3, "{out}");
    lines.sort_unstable();
    assert_eq!(lines, ["a", "a", "b", "b", "c"]);
}

#[test]
fn random_source_must_be_readable() {
    let dir = tempfile::tempdir().unwrap();
    sort(dir.path())
        .args(["-R", "--random-source=nope"])
        .write_stdin("a\n")
        .assert()
        .code(2)
        .stderr("rsort: nope: No such file or directory\n");
}

// --- keys -------------------------------------------------------------------

#[test]
fn keys_with_options_do_not_inherit_global_ones() {
    // -n is not inherited by a key that has its own b.
    assert_eq!(sorted(&["-n", "-k1b"], "10\n9\n"), "10\n9\n");
    // -r is not inherited by a key with f, but the last resort is reversed.
    assert_eq!(sorted(&["-r", "-k1,1f"], "b\nB\na\nA\n"), "a\nA\nb\nB\n");
    // A key without options inherits -r, and so does the last resort.
    assert_eq!(
        sorted(&["-r", "-k1,1"], "a 1\na 2\nb 0\n"),
        "b 0\na 2\na 1\n"
    );
    // Reverse on the key alone leaves the last resort ascending.
    assert_eq!(sorted(&["-k1,1r"], "a 1\na 2\n"), "a 1\na 2\n");
}

#[test]
fn options_on_either_end_apply_to_the_key() {
    assert_eq!(sorted(&["-k1n,1r"], "1\n2\n"), "2\n1\n");
    assert_eq!(sorted(&["-k1,1nn"], "a\n"), "a\n");
}

#[test]
fn later_keys_break_ties() {
    assert_eq!(sorted(&["-k1,1", "-k1,1nr"], "1\n2\n"), "1\n2\n");
    assert_eq!(
        sorted(&["--key=2,2", "--key=1,1nr"], "3 b\n1 a\n2 b\n10 a\n"),
        "10 a\n1 a\n3 b\n2 b\n"
    );
}

#[test]
fn character_positions_count_leading_blanks_unless_b() {
    assert_eq!(sorted(&["-k2.2"], "x bc\ny ac\n"), "y ac\nx bc\n");
    assert_eq!(sorted(&["-k2.2"], "x  bc\ny ac\n"), "x  bc\ny ac\n");
    assert_eq!(sorted(&["-b", "-k2.2"], "x  bc\ny ac\n"), "x  bc\ny ac\n");
    assert_eq!(sorted(&["-k2.2b"], "x  bc\ny ac\n"), "x  bc\ny ac\n");
    assert_eq!(sorted(&["-b", "-k1,1"], " 2\n1\n"), "1\n 2\n");
    assert_eq!(sorted(&["-k1,1"], " 2\n1\n"), " 2\n1\n");
}

#[test]
fn empty_and_missing_keys_fall_back_to_the_whole_line() {
    assert_eq!(sorted(&["-k1.2,1.2"], "a b c\nab\n"), "a b c\nab\n");
    assert_eq!(sorted(&["-k1.5,1.2"], "b\na\n"), "a\nb\n");
    assert_eq!(sorted(&["-k3"], "b a\na b\n"), "a b\nb a\n");
    assert_eq!(sorted(&["-k2,1"], "b a\na b\n"), "a b\nb a\n");
    assert_eq!(sorted(&["-k1,1.0"], "b\na\n"), "a\nb\n");
    assert_eq!(sorted(&["-k2"], "a b\nc\n\n  \n"), "\n  \nc\na b\n");
}

#[test]
fn field_separator_makes_every_char_a_delimiter() {
    assert_eq!(
        sorted(&["-t:", "-k2,2"], "a:b:c\na::c\nx:a\n"),
        "a::c\nx:a\na:b:c\n"
    );
    assert_eq!(
        sorted(&["--field-separator", ",", "-k2"], "1,b\n2,a\n"),
        "2,a\n1,b\n"
    );
}

#[test]
fn field_separator_escapes_drop_backslashes() {
    // As in sort, a backslash is dropped: `\t` is the letter t (not a tab).
    assert_eq!(
        sorted(&["-t", "\\t", "-k2"], "xtb\ta\nytaz\tb\n"),
        "ytaz\tb\nxtb\ta\n"
    );
    assert_eq!(sorted(&["-t", "\\x", "-k2"], "axb\nbxa\n"), "bxa\naxb\n");
    assert_eq!(
        sorted(&["-t", "\\\\", "-k2"], "a\\b\nb\\a\n"),
        "b\\a\na\\b\n"
    );
    // An empty -t, \0 and \\0 are NUL.
    for t in ["", "\\0", "\\\\0", "\\0x"] {
        assert_eq!(
            sorted(&["-t", t, "-k2"], "a\0b\nb\0a\n"),
            "b\0a\na\0b\n",
            "{t:?}"
        );
    }
    assert_eq!(
        sorted(&["-z", "-t", "\\0", "-k2"], "b\0z\0"),
        "b\0z\0",
        "with -z every record is one field"
    );
}

#[test]
fn unique_and_check_use_keys() {
    assert_eq!(sorted(&["-u", "-k1,1"], "a 2\na 1\nb\n"), "a 2\nb\n");
    let dir = tempfile::tempdir().unwrap();
    sort(dir.path())
        .args(["-c", "-k1,1"])
        .write_stdin("a 2\na 1\n")
        .assert()
        .code(1)
        .stderr("rsort: -:2: disorder: a 1\n");
    sort(dir.path())
        .args(["-c", "-s", "-k1,1"])
        .write_stdin("a 2\na 1\n")
        .assert()
        .success();
}

#[test]
fn invalid_keys_are_usage_errors() {
    for (key, fragment) in [
        ("0", "-k 0: fields are numbered from 1"),
        ("0.1", "-k 0.1: fields are numbered from 1"),
        ("1.0", "-k 1.0: characters are numbered from 1"),
        ("1x", "-k 1x: invalid key"),
        ("1,", "-k 1,: invalid key"),
        ("1,1 ", "-k 1,1 : invalid key"),
        ("1.2.3", "-k 1.2.3: invalid key"),
        ("-1", "-k -1: invalid key"),
        ("x", "-k x: invalid key"),
        ("", "-k : invalid key"),
        ("99999999999999999999", "number too large"),
    ] {
        usage_error(&["-k", key], fragment);
    }
}

#[test]
fn invalid_separators_are_usage_errors() {
    usage_error(&["-t", "x\\"], "the field separator must be one character");
    usage_error(&["-t", "ab"], "the field separator must be one character");
    // Every -t is checked, as in sort, though the last one wins.
    usage_error(
        &["-t", "ab", "-t", ":"],
        "the field separator must be one character",
    );
    assert_eq!(
        sorted(&["-t", "x", "-t", ":", "-k2"], "a:2\nb:1\n"),
        "b:1\na:2\n"
    );
}

#[test]
fn text_options_rewrite_the_key_before_numeric_modes() {
    // -d drops the minus sign, so "  -3z" compares as 3 (found by fuzzing).
    assert_eq!(sorted(&["-d", "-n"], "  -3z\n1\n"), "1\n  -3z\n");
    // -i drops the non-ASCII bytes, so "0É19" compares as 19.
    assert_eq!(sorted(&["-i", "-n"], "9K\n0\u{c9}19\n"), "9K\n0\u{c9}19\n");
}

#[test]
fn dictionary_order_wins_over_ignore_nonprinting() {
    // With -d and -i the tab is kept as a blank (found by fuzzing).
    assert_eq!(sorted(&["-d", "-i"], "0x1A\n\te-B\n"), "\te-B\n0x1A\n");
}

#[test]
fn a_separator_that_is_the_decimal_point_stops_n_and_h_reading_fractions() {
    // As in sort: -n and -h then see 1.5 and 1.4 as 1; -g still reads them.
    let input = "1.5\n1.4\n2.0K\n1.9K\n";
    assert_eq!(
        sorted(&["-s", "-n", "-t", "."], input),
        "1.5\n1.4\n1.9K\n2.0K\n"
    );
    assert_eq!(
        sorted(&["-s", "-h", "-t", "."], input),
        "1.5\n1.4\n1.9K\n2.0K\n"
    );
    assert_eq!(
        sorted(&["-s", "-g", "-t", "."], input),
        "1.4\n1.5\n1.9K\n2.0K\n"
    );
    assert_eq!(
        sorted(&["-s", "-n", "-t", ":"], input),
        "1.4\n1.5\n1.9K\n2.0K\n"
    );
}
