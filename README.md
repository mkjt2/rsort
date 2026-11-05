# rsort

[![CI](https://github.com/mkjt2/rsort/actions/workflows/ci.yml/badge.svg)](https://github.com/mkjt2/rsort/actions/workflows/ci.yml)

Sort, merge or check lines of text files.

## Install

```sh
curl -fsSL https://github.com/mkjt2/rsort/releases/latest/download/install.sh | sh
```

Prebuilt binaries for Linux (static musl) and macOS are attached to each
[GitHub release](https://github.com/mkjt2/rsort/releases).
From source: `cargo install --locked --git https://github.com/mkjt2/rsort`.

## Usage

rsort is a drop-in clone of the macOS `sort` (BSD sort 2.3-Apple): same flags,
same output, same exit codes (0 sorted, 1 not sorted with `-c`/`-C`, 2 error).

```sh
rsort names.txt                      # sort lines
du -sh * | rsort -hr                 # largest human-readable sizes first
rsort -t: -k3,3n /etc/passwd         # third field, numerically
rsort -u -o words.txt words.txt      # deduplicate in place
rsort -C data.txt && echo sorted     # check order via the exit code
rsort --help                         # every flag, with examples and exit codes
```

`-h` and `-V` are sort's human-numeric and version sort, so help and version
are `--help` and `--version` only. `rsort --generate man|schema|complete-<shell>`
prints a man page, the interface as JSON, or shell completions.

`-t` takes one byte. As in sort, a backslash before it is dropped, so
`-t '\t'` means the letter `t`: write `-t $'\t'` for a tab, and `-t '\0'`
for NUL.

The obsolete `+POS1 [-POS2]` keys work as in sort: fields and characters
count from 0, so `+1 -2` is `-k2.1,2.0`. As in sort, they are recognized
anywhere before `--`.

### Locale

Like sort, rsort sets each locale category from `LC_ALL`, then its own
variable, then `LANG`:

- `LC_COLLATE`: text compares with the locale's collation (`strcoll`). Lines
  the locale collates the same keep their input order, as in sort (macOS gives
  Cyrillic no collation weight in en_US.UTF-8, for example).
- `LC_CTYPE`: in UTF-8, `-k` character offsets count characters; `-d`, `-i`
  and `-f` use the locale's character classes in any encoding.
- `LC_NUMERIC`: `-n`/`-h`/`-g` use the locale's decimal point, thousands
  separator and minus sign (`1,000` = 1000 in en_US; in sv_FI `−5` is negative
  and `-5` is not a number). `GNUSORT_NUMERIC_COMPATIBILITY` works as in sort, as does
  `GNUSORT_COMPATIBLE_BLANKS` (every white-space character is a blank).
- `LC_TIME`: `-M` matches the locale's month abbreviations.

Set `LC_ALL=C` for byte order and English months.

On Linux the release binaries are static musl builds, whose C library has no
collation: text always compares in byte order there, and numbers use C rules.

### Differences from macOS sort

- Errors name the file: `rsort: nope.txt: No such file or directory`.
- Conflicting options are usage errors with exit code 2: two ordering modes
  (`-n -g`), `-c` with `-m`, `-c` with more than one file, and
  `--files0-from` with FILE operands. sort exits 1 or ignores them.
- `--files0-from -` reads the file list from stdin.
- Invalid UTF-8 in a UTF-8 locale compares by bytes; sort fails with
  "Illegal byte sequence".
- Multibyte encodings other than UTF-8 (ja_JP.eucJP, ja_JP.SJIS, zh_CN.GBK,
  zh_CN.GB18030, zh_TW.Big5, ko_KR.eucKR, ...) are handled as single bytes:
  `-k` character offsets and `-d`/`-i`/`-f` go by byte there. Collation still
  uses the locale.
- In kk_KZ.PT154, whose collation macOS can't use (strcoll fails), the order
  differs.
- `-R` uses a different hash, so a given `--random-source` gives a different
  (but repeatable) order than sort's.
- `-S`, `-T`, `--parallel`, `--batch-size`, `--compress-program`,
  `--radixsort`, `--mergesort`, `--qsort`, `--heapsort` and `--mmap` are
  accepted and ignored: rsort sorts in memory.
- Not supported: `--debug`.

## Development

Requirements: [rustup](https://rustup.rs), [just](https://just.systems) and
`cargo-llvm-cov`. Releases also need `zig`, `cargo-zigbuild` (static Linux builds)
and `gh`.

```sh
brew install just cargo-llvm-cov            # macOS
brew install zig cargo-zigbuild gh          # macOS, for releases
cargo install just cargo-llvm-cov           # Linux (install zig and gh from your package manager)
```

| Task | Command |
| --- | --- |
| Full local CI (fmt, clippy, tests, 90% coverage gate, static check) | `just ci` |
| Run tests | `just test` |
| Accept changed `--help` snapshots | `just snapshots` |
| Benchmark against the system sort (`--quick`, `--only WORD`, `--help`) | `just bench` |
| Compare output with the system sort over sort's own test matrix (`--quick`, `--big`, `--help`) | `just compat` |
| Fuzz against the system sort: keys, numbers, blanks, merge, options in 12 locales (`-n`, `--seed`, `--target`, `--help`) | `just fuzz` |
| Build release archives | `just dist` |
| Cut a release (describe it under `[Unreleased]` in CHANGELOG.md first) | `just release 1.2.0` |
| Upgrade toolchain and dependencies | `just upgrade` |

The interface is defined in [`cli.toml`](cli.toml): commands, flags, help text,
exit codes and JSON output schemas. `build.rs` turns it into Rust at build
time, so the binary doesn't parse it, and `tests/conformance.rs` fails if the
code drifts from it.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or
[MIT](LICENSE-MIT), at your option.

rsort follows the logic of macOS sort (FreeBSD's BSD sort, 2-clause BSD
license) closely enough to match its output, and its tests include a port of
NetBSD's sort tests; see [`NOTICE`](NOTICE) for both notices.
