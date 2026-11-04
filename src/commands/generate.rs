//! `--generate <what>`: man page, shell completions and the machine-readable
//! interface. Hidden from --help; release scripts use it to build archives.

use crate::error::AppError;
use crate::spec::Spec;
use std::io::Write;

/// Everything `--generate` accepts.
pub const TARGETS: &[&str] = &[
    "man",
    "schema",
    "complete-bash",
    "complete-elvish",
    "complete-fish",
    "complete-powershell",
    "complete-zsh",
];

pub fn run(out: &mut dyn Write, what: &str) -> Result<(), AppError> {
    let shell = what
        .strip_prefix("complete-")
        .and_then(|s| s.parse::<clap_complete::Shell>().ok());
    match (what, shell) {
        ("man", _) => clap_mangen::Man::new(crate::cli::command()).render(out)?,
        ("schema", _) => writeln!(out, "{}", super::schema::document(Spec::get()))?,
        (_, Some(shell)) => super::completions::write(out, shell)?,
        _ => {
            return Err(AppError::Usage(format!(
                "cannot generate `{what}`; choose one of: {}",
                TARGETS.join(", ")
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_target_generates_output() {
        for what in TARGETS {
            let mut out = Vec::new();
            run(&mut out, what).unwrap();
            assert!(!out.is_empty(), "{what}");
        }
    }

    #[test]
    fn unknown_target_is_a_usage_error() {
        let err = run(&mut Vec::new(), "complete-tcsh").unwrap_err();
        assert_eq!(err.code(), "usage");
        assert!(err.to_string().contains("complete-zsh"));
    }
}
