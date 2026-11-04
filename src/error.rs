//! Application errors and their mapping to exit codes.
//!
//! Every variant maps to an exit code declared in `cli.toml` (or a built-in
//! one). Humans get a miette report (or `<name>: <message>` with
//! `error_style = "plain"`); with `--json`, errors are written to stderr as
//! `{"error": {"code", "exit_code", "message", "hint"}}`.

use crate::spec::{ErrorStyle, Spec};
use miette::Diagnostic;
use serde::Serialize;

/// Exit codes used by [`AppError`], by name. Must match `cli.toml`
/// (checked by `tests/conformance.rs`). `error` must equal `[cli].error_exit_code`.
pub const EXIT_CODES: &[(&str, i32)] = &[("error", 1), ("usage", 2)];

#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum AppError {
    #[error("{0}")]
    #[diagnostic(code(usage))]
    Usage(String),

    #[error("I/O error: {0}")]
    #[diagnostic(code(error))]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    #[diagnostic(code(error))]
    Other(String),
}

impl AppError {
    /// Stable machine-readable error code (matches the `cli.toml` name).
    pub fn code(&self) -> &'static str {
        match self {
            AppError::Usage(_) => "usage",
            AppError::Io(_) | AppError::Other(_) => "error",
        }
    }

    /// Process exit code for this error.
    pub fn exit_code(&self) -> i32 {
        let code = self.code();
        EXIT_CODES
            .iter()
            .find(|(name, _)| *name == code)
            .map(|(_, n)| *n)
            .unwrap_or(Spec::get().cli.error_exit_code)
    }

    /// The `help(...)` hint, if any.
    pub fn hint(&self) -> Option<String> {
        self.help().map(|h| h.to_string())
    }
}

/// JSON shape of an error on stderr under `--json`.
#[derive(Debug, Serialize)]
pub struct JsonError<'a> {
    pub code: &'a str,
    pub exit_code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

impl<'a> JsonError<'a> {
    pub fn from_app(err: &'a AppError) -> Self {
        JsonError {
            code: err.code(),
            exit_code: err.exit_code(),
            message: err.to_string(),
            hint: err.hint(),
        }
    }

    /// Serialize as the single-line `{"error": ...}` envelope.
    pub fn to_line(&self) -> String {
        serde_json::json!({ "error": self }).to_string()
    }
}

/// Render an error for humans in the style `cli.toml` asks for.
pub fn render_human(err: &AppError, color: bool) -> String {
    let spec = Spec::get();
    match spec.cli.error_style {
        ErrorStyle::Rich => render_rich(err, color),
        ErrorStyle::Plain => render_plain(err, &spec.cli.name),
    }
}

/// `<name>: <message>`, then `<name>: hint: <hint>` when there is one.
pub fn render_plain(err: &AppError, name: &str) -> String {
    match err.hint() {
        Some(hint) => format!("{name}: {err}\n{name}: hint: {hint}"),
        None => format!("{name}: {err}"),
    }
}

/// A miette report, with or without color.
pub fn render_rich(err: &AppError, color: bool) -> String {
    let theme = if color {
        miette::GraphicalTheme::unicode()
    } else {
        miette::GraphicalTheme::unicode_nocolor()
    };
    let mut out = String::new();
    let handler = miette::GraphicalReportHandler::new_themed(theme);
    if handler.render_report(&mut out, err).is_err() {
        out = format!("error: {err}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_has_an_exit_code() {
        let errors = [
            AppError::Usage("u".into()),
            AppError::Io(std::io::Error::other("io")),
            AppError::Other("o".into()),
        ];
        for err in &errors {
            assert!(EXIT_CODES.iter().any(|(name, _)| *name == err.code()));
        }
        assert_eq!(AppError::Usage("u".into()).exit_code(), 2);
        assert_eq!(
            AppError::Other("o".into()).exit_code(),
            Spec::get().cli.error_exit_code
        );
    }

    #[test]
    fn rich_render_works_with_and_without_color() {
        let err = AppError::Other("boom".into());
        for color in [true, false] {
            assert!(render_rich(&err, color).contains("boom"));
        }
    }

    #[test]
    fn plain_render_prefixes_the_name() {
        let err = AppError::Other("cannot read: x".into());
        assert_eq!(render_plain(&err, "tool"), "tool: cannot read: x");
    }

    #[test]
    fn human_render_follows_error_style() {
        let err = AppError::Other("boom".into());
        let out = render_human(&err, false);
        assert!(out.contains("boom"), "{out}");
    }
}
