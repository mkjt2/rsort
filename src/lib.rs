//! Library half of the CLI: everything except process setup lives here so
//! it can be tested in-process. `main.rs` only wires real stdio into [`run`].

pub mod cli;
pub mod commands;
pub mod error;
pub mod sort;
pub mod spec;

use clap::FromArgMatches;
use cli::Cli;
use commands::root::Ctx;
use error::AppError;
use spec::SPEC;
use std::ffi::OsString;
use std::io::{BufRead, ErrorKind, Write};

/// Process-level inputs, injectable for tests.
pub struct Io<'a> {
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
    pub input: &'a mut dyn BufRead,
    /// Whether stderr is a terminal (usage errors are colored there).
    pub stderr_tty: bool,
    pub env: &'a dyn Fn(&str) -> Option<String>,
}

/// Parse `args` (including argv\[0\]), run the command, and return the exit code.
pub fn run(args: Vec<OsString>, io: Io) -> i32 {
    // `<name> --generate <what>` is handled before parsing so it works in every
    // shape and stays out of --help. Release scripts use it.
    if let [_, flag, what] = args.as_slice()
        && flag == "--generate"
    {
        let result = commands::generate::run(io.out, &what.to_string_lossy());
        return finish(result, io.err);
    }
    // sort rewrites the obsolete `+POS1 -POS2` keys before parsing options.
    let args = match sort::obsolete::translate(args) {
        Ok(args) => args,
        Err(message) => return finish(Err(AppError::Usage(message)), io.err),
    };
    let parsed = cli::command()
        .try_get_matches_from(args)
        .and_then(|m| Cli::from_arg_matches(&m));
    let cli = match parsed {
        Ok(cli) => cli,
        Err(e) => return clap_error(e, io),
    };
    let locale = sort::collate::Locale::from_env(io.env);
    let mut ctx = Ctx {
        out: io.out,
        input: io.input,
    };
    let result = commands::root::run(&mut ctx, &cli, locale);
    finish(result, io.err)
}

/// Map a command result to an exit code, reporting errors on stderr.
fn finish(result: Result<(), AppError>, err: &mut dyn Write) -> i32 {
    match result {
        Ok(()) => 0,
        Err(e) => report(&e, err),
    }
}

/// Write `err` to stderr as `rsort: <message>` and return its exit code. A
/// closed stdout pipe (`| head`) is not an error: it exits quietly with 0. An
/// error with an empty message exits with its code silently
/// (`--check=quiet`).
pub fn report(err: &AppError, out: &mut dyn Write) -> i32 {
    if matches!(err, AppError::Io(e) if e.kind() == ErrorKind::BrokenPipe) {
        return 0;
    }
    let message = err.to_string();
    if !message.is_empty() {
        let _ = writeln!(out, "{}: {}", SPEC.name, message.trim_end());
    }
    err.exit_code()
}

/// Flush buffered stdout at exit, keeping `code` unless flushing fails for a
/// reason other than a closed pipe.
pub fn flush_stdout(out: &mut dyn Write, err: &mut dyn Write, code: i32) -> i32 {
    match out.flush() {
        Ok(()) => code,
        Err(e) if e.kind() == ErrorKind::BrokenPipe => code,
        Err(e) => report(&AppError::Io(e), err),
    }
}

/// Handle clap's parse outcomes: help/version go to stdout with exit 0;
/// everything else is a usage error (exit 2), colored on a terminal unless
/// NO_COLOR is set.
fn clap_error(e: clap::Error, io: Io) -> i32 {
    use clap::error::ErrorKind;
    if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) {
        let _ = write!(io.out, "{}", e.render());
        return 0;
    }
    let no_color = (io.env)("NO_COLOR").is_some_and(|v| !v.is_empty());
    let rendered = e.render();
    let _ = if io.stderr_tty && !no_color {
        write!(io.err, "{}", rendered.ansi())
    } else {
        write!(io.err, "{rendered}")
    };
    spec::USAGE_EXIT_CODE
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Failing(ErrorKind);

    impl Write for Failing {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(self.0.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(self.0.into())
        }
    }

    #[test]
    fn broken_pipe_is_quiet_success() {
        let mut err = Vec::new();
        let e = AppError::Io(ErrorKind::BrokenPipe.into());
        assert_eq!(report(&e, &mut err), 0);
        assert!(err.is_empty());
        assert_eq!(
            flush_stdout(&mut Failing(ErrorKind::BrokenPipe), &mut err, 3),
            3
        );
        assert!(err.is_empty());
    }

    #[test]
    fn errors_are_prefixed_and_empty_messages_are_silent() {
        let mut err = Vec::new();
        let e = AppError::Other(String::new());
        assert_eq!(report(&e, &mut err), e.exit_code());
        assert!(err.is_empty());
        report(&AppError::Usage("bad".into()), &mut err);
        assert_eq!(err, b"rsort: bad\n");
    }

    #[test]
    fn other_flush_failures_are_errors() {
        let mut err = Vec::new();
        let code = flush_stdout(&mut Failing(ErrorKind::Other), &mut err, 0);
        assert_eq!(code, SPEC.error_exit_code);
        assert!(!err.is_empty());
        assert_eq!(flush_stdout(&mut Vec::new(), &mut err, 7), 7);
    }
}
