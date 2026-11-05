//! The obsolete `+POS1 [-POS2]` key syntax, rewritten into `-k` the way
//! sort's `fix_obsolete_keys` does before it parses options.
//!
//! `+F[.C][OPTS]` counts fields and characters from 0 and becomes
//! `-k(F+1).(C+1)OPTS`. A following `-F[.C][OPTS]` gives the end:
//! `,F.0` (the end of field F, counting from 1) or, with a character,
//! `,(F+1).C`. Like sort, this rewrites every argument up to `--`, whatever
//! its position; an argument that doesn't match is left alone (so `+x` is a
//! file name).

use std::ffi::OsString;

/// One parsed obsolete position: field, character and option letters.
struct ObsoletePos<'a> {
    field: u64,
    char: u64,
    opts: &'a str,
}

/// Parse `F[.C][OPTS]` (the text after `+` or `-`). `None` if it doesn't
/// match; `Some(Err)` if a number is too large.
fn parse(s: &str) -> Option<Result<ObsoletePos<'_>, String>> {
    let digits = |s: &str| s.bytes().take_while(u8::is_ascii_digit).count();
    let f = digits(s);
    if f == 0 {
        return None;
    }
    let mut rest = &s[f..];
    let mut c = None;
    if let Some(after) = rest.strip_prefix('.') {
        let n = digits(after);
        if n == 0 {
            return None;
        }
        c = Some(&after[..n]);
        rest = &after[n..];
    }
    if !rest.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    let number = |d: &str| {
        d.parse::<u64>()
            .map_err(|_| "Invalid key position".to_string())
    };
    Some((|| {
        Ok(ObsoletePos {
            field: number(&s[..f])?,
            char: c.map(number).transpose()?.unwrap_or(0),
            opts: rest,
        })
    })())
}

/// Rewrite obsolete key arguments in `args` (argv\[0\] included) into `-k`.
pub fn translate(args: Vec<OsString>) -> Result<Vec<OsString>, String> {
    let mut out = Vec::with_capacity(args.len());
    let mut iter = args.into_iter().enumerate().peekable();
    while let Some((i, arg)) = iter.next() {
        if i > 0 && arg == "--" {
            out.push(arg);
            out.extend(iter.map(|(_, a)| a));
            break;
        }
        let start = match arg.to_str().and_then(|s| s.strip_prefix('+')) {
            Some(rest) if i > 0 && !rest.is_empty() => parse(rest),
            _ => None,
        };
        let Some(start) = start else {
            out.push(arg);
            continue;
        };
        let start = start?;
        let first = format!("-k{}.{}{}", start.field + 1, start.char + 1, start.opts);
        let end = iter.peek().and_then(|(_, next)| {
            let rest = next.to_str()?.strip_prefix('-')?;
            if rest.is_empty() {
                return None;
            }
            parse(rest)
        });
        match end {
            Some(end) => {
                let end = end?;
                let field = if end.char > 0 {
                    end.field + 1
                } else {
                    end.field
                };
                out.push(format!("{first},{field}.{}{}", end.char, end.opts).into());
                iter.next();
            }
            None => out.push(first.into()),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tr(args: &[&str]) -> Result<Vec<String>, String> {
        let args = std::iter::once("sort")
            .chain(args.iter().copied())
            .map(OsString::from)
            .collect();
        translate(args).map(|v| {
            v.into_iter()
                .skip(1)
                .map(|a| a.into_string().unwrap())
                .collect()
        })
    }

    #[test]
    fn starts_count_from_zero() {
        assert_eq!(tr(&["+1"]).unwrap(), ["-k2.1"]);
        assert_eq!(tr(&["+0"]).unwrap(), ["-k1.1"]);
        assert_eq!(tr(&["+1.1r"]).unwrap(), ["-k2.2r"]);
        assert_eq!(tr(&["+0d", "in"]).unwrap(), ["-k1.1d", "in"]);
    }

    #[test]
    fn a_following_minus_position_is_the_end() {
        assert_eq!(tr(&["+1", "-2"]).unwrap(), ["-k2.1,2.0"]);
        assert_eq!(tr(&["+1", "-0.3"]).unwrap(), ["-k2.1,1.3"]);
        assert_eq!(tr(&["+1.4", "-1.5b"]).unwrap(), ["-k2.5,2.5b"]);
        assert_eq!(
            tr(&["+1", "-0.3", "+1.4", "-1.5", "in"]).unwrap(),
            ["-k2.1,1.3", "-k2.5,2.5", "in"]
        );
        // Not a position: left for the option parser.
        assert_eq!(tr(&["+1", "-r"]).unwrap(), ["-k2.1", "-r"]);
        assert_eq!(tr(&["+1", "-"]).unwrap(), ["-k2.1", "-"]);
    }

    #[test]
    fn non_positions_and_arguments_after_double_dash_are_untouched() {
        for args in [
            &["+", "+x", "+1x2", "+1.", "+.1", "+1.2.3"][..],
            &["--", "+0", "+1"],
            &["-2"],
        ] {
            assert_eq!(tr(args).unwrap(), args, "{args:?}");
        }
        assert_eq!(tr(&["in", "+0"]).unwrap(), ["in", "-k1.1"]);
    }

    #[test]
    fn huge_positions_are_errors() {
        let huge = "+99999999999999999999";
        assert_eq!(tr(&[huge]).unwrap_err(), "Invalid key position");
        assert!(tr(&["+1", "-99999999999999999999"]).is_err());
        assert!(tr(&["+1.99999999999999999999"]).is_err());
    }

    #[test]
    fn non_utf8_arguments_pass_through() {
        use std::os::unix::ffi::OsStringExt;
        let odd = OsString::from_vec(vec![b'+', 0xff]);
        let args = vec![OsString::from("sort"), odd.clone()];
        assert_eq!(translate(args).unwrap()[1], odd);
    }
}
