//! Locale collation: LC_ALL > LC_COLLATE > LANG pick the locale, and text
//! keys compare with strcoll. Locale-sensitive expectations are macOS-only:
//! the static musl Linux builds always compare bytes.

mod common;

use common::cmd;
use std::path::Path;

const MIXED: &str = "b\nB\na\nA\n_x\n";

fn run(dir: &Path, env: &[(&str, &str)], args: &[&str], input: &[u8]) -> Vec<u8> {
    let mut c = cmd(dir);
    c.env_remove("LC_ALL")
        .env_remove("LC_COLLATE")
        .env_remove("LANG");
    for (k, v) in env {
        c.env(k, v);
    }
    c.args(args)
        .write_stdin(input.to_vec())
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone()
}

#[test]
fn c_and_posix_locales_compare_bytes() {
    let dir = tempfile::tempdir().unwrap();
    for env in [
        &[][..],
        &[("LC_ALL", "C")],
        &[("LANG", "POSIX")],
        &[("LC_ALL", ""), ("LANG", "C")],
        &[("LANG", "no_SUCH.locale")],
    ] {
        assert_eq!(
            run(dir.path(), env, &[], MIXED.as_bytes()),
            b"A\nB\n_x\na\nb\n",
            "{env:?}"
        );
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;

    const UTF8: &str = "en_US.UTF-8";

    #[test]
    fn utf8_locales_collate_like_sort() {
        let dir = tempfile::tempdir().unwrap();
        let want = b"_x\na\nA\nb\nB\n";
        assert_eq!(
            run(dir.path(), &[("LC_ALL", UTF8)], &[], MIXED.as_bytes()),
            want
        );
        assert_eq!(
            run(dir.path(), &[("LANG", UTF8)], &[], MIXED.as_bytes()),
            want
        );
        assert_eq!(
            run(
                dir.path(),
                &[("LANG", "C"), ("LC_COLLATE", UTF8)],
                &[],
                MIXED.as_bytes()
            ),
            want
        );
    }

    #[test]
    fn precedence_is_lc_all_then_lc_collate_then_lang() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = b"A\nB\n_x\na\nb\n";
        assert_eq!(
            run(
                dir.path(),
                &[("LANG", UTF8), ("LC_COLLATE", "C")],
                &[],
                MIXED.as_bytes()
            ),
            bytes
        );
        assert_eq!(
            run(
                dir.path(),
                &[("LC_COLLATE", UTF8), ("LC_ALL", "C")],
                &[],
                MIXED.as_bytes()
            ),
            bytes
        );
    }

    #[test]
    fn invalid_utf8_falls_back_to_bytes() {
        // sort fails with "Illegal byte sequence"; rsort orders such lines by bytes.
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            run(dir.path(), &[("LC_ALL", UTF8)], &[], b"\xff\nb\na\n"),
            b"a\nb\n\xff\n"
        );
    }

    #[test]
    fn nul_bytes_do_not_truncate_comparisons() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            run(dir.path(), &[("LC_ALL", UTF8)], &[], b"a\0c\na\0b\na\n"),
            b"a\na\0b\na\0c\n"
        );
    }

    #[test]
    fn character_offsets_count_characters() {
        // In a UTF-8 locale -k1.2 is the second character, not byte.
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            run(
                dir.path(),
                &[("LC_ALL", UTF8)],
                &["-k1.2,1.2"],
                "\u{e9}x\nab\n\u{e9}a\n".as_bytes()
            ),
            "\u{e9}a\nab\n\u{e9}x\n".as_bytes()
        );
    }

    #[test]
    fn numbers_use_the_locale_separators() {
        let dir = tempfile::tempdir().unwrap();
        let us = [("LC_ALL", UTF8)];
        let de = [("LC_ALL", "de_DE.UTF-8")];
        let sorted = |env: &[(&str, &str)], args: &[&str], input: &str| {
            String::from_utf8(run(dir.path(), env, args, input.as_bytes())).unwrap()
        };
        assert_eq!(sorted(&us, &["-n"], "1,000\n999\n"), "999\n1,000\n");
        assert_eq!(sorted(&us, &["-n"], ",5\n0\n1\n"), "0\n1\n,5\n");
        assert_eq!(sorted(&us, &["-n"], "1,,0\n2\n"), "1,,0\n2\n");
        assert_eq!(sorted(&us, &["-n"], ",.9\n0.5\n1\n"), "0.5\n,.9\n1\n");
        // A -t equal to the thousands separator is only a field separator.
        assert_eq!(sorted(&us, &["-n", "-t,"], "1,000\n999\n"), "1,000\n999\n");
        assert_eq!(sorted(&us, &["-h", "-t,"], "1,0K\n2K\n"), "1,0K\n2K\n");
        assert_eq!(sorted(&us, &["-n"], "-1,000\n-999\n"), "-1,000\n-999\n");
        assert_eq!(sorted(&us, &["-h"], "1,0K\n2K\n"), "2K\n1,0K\n");
        assert_eq!(sorted(&us, &["-g"], "1,000\n999\n"), "1,000\n999\n");
        assert_eq!(sorted(&de, &["-n"], "1,5\n1.4\n"), "1,5\n1.4\n");
        assert_eq!(sorted(&de, &["-g"], "1,5e1\n9\n"), "9\n1,5e1\n");
        assert_eq!(
            sorted(&[("LC_ALL", "C")], &["-n"], "1,000\n999\n"),
            "1,000\n999\n"
        );
    }

    /// rsort and /usr/bin/sort agree in a UTF-8 locale on punctuation, case,
    /// accents, and the text options.
    #[test]
    fn matches_usr_bin_sort_in_utf8() {
        let sort = Path::new("/usr/bin/sort");
        if !sort.exists() {
            return;
        }
        let input = "ab\na b\na-b\naB\n\u{e9}\ne\nE\nf\n\u{c9}t\u{e9}\n_x\nZ\nz\n10\n9\n\u{fc}ber\nUber\n1,000\n999\n2,5K\n1.5e3\n\u{e5}b\n\u{c5}a\naa\nAb\n";
        let dir = tempfile::tempdir().unwrap();
        for args in [
            &[][..],
            &["-f"],
            &["-r"],
            &["-u"],
            &["-d"],
            &["-i"],
            &["-fu"],
            &["-k1.2"],
            &["-s", "-k1,1f"],
            &["-n"],
            &["-h"],
            &["-g"],
        ] {
            let want = std::process::Command::new(sort)
                .args(args)
                .env("LC_ALL", UTF8)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .and_then(|mut child| {
                    use std::io::Write;
                    child.stdin.take().unwrap().write_all(input.as_bytes())?;
                    child.wait_with_output()
                })
                .unwrap()
                .stdout;
            let got = run(dir.path(), &[("LC_ALL", UTF8)], args, input.as_bytes());
            assert_eq!(
                String::from_utf8_lossy(&got),
                String::from_utf8_lossy(&want),
                "args {args:?}"
            );
        }
    }
}
