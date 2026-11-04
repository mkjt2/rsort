//! `man [--out-dir DIR]`.

use crate::ctx::Ctx;
use crate::error::AppError;
use std::path::Path;

pub fn run(ctx: &mut Ctx, out_dir: Option<&Path>) -> Result<(), AppError> {
    let cmd = crate::cli::command();
    match out_dir {
        None => clap_mangen::Man::new(cmd).render(ctx.out)?,
        Some(dir) => {
            std::fs::create_dir_all(dir)?;
            clap_mangen::generate_to(cmd, dir)?;
            if !ctx.json {
                writeln!(ctx.err, "wrote man pages to {}", dir.display())?;
            }
        }
    }
    Ok(())
}
