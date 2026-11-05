//! Application errors and their exit codes. Errors are reported on stderr as
//! `rsort: <message>`.

use crate::spec::SPEC;

/// Exit codes used by [`AppError`], by name. Must match `cli.toml`
/// (checked by `tests/conformance.rs`). `error` must equal `[cli].error_exit_code`.
pub const EXIT_CODES: &[(&str, i32)] = &[("error", 2), ("usage", 2), ("disorder", 1)];

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Usage(String),
    #[error("{0}")]
    Disorder(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

impl AppError {
    /// The error's name in `cli.toml`'s exit codes.
    pub fn code(&self) -> &'static str {
        match self {
            AppError::Usage(_) => "usage",
            AppError::Disorder(_) => "disorder",
            AppError::Io(_) | AppError::Other(_) => "error",
        }
    }

    /// Process exit code for this error.
    pub fn exit_code(&self) -> i32 {
        let code = self.code();
        EXIT_CODES
            .iter()
            .find(|(name, _)| *name == code)
            .map_or(SPEC.error_exit_code, |(_, n)| *n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_has_an_exit_code() {
        let errors = [
            AppError::Usage("u".into()),
            AppError::Disorder("d".into()),
            AppError::Io(std::io::Error::other("io")),
            AppError::Other("o".into()),
        ];
        for err in &errors {
            assert!(EXIT_CODES.iter().any(|(name, _)| *name == err.code()));
        }
        assert_eq!(AppError::Usage("u".into()).exit_code(), 2);
        assert_eq!(AppError::Disorder("d".into()).exit_code(), 1);
        assert_eq!(
            AppError::Other("o".into()).exit_code(),
            SPEC.error_exit_code
        );
    }
}
