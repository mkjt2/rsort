//! One module per command. Each exposes `run(ctx, ...) -> Result<(), AppError>`.

pub mod completions;
pub mod generate;
pub mod man;
pub mod root;
