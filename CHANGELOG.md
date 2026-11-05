# Changelog

All notable changes to this project are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.2.0] - 2026-11-05

### Added
- The obsolete `+POS1 [-POS2]` key syntax, as in sort.
- Every locale category is read as sort reads it (`LC_ALL`, then the
  category's own variable, then `LANG`): `LC_CTYPE` for character classes
  and UTF-8, `LC_NUMERIC` for the decimal point, thousands separator and
  minus sign, and `LC_TIME` for `-M` month names.
- `GNUSORT_NUMERIC_COMPATIBILITY` and `GNUSORT_COMPATIBLE_BLANKS`.
- License files (MIT and Apache-2.0) and a NOTICE crediting FreeBSD and
  NetBSD sort.

### Changed
- Faster and leaner: 0.41x the time of macOS sort on average and faster in
  every benchmark scenario, with less memory; the binary is 850 KB, down
  from 1.3 MB.
- `-t` reads its argument as sort does: a leading backslash is dropped, so
  `-t '\t'` is the letter `t` (use `-t $'\t'` for a tab) and `-t '\0'` is NUL.
- A repeated option takes its last value, and option values may start with
  `-`, as with sort's getopt.
- `-c`, `-C` and `--check` can be combined, and any silent form makes the
  check silent; `--sort=TYPE` combines with the mode flags as in sort.

### Fixed
- Text in a locale collates exactly as sort's does (each NUL-separated part,
  then length), and lines that collate equal keep their input order.
- `-n` and `-h` read numbers as sort does: the locale's minus sign and
  multibyte separators, at most 128 digits per part, and a `-t` equal to the
  decimal point stops the fraction.
- `-g` starts reading numbers where sort does; `-V` handles NUL bytes and
  equal versions as sort does.
- Locale names without an encoding, such as `en_US`, are UTF-8.

### Removed
- `RUST_LOG`, which had nothing to log.

## [0.1.0] - 2026-11-04

### Added
- rsort, a Rust clone of the macOS `sort` (BSD sort 2.3-Apple) with the same
  flags, output and exit codes.
- Sort, merge (`-m`) and order checks (`-c`, `-C`, `--check=quiet|silent`),
  with `-r`, `-u`, `-s`, `-o` and `-z`.
- Keys with `-k` and `-t`, the options `-b -d -f -i`, and the modes
  `-n -g -h -M -V -R` (or `--sort=TYPE`), plus `--random-source`.
- Locale support: `LC_ALL`, `LC_COLLATE` and `LANG` choose text collation and
  the number format for `-n`, `-h` and `-g`.
- `--files0-from` reads NUL-separated file names (`-` = stdin).
- `-S`, `-T`, `--parallel`, `--batch-size`, `--compress-program`,
  `--radixsort`, `--mergesort`, `--qsort`, `--heapsort` and `--mmap` are
  accepted and ignored.
- `--generate man|schema|complete-<shell>`.
- See the README for differences from macOS sort.
