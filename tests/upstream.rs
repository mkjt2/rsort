//! A port of macOS sort's own test suite: `sort/tests/t_sort.sh` from Apple's
//! text_cmds (github.com/apple-oss-distributions/text_cmds, commit 592aaf8),
//! itself from NetBSD. One test per ATF case, same inputs and expectations,
//! fixtures in `tests/upstream/`. Cases upstream marks `atf_expect_fail`
//! assert what macOS sort actually does instead.
//!
//! On macOS every check also runs /usr/bin/sort, so a mistake in the port
//! shows up as sort itself failing. Upstream runs these in the C locale.
//!
//! t_sort.sh carries this notice:
//!
//! ```text
//! Copyright (c) 2008, 2009, 2010 The NetBSD Foundation, Inc.
//! All rights reserved.
//!
//! Redistribution and use in source and binary forms, with or without
//! modification, are permitted provided that the following conditions
//! are met:
//! 1. Redistributions of source code must retain the above copyright
//!    notice, this list of conditions and the following disclaimer.
//! 2. Redistributions in binary form must reproduce the above copyright
//!    notice, this list of conditions and the following disclaimer in the
//!    documentation and/or other materials provided with the distribution.
//!
//! THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
//! ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
//! TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
//! PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
//! BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
//! CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
//! SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
//! INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
//! CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
//! ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
//! POSSIBILITY OF SUCH DAMAGE.
//! ```

mod common;

use std::path::Path;
use std::process::Output;

const SORT: &str = "/usr/bin/sort";

/// rsort, plus the system sort on macOS.
fn tools() -> Vec<&'static str> {
    let mut tools = vec!["rsort"];
    if cfg!(target_os = "macos") && Path::new(SORT).exists() {
        tools.push(SORT);
    }
    tools
}

fn run(tool: &str, dir: &Path, args: &[&str], stdin: &[u8]) -> Output {
    let mut c = if tool == "rsort" {
        common::cmd(dir)
    } else {
        assert_cmd::Command::new(tool)
    };
    c.current_dir(dir)
        .env("LC_ALL", "C")
        .env_remove("LC_COLLATE")
        .env_remove("LANG")
        .args(args)
        .write_stdin(stdin.to_vec())
        .output()
        .unwrap()
}

/// Every tool exits 0 and prints `want`.
fn check(dir: &Path, args: &[&str], stdin: &[u8], want: &[u8]) {
    for tool in tools() {
        let out = run(tool, dir, args, stdin);
        assert!(
            out.status.success(),
            "{tool} {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(want),
            "{tool} {args:?}"
        );
    }
}

/// Every tool exits with `code`; returns their stderr.
fn check_status(dir: &Path, args: &[&str], stdin: &[u8], code: i32) -> Vec<String> {
    tools()
        .into_iter()
        .map(|tool| {
            let out = run(tool, dir, args, stdin);
            assert_eq!(out.status.code(), Some(code), "{tool} {args:?}");
            String::from_utf8_lossy(&out.stderr).into_owned()
        })
        .collect()
}

fn write(dir: &Path, name: &str, content: &[u8]) {
    std::fs::write(dir.join(name), content).unwrap();
}

fn read(path: impl AsRef<Path>) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

fn fixture(name: &str) -> String {
    format!("{}/tests/upstream/{name}", env!("CARGO_MANIFEST_DIR"))
}

const ZYX: &[u8] = b"z b m f\ny c o e\nx a n h\nx a n g\n";

#[test]
fn basic() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "in", ZYX);
    check(
        dir.path(),
        &["in"],
        b"",
        b"x a n g\nx a n h\ny c o e\nz b m f\n",
    );
}

#[test]
fn empty_file() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "empty", b"");
    check(dir.path(), &["empty"], b"", b"");
    check(dir.path(), &["-c", "empty"], b"", b"");
    check(dir.path(), &["-c", "-u", "empty"], b"", b"");
}

