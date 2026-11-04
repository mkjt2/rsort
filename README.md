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

```sh
rsort --help          # human help, with examples and exit codes
rsort schema          # the full interface as JSON (for scripts and agents)
```

Every command that produces data accepts `--json`. With `--json`, errors are
written to stderr as `{"error": {"code", "exit_code", "message", "hint"}}`.
Pass `--no-input` (or set `RSORT_NO_INPUT=1`) to guarantee the tool never
prompts.

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
| Build release archives | `just dist` |
| Cut a release (describe it under `[Unreleased]` in CHANGELOG.md first) | `just release 1.2.0` |
| Upgrade toolchain and dependencies | `just upgrade` |

The interface is defined in [`cli.toml`](cli.toml): commands, flags, help text,
exit codes and JSON output schemas. `tests/conformance.rs` fails if the code
drifts from it.
