# macOS sort's own tests

These files come from Apple's `text_cmds`
(github.com/apple-oss-distributions/text_cmds, commit `592aaf8`), which holds
the source of `/usr/bin/sort` (BSD sort 2.3-Apple). rsort replicates each of
its test suites:

| Upstream | What it does | rsort |
| --- | --- | --- |
| `sort/tests/t_sort.sh` (from NetBSD) | 43 ATF cases with fixed inputs and expected output | `tests/upstream.rs`: all 43, same inputs and expectations, fixtures `d_any_char_*.txt`. The three upstream marks `atf_expect_fail` assert what macOS sort does instead. On macOS each check also runs `/usr/bin/sort`. |
| `sort/testsuite/run.sh` | Differential run against a reference sort: `sample.txt` in 6 locales × 5 flag sets × separators × `-k`/`+POS` positions (about 3 million runs), each output checked with `-c`; plus `bigsample.txt` (155 MB) | `scripts/compat.py` (`just compat`): the same matrix against `/usr/bin/sort`, plus the real `ru_RU.ISO8859-5` next to run.sh's misspelled one. `--quick` runs every 50th case. Inputs in `testsuite/`. |
| `tests/sort_vers.c` | `/usr/bin/sort --version` contains `-Apple` and a version number | Not applicable: rsort reports its own version (`tests/cli.rs` checks `--version`). |

`t_sort.sh` is under the NetBSD 2-clause BSD license (its notice is in
`tests/upstream.rs`); the testsuite inputs come from FreeBSD's sort, also
BSD-licensed.
