//! Logging to stderr via `tracing`.
//!
//! Default level is `warn`; `-v` info, `-vv` debug, `-vvv` trace, `-q` error.
//! `RUST_LOG` overrides the flags entirely.

/// The filter directive for the given flags.
pub fn level(verbose: u8, quiet: bool) -> &'static str {
    if quiet {
        return "error";
    }
    match verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    }
}

/// Install the global subscriber. Safe to call more than once (later calls
/// are ignored), which matters for in-process tests.
pub fn init(verbose: u8, quiet: bool, color: bool) {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_env("RUST_LOG").unwrap_or_else(|_| {
        EnvFilter::new(format!(
            "{}={}",
            env!("CARGO_CRATE_NAME"),
            level(verbose, quiet)
        ))
    });
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(color)
        .without_time()
        .with_target(false)
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels() {
        assert_eq!(level(0, false), "warn");
        assert_eq!(level(1, false), "info");
        assert_eq!(level(2, false), "debug");
        assert_eq!(level(9, false), "trace");
        assert_eq!(level(3, true), "error");
    }

    #[test]
    fn init_twice_is_harmless() {
        init(0, false, false);
        init(1, false, true);
    }
}
