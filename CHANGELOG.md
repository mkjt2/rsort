# Changelog

All notable changes to this project are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

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
