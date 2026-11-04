//! Per-invocation context: output streams, output mode, color and
//! interactivity decisions. Commands write data to `out` (stdout) and
//! everything else (progress, prompts) to `err` (stderr).

use crate::error::AppError;
use serde::Serialize;
use std::io::{BufRead, IsTerminal, Write};

/// What the process is attached to, captured once so tests can fake it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Terminal {
    pub stdin_tty: bool,
    pub stdout_tty: bool,
    pub stderr_tty: bool,
}

impl Terminal {
    pub fn detect() -> Self {
        Terminal {
            stdin_tty: std::io::stdin().is_terminal(),
            stdout_tty: std::io::stdout().is_terminal(),
            stderr_tty: std::io::stderr().is_terminal(),
        }
    }
}

pub struct Ctx<'a> {
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
    pub input: &'a mut dyn BufRead,
    /// `--json` was passed.
    pub json: bool,
    /// Whether stderr output may use color.
    pub color: bool,
}

impl Ctx<'_> {
    /// Write a data result: JSON when `--json`, otherwise the human rendering.
    pub fn emit<T: Serialize>(
        &mut self,
        value: &T,
        human: impl FnOnce(&mut dyn Write) -> std::io::Result<()>,
    ) -> Result<(), AppError> {
        if self.json {
            self.emit_json(value)
        } else {
            human(self.out)?;
            Ok(())
        }
    }

    /// Write a JSON value as a single line on stdout.
    pub fn emit_json<T: Serialize>(&mut self, value: &T) -> Result<(), AppError> {
        let line = serde_json::to_string(value)
            .map_err(|e| AppError::Other(format!("failed to serialize output: {e}")))?;
        writeln!(self.out, "{line}")?;
        Ok(())
    }
}

/// Decide whether to color stderr output.
///
/// Precedence: explicit `--color` / config value, then `NO_COLOR`, then
/// whether stderr is a terminal.
pub fn decide_color(choice: crate::cli::ColorWhen, no_color_env: bool, stderr_tty: bool) -> bool {
    use crate::cli::ColorWhen;
    match choice {
        ColorWhen::Always => true,
        ColorWhen::Never => false,
        ColorWhen::Auto => !no_color_env && stderr_tty,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::ColorWhen;

    fn with_ctx<R>(
        #[allow(unused_variables)] interactive: bool,
        input: &str,
        f: impl FnOnce(&mut Ctx) -> R,
    ) -> (R, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut input = std::io::Cursor::new(input.as_bytes().to_vec());
        let r = {
            let mut ctx = Ctx {
                out: &mut out,
                err: &mut err,
                input: &mut input,
                json: false,
                color: false,
            };
            f(&mut ctx)
        };
        (
            r,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn color_precedence() {
        assert!(decide_color(ColorWhen::Always, true, false));
        assert!(!decide_color(ColorWhen::Never, false, true));
        assert!(decide_color(ColorWhen::Auto, false, true));
        assert!(!decide_color(ColorWhen::Auto, true, true));
        assert!(!decide_color(ColorWhen::Auto, false, false));
    }

    #[test]
    fn emit_switches_on_json() {
        let (_, out, _) = with_ctx(false, "", |ctx| {
            ctx.emit(&serde_json::json!({"a": 1}), |w| writeln!(w, "a is 1"))
                .unwrap();
            ctx.json = true;
            ctx.emit(&serde_json::json!({"a": 1}), |w| writeln!(w, "a is 1"))
                .unwrap();
        });
        assert_eq!(out, "a is 1\n{\"a\":1}\n");
    }
}
