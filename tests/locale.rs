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
    fn lines_that_collate_equal_keep_input_order() {
        // macOS gives Cyrillic no weight in en_US.UTF-8: strcoll calls any
        // two such strings equal, and sort then orders by length only.
        let dir = tempfile::tempdir().unwrap();
        let env = &[("LC_ALL", UTF8)][..];
        let input = "кц\nва\nваа\nк\n".as_bytes();
        assert_eq!(
            run(dir.path(), env, &[], input),
            "к\nкц\nва\nваа\n".as_bytes()
        );
        assert_eq!(
            run(dir.path(), env, &["-u"], input),
            "к\nкц\nваа\n".as_bytes()
        );
        assert_eq!(run(dir.path(), env, &["-c"], "кц\nва\n".as_bytes()), b"");
        assert_eq!(
            run(
                dir.path(),
                env,
                &["-s", "-k2"],
                "z вв\ny кк\nx аа\n".as_bytes()
            ),
            "z вв\ny кк\nx аа\n".as_bytes()
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

/// Locales other than C and en_US.UTF-8, checked against /usr/bin/sort.
#[cfg(target_os = "macos")]
mod oracle {
    use super::*;

    /// rsort and sort agree on `input` with `env` (LC_ALL etc. cleared).
    fn same_as_sort(env: &[(&str, &str)], args: &[&str], input: &[u8]) {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let mut c = std::process::Command::new("/usr/bin/sort");
        c.env_remove("LC_ALL")
            .env_remove("LC_COLLATE")
            .env_remove("LANG");
        c.envs(env.iter().copied()).args(args);
        c.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped());
        let mut child = c.spawn().unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        let want = child.wait_with_output().unwrap().stdout;
        let got = run(dir.path(), env, args, input);
        assert_eq!(
            String::from_utf8_lossy(&got),
            String::from_utf8_lossy(&want),
            "{env:?} {args:?}"
        );
    }

    /// Latin-1-ish and Cyrillic text in UTF-8 and in a single-byte encoding.
    const LATIN: &str = "\u{e9}t\u{e9}\n\u{c9}T\u{c9}\nzoo\n\u{df}x\nA-b\na b\nAb\n\u{a0}x\n";
    const CYRILLIC: &str = "\u{44f}\u{43d}\u{432}. 3\n\u{424}\u{415}\u{412}\u{420}. 2\n\u{43c}\u{430}\u{44f} 5\n\u{434}\u{435}\u{43a}. 1\nfoo\n\u{42f}\u{41d}\u{412}\u{410}\u{420}\u{42c}\n";

    fn encode(text: &str, charset: &str) -> Vec<u8> {
        use std::io::Write;
        let mut child = std::process::Command::new("iconv")
            .args(["-f", "UTF-8", "-t", charset])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap().stdout
    }

    #[test]
    fn single_byte_locales_use_their_classes_and_collation() {
        for (locale, charset, text) in [
            ("en_US.ISO8859-15", "ISO-8859-15", LATIN),
            ("ru_RU.KOI8-R", "KOI8-R", CYRILLIC),
            ("ru_RU.ISO8859-5", "ISO-8859-5", CYRILLIC),
        ] {
            let input = encode(text, charset);
            for args in [
                &[][..],
                &["-f"],
                &["-d"],
                &["-i"],
                &["-df"],
                &["-fu"],
                &["-M"],
                &["-k1.2,1.3"],
            ] {
                same_as_sort(&[("LANG", locale)], args, &input);
            }
        }
    }

    #[test]
    fn utf8_classes_and_month_names_follow_the_locale() {
        for locale in ["en_US.UTF-8", "ru_RU.UTF-8"] {
            for text in [LATIN, CYRILLIC, "a\u{200b}b\nab\n\u{85}x\nx\n"] {
                for args in [&[][..], &["-f"], &["-d"], &["-i"], &["-M"], &["-s", "-M"]] {
                    same_as_sort(&[("LANG", locale)], args, text.as_bytes());
                }
            }
        }
    }

    #[test]
    fn categories_come_from_their_own_variables() {
        let text = CYRILLIC.as_bytes();
        for env in [
            &[("LANG", "C"), ("LC_CTYPE", "en_US.UTF-8")][..],
            &[("LANG", "C"), ("LC_COLLATE", "en_US.UTF-8")],
            &[("LANG", "en_US.UTF-8"), ("LC_TIME", "ru_RU.UTF-8")],
            &[("LANG", "en_US.UTF-8"), ("LC_MESSAGES", "no_SUCH.locale")],
        ] {
            for args in [&[][..], &["-M"], &["-f"], &["-k1.2,1.3"]] {
                same_as_sort(env, args, text);
            }
        }
    }

    #[test]
    fn numbers_use_each_locales_symbols() {
        // sv_FI: U+00A0 groups and U+2212 is the minus ('-' is not); fa_IR:
        // Arabic decimal and group separators; de_DE: '.' groups, ',' point.
        let input = "\u{2212}5\n-5\n1\u{a0}000\n2 000\n1,5\n1.5\n3\u{66b}5\n3\u{66c}500\n,05\n0,0\n0\n5\n\u{a0}7\n\u{202f}8e1\n";
        for locale in [
            "en_US.UTF-8",
            "sv_FI.UTF-8",
            "fa_IR.UTF-8",
            "de_DE.UTF-8",
            "fr_FR.UTF-8",
        ] {
            for args in [
                &["-n"][..],
                &["-h"],
                &["-g"],
                &["-s", "-n"],
                &["-rn"],
                &["-un"],
                &["-t", ",", "-n"],
                &["-t", "-", "-n"],
            ] {
                same_as_sort(&[("LANG", locale)], args, input.as_bytes());
            }
        }
        // GNUSORT_NUMERIC_COMPATIBILITY keeps the symbols whatever -t is.
        same_as_sort(
            &[
                ("LANG", "en_US.UTF-8"),
                ("GNUSORT_NUMERIC_COMPATIBILITY", "1"),
            ],
            &["-t", ",", "-k1n"],
            b"1,5\n1,4\n-2\n",
        );
        same_as_sort(
            &[("LANG", "C"), ("GNUSORT_NUMERIC_COMPATIBILITY", "1")],
            &["-t", "-", "-k1n"],
            b"-2\n-1\n3\n",
        );
    }

    #[test]
    fn keys_strxfrm_misorders_still_collate_like_strcoll() {
        // macOS strxfrm disagrees with strcoll on tabs and on characters a
        // locale doesn't weigh, so such keys must compare with strcoll.
        for locale in ["en_US.UTF-8", "de_DE.UTF-8", "en_US.ISO8859-15"] {
            for input in [
                "B\nb\t\n",
                "b\t\nB\n",
                "\u{4e2d}a\nZ\na\u{4e2d}\n\u{4e2d}\n",
                "z\t\t\nZ\nz\n",
            ] {
                for args in [&[][..], &["-k1d"], &["-r"], &["-u"]] {
                    same_as_sort(&[("LANG", locale)], args, input.as_bytes());
                }
            }
        }
    }

    #[test]
    fn a_multibyte_decimal_point_counts_by_its_first_byte() {
        // zh_HK.Big5HKSCS is single-byte on macOS, with a two-byte decimal
        // point: "." is then not one.
        for args in [&["-n"][..], &["-h"], &["-g"], &["-rn"], &["-s", "-n"]] {
            same_as_sort(
                &[("LANG", "zh_HK.Big5HKSCS")],
                args,
                b"jan\n.2\n1.5\n0.1\n10\n1\xf3\xae5\n1\xf35\n",
            );
        }
    }
}
