//! The root command: read the inputs, then sort, merge or check them.

use crate::cli::Cli;
use crate::ctx::Ctx;
use crate::error::AppError;
use crate::sort::collate::Locale;
use crate::sort::key::{KeyDef, Mode, Order};
use crate::sort::{Record, Sorter};
use std::path::{Path, PathBuf};

pub fn run(ctx: &mut Ctx, cli: &Cli, locale: Locale) -> Result<(), AppError> {
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
    let mut sorter = Sorter {
        collator: locale.collator,
        numbers: locale.numbers,
        ..sorter(cli)?
    };
    // As in sort, a -t that is also the thousands separator is only a field
    // separator.
    if sorter.separator.is_some() && sorter.separator == sorter.numbers.thousands {
        sorter.numbers.thousands = None;
    }
    // Likewise -n and -h stop reading fractions at a -t decimal point.
    if sorter.separator == Some(sorter.numbers.decimal) {
        sorter.numbers.integers_only = true;
    }
    let sep = if cli.zero_terminated { b'\0' } else { b'\n' };

    let data = inputs
        .iter()
        .map(|path| read_input(ctx, path))
        .collect::<Result<Vec<_>, _>>()?;
    let mut keys = sorter.keys();
    let mut per_input: Vec<Vec<Record>> = data
        .iter()
        .map(|d| {
            lines(d, sep)
                .map(|line| sorter.record(&mut keys, line))
                .collect()
        })
        .collect();

    if let Some(diagnose) = check {
        let records = per_input.pop().unwrap_or_default();
        let name = inputs[0].display().to_string();
        return check_order(&sorter, &keys, &records, &name, diagnose);
    }

    let records = if cli.merge {
        sorter.merge(&keys, per_input)
    } else {
        let mut all: Vec<Record> = per_input.into_iter().flatten().collect();
        sorter.sort(&keys, &mut all);
        all
    };

    let mut out = Vec::with_capacity(records.iter().map(|r| r.line.len() + 1).sum());
    for r in &records {
        out.extend_from_slice(r.line);
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
        ..Sorter::default()
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
    let names: Vec<&[u8]> = lines(&data, b'\0').collect();
    if names.iter().any(|n| n.is_empty()) {
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
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}

fn read_input(ctx: &mut Ctx, path: &Path) -> Result<Vec<u8>, AppError> {
    if path.as_os_str() == "-" {
        let mut data = Vec::new();
        ctx.input.read_to_end(&mut data)?;
        return Ok(data);
    }
    std::fs::read(path).map_err(|e| io_error(path, &e))
}

/// The lines of `data` split on `sep`; a missing final separator still
/// ends a line.
fn lines(data: &[u8], sep: u8) -> impl Iterator<Item = &[u8]> {
    let body = match data.split_last() {
        None => None,
        Some((&last, rest)) if last == sep => Some(rest),
        Some(_) => Some(data),
    };
    body.into_iter()
        .flat_map(move |b| b.split(move |&c| c == sep))
}

fn check_order(
    sorter: &Sorter,
    keys: &crate::sort::Keys,
    records: &[Record],
    name: &str,
    diagnose: bool,
) -> Result<(), AppError> {
    match sorter.first_disorder(keys, records) {
        None => Ok(()),
        Some(_) if !diagnose => Err(AppError::Disorder(String::new())),
        Some(i) => Err(AppError::Disorder(format!(
            "{name}:{}: disorder: {}",
            i + 1,
            String::from_utf8_lossy(records[i].line)
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
    fn lines_handle_missing_and_present_final_separators() {
        let split = |d: &'static [u8]| lines(d, b'\n').collect::<Vec<_>>();
        assert!(split(b"").is_empty());
        assert_eq!(split(b"a\nb"), [b"a", b"b"]);
        assert_eq!(split(b"a\nb\n"), [b"a", b"b"]);
        assert_eq!(split(b"\n"), [b""]);
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
