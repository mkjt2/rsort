//! The root command: read the inputs, then sort, merge or check them.

use crate::cli::Cli;
use crate::ctx::Ctx;
use crate::error::AppError;
use crate::sort::collate::Locale;
use crate::sort::key::{KeyDef, Mode, Order};
use crate::sort::{Record, Sorter};
use std::cmp::Ordering;
use std::io::{BufRead, BufReader, BufWriter, Write};
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
        utf8: locale.utf8,
        ..sorter(cli)?
    };
    // sort takes -t with btowc, which has no character for a lone byte from
    // 0x80 in UTF-8.
    if locale.utf8 && sorter.separator.is_some_and(|b| b >= 0x80) {
        return Err(AppError::Usage(
            "-t: the field separator is not a character in this locale".into(),
        ));
    }
    // As in sort, a -t equal to the decimal point, thousands separator or
    // minus sign is only a field separator for -n and -h, unless
    // GNUSORT_NUMERIC_COMPATIBILITY is set.
    if let Some(sep) = sorter.separator
        && !locale.gnu_numbers
    {
        sorter.numbers = sorter.numbers.separated_by(sep);
    }
    let sep = if cli.zero_terminated { b'\0' } else { b'\n' };

    if let Some(diagnose) = check {
        return check_input(ctx, &sorter, &inputs[0], sep, diagnose);
    }
    if cli.merge && !output_is_an_input(cli.output.as_deref(), &inputs) {
        return merge_inputs(ctx, &sorter, &inputs, sep, cli.output.as_deref());
    }

    // Sort in memory (or merge, when -o overwrites an input).
    let data = inputs
        .iter()
        .map(|path| read_input(ctx, path))
        .collect::<Result<Vec<_>, _>>()?;
    if cli.merge {
        let readers = data.iter().map(|d| Box::new(&d[..]) as Box<dyn BufRead>);
        let mut sources = Vec::new();
        for (path, reader) in inputs.iter().zip(readers) {
            sources.push((path.as_path(), reader));
        }
        return merge(&mut *ctx.out, &sorter, sources, sep, cli.output.as_deref());
    }
    let total = data.iter().map(|d| count_lines(d, sep)).sum();
    let mut keys = sorter.keys(total);
    let mut records: Vec<Record> = Vec::with_capacity(total);
    for d in &data {
        for line in lines(d, sep) {
            let record = sorter.record(&mut keys, line).ok_or_else(|| {
                AppError::Other("too many lines to sort with several keys".into())
            })?;
            records.push(record);
        }
    }
    sorter.sort(&keys, &mut records);
    write_output(&mut *ctx.out, cli.output.as_deref(), |w| {
        for r in &records {
            w.write_all(r.line)?;
            w.write_all(&[sep])?;
        }
        Ok(())
    })
}

/// Write to `-o PATH` or stdout through a buffer.
fn write_output(
    stdout: &mut dyn Write,
    path: Option<&Path>,
    body: impl FnOnce(&mut dyn Write) -> std::io::Result<()>,
) -> Result<(), AppError> {
    match path {
        Some(path) => {
            let file = std::fs::File::create(path).map_err(|e| io_error(path, &e))?;
            let mut w = BufWriter::with_capacity(OUT_BUFFER, file);
            body(&mut w)
                .and_then(|()| w.flush())
                .map_err(|e| io_error(path, &e))
        }
        None => {
            let mut w = BufWriter::with_capacity(OUT_BUFFER, stdout);
            body(&mut w)?;
            w.flush()?;
            Ok(())
        }
    }
}

const OUT_BUFFER: usize = 64 * 1024;

/// Whether `-o` names an existing file that is also an input, which a
/// streaming merge would overwrite before reading.
fn output_is_an_input(output: Option<&Path>, inputs: &[PathBuf]) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Some(out) = output.and_then(|p| std::fs::metadata(p).ok()) else {
        return false;
    };
    inputs
        .iter()
        .filter(|p| p.as_os_str() != "-")
        .filter_map(|p| std::fs::metadata(p).ok())
        .any(|m| m.dev() == out.dev() && m.ino() == out.ino())
}

/// An input's name and a reader over its lines.
type Source<'a> = (&'a Path, Box<dyn BufRead + 'a>);

/// Open every input for streaming: files, and stdin for the first `-` (a
/// later `-` reads nothing, as stdin is then used up).
fn open_inputs<'a>(
    input: &'a mut dyn BufRead,
    paths: &'a [PathBuf],
) -> Result<Vec<Source<'a>>, AppError> {
    let mut stdin = Some(input);
    let mut out = Vec::with_capacity(paths.len());
    for path in paths {
        let reader: Box<dyn BufRead + 'a> = if path.as_os_str() == "-" {
            match stdin.take() {
                Some(r) => Box::new(r),
                None => Box::new(&[][..]),
            }
        } else {
            let file = std::fs::File::open(path).map_err(|e| io_error(path, &e))?;
            Box::new(BufReader::with_capacity(IN_BUFFER, file))
        };
        out.push((path.as_path(), reader));
    }
    Ok(out)
}

const IN_BUFFER: usize = 64 * 1024;

