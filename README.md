# rsort

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

### Locale

Like sort, rsort follows `LC_ALL`, then `LC_COLLATE`, then `LANG`. In a non-C
locale, text compares with the locale's collation (`strcoll`), `-d`/`-i`/`-f`
and `-k` character offsets work by character in UTF-8, and `-n`/`-h`/`-g` use
the locale's decimal point and thousands separator (`1,000` = 1000 in en_US).
Set `LC_ALL=C` for byte order.

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
- Month names (`-M`) are always the English abbreviations JAN to DEC.
- `-R` uses a different hash, so a given `--random-source` gives a different
  (but repeatable) order than sort's.
- A thousands separator before any digit and followed by a zero (`,05` in
  en_US) does not reproduce sort's ordering.
- `-S`, `-T`, `--parallel`, `--batch-size`, `--compress-program`,
  `--radixsort`, `--mergesort`, `--qsort`, `--heapsort` and `--mmap` are
  accepted and ignored: rsort sorts in memory.
- Not supported: the obsolete `+POS1 -POS2` keys (an argument starting with
  `+` is a file name) and `--debug`.

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
| Build release archives | `just dist` |
| Cut a release (describe it under `[Unreleased]` in CHANGELOG.md first) | `just release 1.2.0` |
| Upgrade toolchain and dependencies | `just upgrade` |

The interface is defined in [`cli.toml`](cli.toml): commands, flags, help text,
exit codes and JSON output schemas. `tests/conformance.rs` fails if the code
drifts from it.
