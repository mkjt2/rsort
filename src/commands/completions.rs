//! `completions <shell>`.

use crate::ctx::Ctx;
use crate::error::AppError;
use std::io::Write;

pub fn run(ctx: &mut Ctx, shell: clap_complete::Shell) -> Result<(), AppError> {
    write(ctx.out, shell)
}

/// Write the completion script for `shell`. Rendered into memory first:
/// clap_complete panics on write errors, and a closed pipe must not panic.
pub fn write(out: &mut dyn Write, shell: clap_complete::Shell) -> Result<(), AppError> {
    let mut cmd = crate::cli::command();
    let name = cmd.get_name().to_string();
    let mut script = Vec::new();
    clap_complete::generate(shell, &mut cmd, name, &mut script);
    out.write_all(&script)?;
    Ok(())
}
