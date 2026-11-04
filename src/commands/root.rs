//! The root command: read the inputs, then sort, merge or check them.

use crate::cli::Cli;
use crate::ctx::Ctx;
use crate::error::AppError;
use crate::sort::key::{KeyDef, Mode, Order};
use crate::sort::{Record, Sorter};
use std::path::{Path, PathBuf};

pub fn run(ctx: &mut Ctx, cli: &Cli) -> Result<(), AppError> {
    let inputs = input_names(ctx, cli)?;
    let check = check_mode(cli);
    if check.is_some() && cli.merge {
        return Err(AppError::Usage("-c cannot be used with -m".into()));
    }
    if check.is_some() && inputs.len() > 1 {
        return Err(AppError::Usage(format!(
            "-c: can check only one file at a time (got {})",
            inputs.len()
        )));
    }
    let sorter = sorter(cli)?;
    let sep = if cli.zero_terminated { b'\0' } else { b'\n' };

    let mut per_input = Vec::with_capacity(inputs.len());
    for path in &inputs {
        let data = read_input(ctx, path)?;
        let records: Vec<Record> = split_lines(data, sep)
            .into_iter()
            .map(|line| sorter.record(line))
            .collect();
        per_input.push(records);
    }

    if let Some(diagnose) = check {
        let records = per_input.pop().unwrap_or_default();
        let name = inputs[0].display().to_string();
        return check_order(&sorter, &records, &name, diagnose);
    }

    let records = if cli.merge {
        sorter.merge(per_input)
    } else {
        let mut all: Vec<Record> = per_input.into_iter().flatten().collect();
        sorter.sort(&mut all);
        all
    };

    let mut out = Vec::with_capacity(records.iter().map(|r| r.line.len() + 1).sum());
    for r in &records {
        out.extend_from_slice(&r.line);
        out.push(sep);
    }
    match &cli.output {
        Some(path) => std::fs::write(path, out).map_err(|e| io_error(path, &e)),
        None => Ok(ctx.out.write_all(&out)?),
    }
}

/// `Some(true)` to check and report, `Some(false)` to check silently.
fn check_mode(cli: &Cli) -> Option<bool> {
    if cli.check_silent {
        return Some(false);
    }
    cli.check.as_deref().map(|mode| mode == "diagnose-first")
}

fn sorter(cli: &Cli) -> Result<Sorter, AppError> {
    let global = global_order(cli)?;
    let blanks = cli.ignore_leading_blanks;
    let keys = if cli.key.is_empty() {
        vec![KeyDef::whole_line(global, blanks)]
    } else {
        cli.key
            .iter()
            .map(|k| {
                KeyDef::parse(k, global, blanks)
                    .map_err(|e| AppError::Usage(format!("-k {k}: {e}")))
            })
            .collect::<Result<_, _>>()?
    };
    Ok(Sorter {
        keys,
        separator: separator(cli.field_separator.as_deref())?,
        seed: seed(cli.random_source.as_deref())?,
        reverse: cli.reverse,
        unique: cli.unique,
        stable: cli.stable || cli.unique,
    })
}

/// The ordering flags that apply to keys without option letters.
fn global_order(cli: &Cli) -> Result<Order, AppError> {
    let mut order = Order {
        reverse: cli.reverse,
        fold: cli.ignore_case,
        dictionary: cli.dictionary_order,
        nonprinting: cli.ignore_nonprinting,
        ..Order::default()
    };
    // clap rejects two different mode flags; --sort is checked here.
    for (on, mode) in [
        (cli.numeric_sort, Mode::Numeric),
        (cli.general_numeric_sort, Mode::General),
        (cli.human_numeric_sort, Mode::Human),
        (cli.month_sort, Mode::Month),
        (cli.random_sort, Mode::Random),
        (cli.version_sort, Mode::Version),
    ] {
        if on {
            order.set_mode(mode).map_err(AppError::Usage)?;
        }
    }
    if let Some(name) = cli.sort.as_deref() {
        let mode = match name {
            "general-numeric" => Mode::General,
            "human-numeric" => Mode::Human,
            "month" => Mode::Month,
            "random" => Mode::Random,
            _ => Mode::Numeric,
        };
        let current = order.mode;
        order.set_mode(mode).map_err(|_| {
            AppError::Usage(format!(
                "--sort={name} cannot be used with -{}",
                current.letter()
            ))
        })?;
    }
    Ok(order)
}