/// Read the next line (without its separator) into `buf`; false at the end.
fn next_line(
    reader: &mut dyn BufRead,
    sep: u8,
    buf: &mut Vec<u8>,
    path: &Path,
) -> Result<bool, AppError> {
    buf.clear();
    let n = reader
        .read_until(sep, buf)
        .map_err(|e| io_error(path, &e))?;
    if buf.last() == Some(&sep) {
        buf.pop();
    }
    Ok(n > 0)
}

/// `-c`/`-C`: read the input line by line, keeping only the previous line.
fn check_input(
    ctx: &mut Ctx,
    sorter: &Sorter,
    path: &Path,
    sep: u8,
    diagnose: bool,
) -> Result<(), AppError> {
    let paths = [path.to_path_buf()];
    let mut inputs = open_inputs(&mut *ctx.input, &paths)?;
    let (path, reader) = &mut inputs[0];
    let (mut prev, mut line) = (Vec::new(), Vec::new());
    if !next_line(reader.as_mut(), sep, &mut prev, path)? {
        return Ok(());
    }
    let mut number = 1;
    while next_line(reader.as_mut(), sep, &mut line, path)? {
        number += 1;
        if sorter.disorder(&prev, &line) {
            if !diagnose {
                return Err(AppError::Disorder(String::new()));
            }
            return Err(AppError::Disorder(format!(
                "{}:{number}: disorder: {}",
                path.display(),
                String::from_utf8_lossy(&line)
            )));
        }
        std::mem::swap(&mut prev, &mut line);
    }
    Ok(())
}

/// `-m`: stream a merge of the inputs.
fn merge_inputs(
    ctx: &mut Ctx,
    sorter: &Sorter,
    paths: &[PathBuf],
    sep: u8,
    output: Option<&Path>,
) -> Result<(), AppError> {
    let sources = open_inputs(&mut *ctx.input, paths)?;
    merge(&mut *ctx.out, sorter, sources, sep, output)
}

/// Merge sorted sources line by line; on ties the earlier source wins.
fn merge(
    stdout: &mut dyn Write,
    sorter: &Sorter,
    mut sources: Vec<Source<'_>>,
    sep: u8,
    output: Option<&Path>,
) -> Result<(), AppError> {
    // Each source's current line, or None once it is used up.
    let mut heads: Vec<Option<Vec<u8>>> = Vec::with_capacity(sources.len());
    for (path, reader) in &mut sources {
        let mut line = Vec::new();
        heads.push(next_line(reader.as_mut(), sep, &mut line, path)?.then_some(line));
    }
    let mut read_error = None;
    let mut last: Option<Vec<u8>> = None;
    let written = write_output(stdout, output, |w| {
        loop {
            let mut best: Option<usize> = None;
            for (i, head) in heads.iter().enumerate() {
                let Some(line) = head else { continue };
                if best.is_none_or(|b| {
                    let current = heads[b].as_deref().unwrap_or_default();
                    sorter.compare_lines(line, current) == Ordering::Less
                }) {
                    best = Some(i);
                }
            }
            let Some(i) = best else { return Ok(()) };
            let Some(line) = heads[i].take() else {
                return Ok(());
            };
            let duplicate = sorter.unique
                && last
                    .as_deref()
                    .is_some_and(|prev| sorter.compare_line_keys(prev, &line) == Ordering::Equal);
            if !duplicate {
                w.write_all(&line)?;
                w.write_all(&[sep])?;
            }
            // Reuse the previous line's buffer for the next read.
            let mut next = last.replace(line).unwrap_or_default();
            let (path, reader) = &mut sources[i];
            match next_line(reader.as_mut(), sep, &mut next, path) {
                Ok(true) => heads[i] = Some(next),
                Ok(false) => {}
                Err(e) => {
                    read_error = Some(e);
                    return Ok(());
                }
            }
        }
    });
    match read_error {
        Some(e) => Err(e),
        None => written,
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
        stable: cli.stable,
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

/// `-t CHAR` as sort reads it: while more than one byte is left it must
/// start with a backslash, which is dropped, and a `0` right after a dropped
/// backslash is NUL; then the first byte (NUL when empty). So `\t` is `t`
/// and `\\` is a backslash.
fn separator(arg: Option<&std::ffi::OsStr>) -> Result<Option<u8>, AppError> {
    use std::os::unix::ffi::OsStrExt;
    let Some(arg) = arg else {
        return Ok(None);
    };
    let mut s = arg.as_bytes();
    while s.len() > 1 {
        if s[0] != b'\\' {
            return Err(AppError::Usage(format!(
                "-t {}: the field separator must be one character",
                arg.to_string_lossy()
            )));
        }
        s = &s[1..];
        if s[0] == b'0' {
            return Ok(Some(0));
        }
    }
    Ok(Some(s.first().copied().unwrap_or(0)))
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

/// How many lines [`lines`] yields.
fn count_lines(data: &[u8], sep: u8) -> usize {
    let separators = data.iter().filter(|&&b| b == sep).count();
    separators + usize::from(data.last().is_some_and(|&b| b != sep))
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
        for d in [&b""[..], b"a", b"a\n", b"a\nb", b"\n\n"] {
            assert_eq!(count_lines(d, b'\n'), split(d).len(), "{d:?}");
        }
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