#[test]
fn end_of_options() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "-k", b"x\n");
    check(dir.path(), &["--", "-k"], b"", b"x\n");
    // Upstream only requires "not exit 1": `-c` after `-` is still an option.
    check(dir.path(), &["-", "-c"], b"", b"");
}

#[test]
fn missing_newline() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "in", b"x");
    check(dir.path(), &["in"], b"", b"x\n");
}

#[test]
fn null_bytes() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "in", b"\0b\n\0a\n");
    check(dir.path(), &["in"], b"", b"\0a\n\0b\n");
}

#[test]
fn long_records() {
    let dir = tempfile::tempdir().unwrap();
    let x = "x".repeat(4096);
    let lines = |range: &mut dyn Iterator<Item = u32>| -> Vec<u8> {
        range
            .map(|i| format!("{x}{i}\n"))
            .collect::<String>()
            .into()
    };
    write(dir.path(), "in", &lines(&mut (15..=25)));
    let out = lines(&mut (15..=25).rev());
    check(dir.path(), &["-r", "in"], b"", &out);
    check(dir.path(), &["-k", "1,1r", "-k", "1", "in"], b"", &out);
}

#[test]
fn long_file() {
    // 20000 random fractions like awk's rand(); the expectation is
    // `sort -r | uniq`, computed here.
    let dir = tempfile::tempdir().unwrap();
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    let mut lines: Vec<String> = (0..20000)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            format!("0.{:06}", state % 1_000_000)
        })
        .collect();
    write(dir.path(), "in", (lines.join("\n") + "\n").as_bytes());
    lines.sort_by(|a, b| b.cmp(a));
    lines.dedup();
    check(
        dir.path(),
        &["-u", "-r", "in"],
        b"",
        (lines.join("\n") + "\n").as_bytes(),
    );
}

#[test]
fn any_char() {
    let dir = tempfile::tempdir().unwrap();
    let input = fixture("d_any_char_in.txt");
    for (flag, out) in [
        ("-d", "d_any_char_dflag_out.txt"),
        ("-f", "d_any_char_fflag_out.txt"),
        ("-i", "d_any_char_iflag_out.txt"),
    ] {
        check(
            dir.path(),
            &[flag, "-k", "2", &input],
            b"",
            &read(fixture(out)),
        );
    }
}

#[test]
fn bflag() {
    // Upstream expects failure ("Behavior differs from NetBSD"): NetBSD keeps
    // the input order, macOS sorts on the text after the blanks.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "in", b"  b\n a\n");
    check(dir.path(), &["-b", "in"], b"", b" a\n  b\n");
    check(dir.path(), &["-b"], b"  b\n a\n", b" a\n  b\n");
    // `sort in | sort -c -r`
    check(dir.path(), &["in"], b"", b"  b\n a\n");
    check_status(dir.path(), &["-c", "-r"], b"  b\n a\n", 1);
}

#[test]
fn cflag() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "in", b"b\na\n");
    check_status(dir.path(), &["-c", "in"], b"", 1);
}

#[test]
fn kflag_one_field() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "in", ZYX);
    check(
        dir.path(),
        &["-k2.1", "in"],
        b"",
        b"x a n g\nx a n h\nz b m f\ny c o e\n",
    );
}

#[test]
fn kflag_two_fields() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "in", ZYX);
    check(
        dir.path(),
        &["-s", "-k2.1,2.0", "in"],
        b"",
        b"x a n h\nx a n g\nz b m f\ny c o e\n",
    );
}

#[test]
fn kflag_many_fields() {
    let dir = tempfile::tempdir().unwrap();
    let rows = [
        "0:2:3:4:5:6:7:8:9",
        "1:1:3:4:5:6:7:8:9",
        "1:2:2:4:5:6:7:8:9",
        "1:2:3:3:5:6:7:8:9",
        "1:2:3:4:4:6:7:8:9",
        "1:2:3:4:5:5:7:8:9",
        "1:2:3:4:5:6:6:8:9",
        "1:2:3:4:5:6:7:7:9",
        "1:2:3:4:5:6:7:8:8",
    ];
    write(dir.path(), "in", (rows.join("\n") + "\n").as_bytes());
    let mut out = rows;
    out.reverse();
    check(
        dir.path(),
        &[
            "-t:", "-k9", "-k8", "-k7", "-k6", "-k5", "-k4", "-k3", "-k2", "-k1", "in",
        ],
        b"",
        (out.join("\n") + "\n").as_bytes(),
    );
}

