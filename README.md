# rsort

[![CI](https://github.com/mkjt2/rsort/actions/workflows/ci.yml/badge.svg)](https://github.com/mkjt2/rsort/actions/workflows/ci.yml)

A Rust port of the macOS `sort` (BSD sort 2.3-Apple), compatible with the
original.

## Why

Just for fun: to see what the Rust rage is all about. Take a classic Unix
tool, write a compatible port, and see how it compares.

## Benchmarks

Each scenario sorts 500,000 generated lines with `/usr/bin/sort` and with
rsort, alternating between them: 1 warmup and 5 timed runs each, median wall
time, peak resident memory, and a check that both sorted the same way.
Apple M4 (10 cores, 16 GiB), macOS sort 2.3-Apple (201). Run it yourself with
`just bench`.

The faster or smaller of the two is in bold yellow.

**Time** (lower ratio is better; rsort is faster in all 17 scenarios, 0.41×
sort's time on average):

| Scenario | Flags | Locale | sort | rsort | rsort/sort |
| --- | --- | --- | ---: | ---: | ---: |
| text | | C | 112 ms | $\color{yellow}{\textbf{55 ms}}$ | 0.49× |
| text | | en_US.UTF-8 | 725 ms | $\color{yellow}{\textbf{250 ms}}$ | 0.35× |
| reverse | `-r` | C | 115 ms | $\color{yellow}{\textbf{59 ms}}$ | 0.51× |
| fold case | `-f` | C | 151 ms | $\color{yellow}{\textbf{67 ms}}$ | 0.44× |
| fold case | `-f` | en_US.UTF-8 | 825 ms | $\color{yellow}{\textbf{401 ms}}$ | 0.49× |
| dictionary | `-d` | C | 148 ms | $\color{yellow}{\textbf{79 ms}}$ | 0.53× |
| unique | `-u` | C | 52 ms | $\color{yellow}{\textbf{37 ms}}$ | 0.70× |
| numeric | `-n` | C | 309 ms | $\color{yellow}{\textbf{68 ms}}$ | 0.22× |
| general numeric | `-g` | C | 206 ms | $\color{yellow}{\textbf{66 ms}}$ | 0.32× |
| human numeric | `-h` | C | 254 ms | $\color{yellow}{\textbf{57 ms}}$ | 0.22× |
| version | `-V` | C | 860 ms | $\color{yellow}{\textbf{362 ms}}$ | 0.42× |
| month | `-M` | C | 410 ms | $\color{yellow}{\textbf{111 ms}}$ | 0.27× |
| keys | `-t '\t' -k2,2 -k3,3n` | C | 260 ms | $\color{yellow}{\textbf{98 ms}}$ | 0.38× |
| keys | `-t '\t' -k2,2 -k3,3n` | en_US.UTF-8 | 729 ms | $\color{yellow}{\textbf{320 ms}}$ | 0.44× |
| check sorted | `-c` | C | 30 ms | $\color{yellow}{\textbf{18 ms}}$ | 0.60× |
| merge | `-m` | C | 46 ms | $\color{yellow}{\textbf{31 ms}}$ | 0.68× |
| random | `-R --random-source FILE` | C | 222 ms | $\color{yellow}{\textbf{65 ms}}$ | 0.29× |

**Peak memory**:

| Scenario | Flags | Locale | sort | rsort |
| --- | --- | --- | ---: | ---: |
| text | | C | 40 MiB | $\color{yellow}{\textbf{30 MiB}}$ |
| text | | en_US.UTF-8 | 60 MiB | $\color{yellow}{\textbf{40 MiB}}$ |
| fold case | `-f` | en_US.UTF-8 | 101 MiB | $\color{yellow}{\textbf{40 MiB}}$ |
| numeric | `-n` | C | 44 MiB | $\color{yellow}{\textbf{25 MiB}}$ |
| month | `-M` | C | 65 MiB | $\color{yellow}{\textbf{28 MiB}}$ |
| keys | `-t '\t' -k2,2 -k3,3n` | C | 90 MiB | $\color{yellow}{\textbf{42 MiB}}$ |
| keys | `-t '\t' -k2,2 -k3,3n` | en_US.UTF-8 | 146 MiB | $\color{yellow}{\textbf{52 MiB}}$ |
| check sorted | `-c` | C | $\color{yellow}{\textbf{1 MiB}}$ | 2 MiB |
| merge | `-m` | C | $\color{yellow}{\textbf{1 MiB}}$ | 2 MiB |

## Validation

rsort is checked against macOS sort's own tests and against `/usr/bin/sort`
itself:

- **sort's test suite.** Apple's source for `/usr/bin/sort`
  ([text_cmds](https://github.com/apple-oss-distributions/text_cmds)) ships
  two suites, both ported (see [tests/upstream](tests/upstream/README.md)):
  - `t_sort.sh`: all 43 cases in `tests/upstream.rs`, each also run against
    `/usr/bin/sort`.
  - `run.sh`: its differential matrix of locales, flags, separators and key
    positions in `just compat`. The full run matches sort on 2,968,370 of
    2,968,370 cases, plus a 155 MB input in every locale.
- **Fuzzing.** `just fuzz` generates random keys, numbers, white space,
  merges and option syntax in 12 locales and compares exit status and output
  with sort.
- **Unit and integration tests**, 97% line coverage, and CI on macOS (where
  the suites above run against the runner's sort) and Linux.

## Install

```sh
curl -fsSL https://github.com/mkjt2/rsort/releases/latest/download/install.sh | sh
```

Prebuilt binaries for macOS and Linux (static) are on each
[release](https://github.com/mkjt2/rsort/releases). From source:
`cargo install --locked --git https://github.com/mkjt2/rsort`.

```sh
rsort names.txt                      # sort lines
du -sh * | rsort -hr                 # largest human-readable sizes first
rsort -t: -k3,3n /etc/passwd         # third field, numerically
rsort -u -o words.txt words.txt      # deduplicate in place
rsort -C data.txt && echo sorted     # check order via the exit code
rsort --help                         # every flag, with examples and exit codes
```

## Differences from macOS sort

- Errors name the file: `rsort: nope.txt: No such file or directory`.
- Conflicting options are usage errors with exit code 2: two ordering modes
  (`-n -g`), `-c` with `-m`, `-c` with more than one file, and
  `--files0-from` with FILE operands. sort exits 1 or ignores them.
- `--files0-from -` reads the file list from stdin.
- Invalid UTF-8 in a UTF-8 locale compares by bytes; sort fails with
  "Illegal byte sequence".
- Multibyte encodings other than UTF-8 (ja_JP.eucJP, ja_JP.SJIS, zh_CN.GBK,
  ko_KR.eucKR, ...) are handled byte by byte for `-k` offsets and
  `-d`/`-i`/`-f`.
- kk_KZ.PT154, whose collation macOS itself can't use, sorts differently.
- `-R` uses a different hash, so a `--random-source` gives a different (but
  repeatable) order.
- `-S`, `-T`, `--parallel`, `--batch-size`, `--compress-program`,
  `--radixsort`, `--mergesort`, `--qsort`, `--heapsort` and `--mmap` are
  accepted and ignored: rsort sorts in memory.
- `--debug` is not supported.
- The Linux builds are static musl binaries without locale collation: text
  compares in byte order there.

## Development

Needs [rustup](https://rustup.rs), [just](https://just.systems) and
`cargo-llvm-cov` (`brew install just cargo-llvm-cov`). Releases also need
`zig`, `cargo-zigbuild` and `gh`.

| Task | Command |
| --- | --- |
| Local CI: fmt, clippy, tests, coverage gate, static check | `just ci` |
| Benchmark against the system sort | `just bench` |
| sort's differential suite (`--quick`, `--big`) | `just compat` |
| Fuzz against the system sort (`-n`, `--seed`, `--target`) | `just fuzz` |
| Release | `just release 1.2.0` |

The interface (flags, help text, exit codes) is defined in
[`cli.toml`](cli.toml); `tests/conformance.rs` keeps the code in line with it.

## License

[Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option. rsort
follows the logic of FreeBSD's BSD sort (which macOS ships) and ports NetBSD's
sort tests; see [`NOTICE`](NOTICE).