/// `-t CHAR`: one byte, or the escapes `\0` (NUL) and `\t` (tab).
fn separator(arg: Option<&str>) -> Result<Option<u8>, AppError> {
    let Some(s) = arg else {
        return Ok(None);
    };
    match s.as_bytes() {
        [b] => Ok(Some(*b)),
        b"\\0" => Ok(Some(b'\0')),
        b"\\t" => Ok(Some(b'\t')),
        _ => {
            let flag = if s.is_empty() {
                "-t".to_string()
            } else {
                format!("-t {s}")
            };
            Err(AppError::Usage(format!(
                "{flag}: the field separator must be one character"
            )))
        }
    }
}

/// The `-R` hash seed: from `--random-source` contents, or random.
fn seed(source: Option<&Path>) -> Result<u64, AppError> {
    use std::hash::{BuildHasher, Hasher};
    let Some(path) = source else {
        return Ok(std::collections::hash_map::RandomState::new()
            .build_hasher()
            .finish());
    };
    let data = std::fs::read(path).map_err(|e| io_error(path, &e))?;
    let mut h = std::hash::DefaultHasher::new();
    h.write(&data);
    Ok(h.finish())
}

/// The inputs to read: `--files0-from` names, the FILE operands, or stdin.
fn input_names(ctx: &mut Ctx, cli: &Cli) -> Result<Vec<PathBuf>, AppError> {
    let stdin = || vec![PathBuf::from("-")];
    let Some(list) = &cli.files0_from else {
        return Ok(if cli.files.is_empty() {
            stdin()
        } else {
            cli.files.clone()
        });
    };
    if !cli.files.is_empty() {
        return Err(AppError::Usage(
            "--files0-from cannot be used with FILE operands".into(),
        ));
    }
    let data = read_input(ctx, list)?;
    let names = split_lines(data, b'\0');
    if names.iter().any(Vec::is_empty) {
        return Err(AppError::Usage(format!(
            "--files0-from {}: empty file name",
            list.display()
        )));
    }
    if names.is_empty() {
        return Ok(stdin());
    }
    Ok(names.into_iter().map(path_from_bytes).collect())
}

#[cfg(unix)]
fn path_from_bytes(bytes: Vec<u8>) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    PathBuf::from(std::ffi::OsString::from_vec(bytes))
}

fn read_input(ctx: &mut Ctx, path: &Path) -> Result<Vec<u8>, AppError> {
    if path.as_os_str() == "-" {
        let mut data = Vec::new();
        ctx.input.read_to_end(&mut data)?;
        return Ok(data);
    }
    std::fs::read(path).map_err(|e| io_error(path, &e))
}

/// Split on `sep`; a missing final separator still ends a line.
fn split_lines(mut data: Vec<u8>, sep: u8) -> Vec<Vec<u8>> {
    if data.is_empty() {
        return Vec::new();
    }
    if data.last() == Some(&sep) {
        data.pop();
    }
    data.split(|&b| b == sep).map(<[u8]>::to_vec).collect()
}

fn check_order(
    sorter: &Sorter,
    records: &[Record],
    name: &str,
    diagnose: bool,
) -> Result<(), AppError> {
    match sorter.first_disorder(records) {
        None => Ok(()),
        Some(_) if !diagnose => Err(AppError::Disorder(String::new())),
        Some(i) => Err(AppError::Disorder(format!(
            "{name}:{}: disorder: {}",
            i + 1,
            String::from_utf8_lossy(&records[i].line)
        ))),
    }
}

/// `<path>: <reason>` without Rust's "(os error N)" suffix, like classic tools.
fn io_error(path: &Path, e: &std::io::Error) -> AppError {
    let reason = e.to_string();
    let reason = reason.split(" (os error").next().unwrap_or(&reason);
    AppError::Other(format!("{}: {reason}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_lines_handles_missing_and_present_final_separators() {
        assert!(split_lines(Vec::new(), b'\n').is_empty());
        assert_eq!(split_lines(b"a\nb".to_vec(), b'\n'), [b"a", b"b"]);
        assert_eq!(split_lines(b"a\nb\n".to_vec(), b'\n'), [b"a", b"b"]);
        assert_eq!(split_lines(b"\n".to_vec(), b'\n'), [b""]);
    }

    #[test]
    fn io_errors_drop_the_os_error_suffix() {
        let e = std::io::Error::from(std::io::ErrorKind::NotFound);
        let raw = std::io::Error::from_raw_os_error(2);
        assert!(
            io_error(Path::new("x"), &raw)
                .to_string()
                .starts_with("x: ")
        );
        assert!(
            !io_error(Path::new("x"), &raw)
                .to_string()
                .contains("os error")
        );
        assert!(io_error(Path::new("y"), &e).to_string().starts_with("y: "));
    }
}
