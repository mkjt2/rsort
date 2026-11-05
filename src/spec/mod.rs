//! The interface from `cli.toml`, as static data.
//!
//! `build.rs` reads `cli.toml` and generates [`SPEC`] (help text, help and
//! version flags, the error exit code) and [`SCHEMA`], so the binary never parses
//! TOML. The typed model of the file lives in `model.rs`, which only
//! `build.rs` and the tests compile; `tests/conformance.rs` checks the clap
//! definitions against it.

/// Usage errors always exit with this code.
pub const USAGE_EXIT_CODE: i32 = 2;

/// Everything the binary needs from `cli.toml`.
#[derive(Debug, Clone, Copy)]
pub struct Interface {
    pub name: &'static str,
    pub about: &'static str,
    pub long_about: Option<&'static str>,
    /// Examples, exit codes and the schema pointer shown after `--help`.
    pub after_long_help: &'static str,
    /// Flags that print help, e.g. `["--help"]`.
    pub help_flags: &'static [&'static str],
    /// Flags that print the version; empty for none.
    pub version_flags: &'static [&'static str],
    /// Help text of the flags and positional args, by clap id.
    pub help: &'static [(&'static str, &'static str)],
    /// Exit code for general errors.
    pub error_exit_code: i32,
}

/// Split declared flags like `["-h", "--help"]` into (short, long).
pub fn split_flags<S: AsRef<str>>(flags: &[S]) -> (Option<char>, Option<&str>) {
    let mut short = None;
    let mut long = None;
    for f in flags {
        let f = f.as_ref();
        if let Some(l) = f.strip_prefix("--") {
            long = Some(l);
        } else if let Some(s) = f.strip_prefix('-') {
            short = s.chars().next();
        }
    }
    (short, long)
}

include!(concat!(env!("OUT_DIR"), "/spec.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_flags_finds_short_and_long() {
        assert_eq!(split_flags(&["-h", "--help"]), (Some('h'), Some("help")));
        assert_eq!(split_flags(&["--help"]), (None, Some("help")));
        assert_eq!(split_flags::<&str>(&[]), (None, None));
    }

    #[test]
    fn generated_spec_is_this_crate() {
        assert_eq!(SPEC.name, env!("CARGO_PKG_NAME"));
        assert!(SPEC.after_long_help.contains("Exit codes:"));
        assert!(SCHEMA.contains(env!("CARGO_PKG_VERSION")));
    }
}