#[test]
fn kflag_outofbounds() {
    let dir = tempfile::tempdir().unwrap();
    let input = b"0 5\n1 4\n2 3\n3 2\n4 1\n5 0\n";
    write(dir.path(), "in", input);
    check(dir.path(), &["-k2.2,2.1", "-k2.3,2.4", "in"], b"", input);
}

#[test]
fn kflag_nonmonotone() {
    let dir = tempfile::tempdir().unwrap();
    let input = b"aaaa c\nx a\n0 b\n";
    write(dir.path(), "in", input);
    check(dir.path(), &["-k2,1.3", "-k2.5,2.5", "in"], b"", input);
}

#[test]
fn kflag_limits() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "in", b"a\t2\na\t1\nb\t2\nb\t1\n");
    check(
        dir.path(),
        &["-r", "-k1,1", "-k2nr", "in"],
        b"",
        b"b\t2\nb\t1\na\t2\na\t1\n",
    );
}

#[test]
fn kflag_alpha() {
    let rows = "\
01:04:19:01:16:01:21:01 a
02:03:13:15:13:19:15:02  a
03:02:07:09:07:13:09:03   a
04:01:01:03:01:07:03:04    a
05:08:20:16:17:02:20:05 aa
06:07:14:18:14:20:14:06  aa
07:06:08:10:08:14:08:07   aa
08:05:02:04:02:08:02:08    aa
09:16:22:02:22:04:24:13 b
10:15:16:20:19:22:18:14  b
11:14:10:12:10:16:12:15   b
12:13:04:06:04:10:06:16    b
13:24:24:22:24:06:22:21 bb
14:23:18:24:21:24:16:22  bb
15:22:12:14:12:18:10:23   bb
16:21:06:08:06:12:04:24    bb
17:12:21:21:18:03:19:09 ab
18:11:15:19:15:21:13:10  ab
19:10:09:11:09:15:07:11   ab
20:09:03:05:03:09:01:12    ab
21:20:23:17:23:05:23:17 ba
22:19:17:23:20:23:17:18  ba
23:18:11:13:11:17:11:19   ba
24:17:05:07:05:11:05:20    ba
";
    // Each ordering must leave column N of the output ascending.
    let cases: &[(&[&str], &str)] = &[
        (&["-k2b", "-k2"], "-k2n"),
        (&["-k2,2.1b", "-k2"], "-k3n"),
        (&["-k2.3", "-k2"], "-k4n"),
        (&["-k2b,2.3", "-k2"], "-k5n"),
        (&["-k2.3,2.1b", "-k2"], "-k6n"),
        (&["-k2,2.1b", "-k2r"], "-k7n"),
        (&["-b", "-k2,2", "-k2"], "-k8n"),
    ];
    let dir = tempfile::tempdir().unwrap();
    // Upstream's input is `sort`ed first.
    let mut sorted: Vec<&str> = rows.lines().collect();
    sorted.sort();
    write(dir.path(), "in", (sorted.join("\n") + "\n").as_bytes());
    for tool in tools() {
        for (keys, column) in cases {
            let args: Vec<&str> = keys.iter().copied().chain(["in"]).collect();
            let out = run(tool, dir.path(), &args, b"");
            assert!(out.status.success(), "{tool} {keys:?}");
            let check = run(tool, dir.path(), &["-c", "-t:", column], &out.stdout);
            assert!(
                check.status.success(),
                "{tool} {keys:?}: {column} not ascending\n{}",
                String::from_utf8_lossy(&out.stdout)
            );
        }
    }
}

#[test]
fn kflag_no_end() {
    let dir = tempfile::tempdir().unwrap();
    check(
        dir.path(),
        &["-df", "-k", "1", "-k", "1d"],
        b"a-B\na+b\na b\nA+b\na\tb\n",
        b"a\tb\na b\nA+b\na-B\na+b\n",
    );
}

