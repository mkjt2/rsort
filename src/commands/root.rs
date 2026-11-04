//! The root command (kit:single): what a single-command CLI does when run.
//! Its operands and flags are fields on `Cli` in `src/cli.rs`.

use crate::cli::Cli;
use crate::ctx::Ctx;
use crate::error::AppError;

pub fn run(_ctx: &mut Ctx, _cli: &Cli) -> Result<(), AppError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::FromArgMatches;

    #[test]
    fn placeholder_succeeds_without_output() {
        let args: Vec<&str> = ["x"].into_iter().collect();
        // Parse through cli::command(): it applies cli.toml's help/version flags.
        let matches = crate::cli::command().try_get_matches_from(args).unwrap();
        let cli = Cli::from_arg_matches(&matches).unwrap();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut input = std::io::empty();
        let mut ctx = Ctx {
            out: &mut out,
            err: &mut err,
            input: &mut input,
            json: false,
            color: false,
        };
        run(&mut ctx, &cli).unwrap();
        assert!(out.is_empty() && err.is_empty());
    }
}
