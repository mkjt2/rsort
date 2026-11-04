//! Library half of the CLI: everything except process setup lives here so
//! it can be tested in-process. `main.rs` only wires real stdio into [`run`].

pub mod cli;
pub mod commands;
pub mod ctx;
pub mod error;
pub mod logging;
pub mod sort;
pub mod spec;

use clap::FromArgMatches;
use cli::Cli;
use cli::ColorWhen;
use ctx::{Ctx, Terminal};
use error::{AppError, JsonError};
use spec::Spec;
use std::ffi::OsString;
use std::io::{BufRead, ErrorKind, Write};

/// Process-level inputs, injectable for tests.
pub struct Io<'a> {
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
    pub input: &'a mut dyn BufRead,
    pub term: Terminal,
    pub env: &'a dyn Fn(&str) -> Option<String>,
}

/// Parse `args` (including argv\[0\]), run the command, and return the exit code.
pub fn run(args: Vec<OsString>, io: Io) -> i32 {
    let spec = Spec::get();
    let json = spec.kit.json && args.iter().any(|a| a == "--json");
    let no_color = (io.env)("NO_COLOR").is_some_and(|v| !v.is_empty());

    // `<name> --generate <what>` is handled before parsing so it works in every
    // shape and stays out of --help. Release scripts use it.
    if let [_, flag, what] = args.as_slice()
        && flag == "--generate"
    {
        let result = commands::generate::run(io.out, &what.to_string_lossy());
        return finish(result, false, false, io.err);
    }

    let matches = match cli::command().try_get_matches_from(args) {
        Ok(m) => m,
        Err(e) => return clap_error(e, json, no_color, io),
    };
    let cli = match Cli::from_arg_matches(&matches) {
        Ok(cli) => cli,
        Err(e) => return clap_error(e, json, no_color, io),
    };
    let settings = cli.global.settings();

    let color_choice = settings.color.unwrap_or(ColorWhen::Auto);
    let color = ctx::decide_color(color_choice, no_color, io.term.stderr_tty);
    logging::init(settings.verbose, settings.quiet, color);

    let mut ctx = Ctx {
        out: io.out,
        err: io.err,
        input: io.input,
        json,
        color,
    };
    let locale = sort::collate::locale_name(io.env);
    let locale = sort::collate::Locale::activate(locale.as_deref());
    let result = commands::root::run(&mut ctx, &cli, locale);
    finish(result, ctx.json, ctx.color, ctx.err)
}

/// Map a command result to an exit code, reporting errors on stderr.
pub fn finish(result: Result<(), AppError>, json: bool, color: bool, err: &mut dyn Write) -> i32 {
    match result {
        Ok(()) => 0,
        Err(e) => report(&e, json, color, err),
    }
}

/// Write `err` to stderr (JSON or human) and return its exit code. A closed
/// stdout pipe (`| head`) is not an error: it exits quietly with 0. An error
/// whose message is empty exits with its code silently for humans (predicate
/// modes like `grep -q` or `sort --check=quiet`).
pub fn report(err: &AppError, json: bool, color: bool, out: &mut dyn Write) -> i32 {
    if matches!(err, AppError::Io(e) if e.kind() == ErrorKind::BrokenPipe) {
        return 0;
    }
    if !json && err.to_string().is_empty() {
        return err.exit_code();
    }
    let text = if json {
        JsonError::from_app(err).to_line()
    } else {
        error::render_human(err, color)
    };
    let _ = writeln!(out, "{}", text.trim_end());
    err.exit_code()
}

/// Flush buffered stdout at exit, keeping `code` unless flushing fails for a
/// reason other than a closed pipe.
pub fn flush_stdout(out: &mut dyn Write, err: &mut dyn Write, code: i32) -> i32 {
    match out.flush() {
        Ok(()) => code,
        Err(e) if e.kind() == ErrorKind::BrokenPipe => code,
        Err(e) => report(&AppError::Io(e), false, false, err),
    }
}

/// Handle clap's parse outcomes: help/version go to stdout with exit 0;
/// everything else is a usage error (exit 2).
fn clap_error(e: clap::Error, json: bool, no_color: bool, io: Io) -> i32 {
    use clap::error::ErrorKind;
    let color = !no_color && io.term.stderr_tty;
    match e.kind() {
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
            let _ = write!(io.out, "{}", e.render());
            0
        }
        _ if json => {
            let rendered = e.render().to_string();
            let message = rendered
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("invalid usage")
                .trim_start_matches("error: ")
                .to_string();
            let err = JsonError {
                code: "usage",
                exit_code: spec::USAGE_EXIT_CODE,
                message,
                hint: Some("run with --help for usage".into()),
            };
            let _ = writeln!(io.err, "{}", err.to_line());
            spec::USAGE_EXIT_CODE
        }
        _ => {
            let rendered = e.render();
            let text = if color {
                rendered.ansi().to_string()
            } else {
                rendered.to_string()
            };
            let _ = write!(io.err, "{text}");
            spec::USAGE_EXIT_CODE
        }
    }
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
        assert_eq!(report(&e, false, false, &mut err), 0);
        assert!(err.is_empty());
        assert_eq!(
            flush_stdout(&mut Failing(ErrorKind::BrokenPipe), &mut err, 3),
            3
        );
        assert!(err.is_empty());
    }

    #[test]
    fn empty_messages_exit_silently_for_humans() {
        let mut err = Vec::new();
        let e = AppError::Other(String::new());
        assert_eq!(report(&e, false, false, &mut err), e.exit_code());
        assert!(err.is_empty());
        assert_eq!(report(&e, true, false, &mut err), e.exit_code());
        assert!(!err.is_empty(), "JSON mode still reports");
    }

    #[test]
    fn other_flush_failures_are_errors() {
        let mut err = Vec::new();
        let code = flush_stdout(&mut Failing(ErrorKind::Other), &mut err, 0);
        assert_eq!(code, Spec::get().cli.error_exit_code);
        assert!(!err.is_empty());
        assert_eq!(flush_stdout(&mut Vec::new(), &mut err, 7), 7);
    }
}