#[test]
fn mflag() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "in1", b"a\nab\nab\nbc\nca\n");
    write(dir.path(), "in2", b"Z\na\naa\nac\nc\n");
    check(
        dir.path(),
        &["-m", "in1", "in2"],
        b"",
        b"Z\na\na\naa\nab\nab\nac\nbc\nc\nca\n",
    );
}

#[test]
fn mflag_uflag() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "in", b"a\nb\nc\nd\n");
    check(dir.path(), &["-m", "-u", "in"], b"", b"a\nb\nc\nd\n");
}

#[test]
fn mflag_uflag_first() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "in", b"3B\n3b\n3B2\n~3B2\n4.1\n41\n5\n5.\n");
    let out = b"3B\n3B2\n4.1\n5\n";
    check(dir.path(), &["-mudf", "in"], b"", out);
    check(dir.path(), &["-mudf", "-k1", "in"], b"", out);
}

#[test]
fn nflag() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "in",
        b"-99.0\n-99.1\n-.0002\n-10\n2\n0010.000000000000000000000000000000000001\n10\n3x\nx\n",
    );
    check(
        dir.path(),
        &["-n", "in"],
        b"",
        b"-99.1\n-99.0\n-10\n-.0002\nx\n2\n3x\n10\n0010.000000000000000000000000000000000001\n",
    );
}

#[test]
fn nflag_rflag() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "in", b"1\n123\n2\n");
    check(dir.path(), &["-rn", "in"], b"", b"123\n2\n1\n");
}

#[test]
fn oflag() {
    for tool in tools() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "in", b"1\n1\n2\n2\n3\n3\n4\n4\n");
        let out = run(tool, dir.path(), &["-u", "-o", "in", "in"], b"");
        assert!(out.status.success(), "{tool}");
        assert!(out.stdout.is_empty(), "{tool}");
        assert_eq!(read(dir.path().join("in")), b"1\n2\n3\n4\n", "{tool}");
    }
}

#[test]
fn oflag_displaced() {
    for tool in tools() {
        let dir = tempfile::tempdir().unwrap();
        let out = run(tool, dir.path(), &["/dev/null", "-o", "out"], b"");
        assert!(out.status.success(), "{tool}");
        assert!(dir.path().join("out").is_file(), "{tool}: file not created");
    }
}

#[test]
fn rflag() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "in", ZYX);
    check(dir.path(), &["-r", "in"], b"", ZYX);
}

#[test]
fn sflag() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "in", b"a 2\nb 1\nc 2\na 1\nb 2\nc 1\n");
    check(
        dir.path(),
        &["-s", "-k1,1", "in"],
        b"",
        b"a 2\na 1\nb 1\nb 2\nc 2\nc 1\n",
    );
}

#[test]
fn sflag_many_files() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "in1", b"c 2\na 2\n");
    write(dir.path(), "in2", b"c 1\nb 1\na 1\n");
    check(
        dir.path(),
        &["-smru", "-k1,1", "in1", "in1", "in2", "in2"],
        b"",
        b"c 2\nb 1\na 2\n",
    );
}

#[test]
fn tflag() {
    let dir = tempfile::tempdir().unwrap();
    let input = b"a:\na!\n";
    write(dir.path(), "in", input);
    for args in [
        &["-t", ":", "-r", "+0", "in"][..],
        &["-t", ":", "+0", "-1", "in"],
        &["-t", ":", "-r", "-k", "1", "in"],
        &["-t", ":", "-k", "1,1", "in"],
    ] {
        check(dir.path(), args, b"", input);
    }
}

#[test]
fn tflag_alphabetic() {
    let dir = tempfile::tempdir().unwrap();
    let input = b"zXa\nyXa\nzXb\n";
    write(dir.path(), "in", input);
    check(dir.path(), &["-tX", "-k2", "-k1r,1", "in"], b"", input);
}

#[test]
fn tflag_char_pos() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "in", b": ab\n:bac\n");
    let out = b":bac\n: ab\n";
    for args in [
        &["-b", "-t:", "+1.1", "in"][..],
        &["-t:", "+1.1r", "in"],
        &["-b", "-t:", "-k", "2.2", "in"],
        &["-t:", "-k", "2.2r", "in"],
    ] {
        check(dir.path(), args, b"", out);
    }
}

#[test]
fn tflag_whitespace() {
    let dir = tempfile::tempdir().unwrap();
    let input = b" b c\n b\tc\n\tb c\n";
    write(dir.path(), "in", input);
    check(dir.path(), &["-t", " ", "-k2,2", "in"], b"", input);
    check(dir.path(), &["-t", " ", "-k2.1,2.0", "in"], b"", input);
    let out = b" b c\n\tb c\n b\tc\n";
    check(dir.path(), &["-t", "\t", "-k2,2", "in"], b"", out);
    check(dir.path(), &["-t", "\t", "-k2.1,2.0", "in"], b"", out);
    check(dir.path(), &["-k2", "in"], b"", b" b\tc\n\tb c\n b c\n");
    check(dir.path(), &["-k2b", "in"], b"", b"\tb c\n b\tc\n b c\n");
}

#[test]
fn uflag() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "in", b"a\naa\naaa\naa\n");
    check(dir.path(), &["-u", "in"], b"", b"a\naa\naaa\n");
}

#[test]
fn uflag_rflag() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "in", b"a\naa\naaa\naa\n");
    check(dir.path(), &["-ru", "in"], b"", b"aaa\naa\na\n");
}

#[test]
fn plus_one() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "in", ZYX);
    check(
        dir.path(),
        &["+1", "in"],
        b"",
        b"x a n g\nx a n h\nz b m f\ny c o e\n",
    );
}

#[test]
fn plus_one_minus_two() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "in", ZYX);
    check(
        dir.path(),
        &["-s", "+1", "-2", "in"],
        b"",
        b"x a n h\nx a n g\nz b m f\ny c o e\n",
    );
}

#[test]
fn plus_zero() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "+0", b"good contents\n");
    check(dir.path(), &["--", "+0"], b"", b"good contents\n");
}

#[test]
fn plus_nonmonotone() {
    let dir = tempfile::tempdir().unwrap();
    let input = b"aaaa c\nx a\n0 b\n";
    write(dir.path(), "in", input);
    check(
        dir.path(),
        &["+1", "-0.3", "+1.4", "-1.5", "in"],
        b"",
        input,
    );
}

#[test]
fn plus_as_path() {
    // Upstream expects failure ("Behavior differs from NetBSD"): sort turns
    // `+0` into a key wherever it appears, so `+0` is not read as a file.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "+0", b"good contents\n");
    write(dir.path(), "in", b"more contents\n");
    check(dir.path(), &["in", "+0"], b"", b"more contents\n");
}

#[test]
fn plus_bad_tempfile() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "in", b"good contents\n");
    check(
        dir.path(),
        &["-T", "/tmp", "+0", "in"],
        b"",
        b"good contents\n",
    );
}

#[test]
fn plus_rflag_invalid() {
    // Upstream expects failure ("-R flag not available on FreeBSD"): -R is
    // random sort here, so `+` is an input file, and it doesn't exist.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "in", b"z b m f+y c o e+x a n h+x a n g+");
    for stderr in check_status(dir.path(), &["-R", "+", "-k2", "in"], b"", 2) {
        assert!(stderr.contains("No such file or directory"), "{stderr}");
    }
}

#[test]
fn plus_tflag() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("+")).unwrap();
    let input = "y\n".repeat(200_000);
    check(dir.path(), &["-T", "+"], input.as_bytes(), input.as_bytes());
}

#[test]
fn plus_no_end() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "in", b"a-B\na+b\na b\nA+b\na\tb\n");
    check(
        dir.path(),
        &["-df", "+0", "+0d", "in"],
        b"",
        b"a\tb\na b\nA+b\na-B\na+b\n",
    );
}
