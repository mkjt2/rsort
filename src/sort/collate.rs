//! Locale support: text collation (byte order, or strcoll when LC_ALL /
//! LC_COLLATE / LANG name a non-C locale) and the locale's number format,
//! as sort uses them.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::ffi::CString;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Collator {
    /// Compare bytes (the C and POSIX locales, or no usable locale).
    #[default]
    Bytes,
    /// strcoll in the process's LC_COLLATE locale; `utf8` when its
    /// encoding is UTF-8 (character classes and case folding go by `char`).
    Locale { utf8: bool },
}

/// The collation locale named by the environment: the first non-empty of
/// LC_ALL, LC_COLLATE and LANG.
pub fn locale_name(env: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    ["LC_ALL", "LC_COLLATE", "LANG"]
        .into_iter()
        .filter_map(env)
        .find(|v| !v.is_empty())
}

/// The decimal point and thousands separator used by -n, -h and -g.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumberFormat {
    pub decimal: u8,
    pub thousands: Option<u8>,
}

impl Default for NumberFormat {
    fn default() -> Self {
        NumberFormat {
            decimal: b'.',
            thousands: None,
        }
    }
}

/// What the active locale means for sorting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Locale {
    pub collator: Collator,
    pub numbers: NumberFormat,
}

impl Locale {
    /// Activate `name` for collation, character classes and numbers.
    /// Unknown locales, C and POSIX give byte order and C number rules.
    pub fn activate(name: Option<&str>) -> Locale {
        let Some(name) = name.filter(|n| !matches!(*n, "C" | "POSIX")) else {
            return Locale::default();
        };
        let Ok(c_name) = CString::new(name) else {
            return Locale::default();
        };
        // SAFETY: setlocale is called with valid NUL-terminated strings
        // before any other thread exists or compares text.
        let ok = [libc::LC_COLLATE, libc::LC_CTYPE, libc::LC_NUMERIC]
            .into_iter()
            .all(|category| unsafe { !libc::setlocale(category, c_name.as_ptr()).is_null() });
        if !ok {
            return Locale::default();
        }
        let lower = name.to_ascii_lowercase();
        Locale {
            collator: Collator::Locale {
                utf8: lower.contains("utf-8") || lower.contains("utf8"),
            },
            numbers: number_format(),
        }
    }
}

/// The current LC_NUMERIC separators; multi-byte ones are not supported.
fn number_format() -> NumberFormat {
    // SAFETY: localeconv returns a pointer to static data that stays valid
    // until the next setlocale/localeconv call; we copy out of it at once.
    let single_byte = |ptr: *const libc::c_char| {
        if ptr.is_null() {
            return None;
        }
        // SAFETY: localeconv strings are NUL-terminated.
        match unsafe { std::ffi::CStr::from_ptr(ptr) }.to_bytes() {
            [b] => Some(*b),
            _ => None,
        }
    };
    let conv = unsafe { libc::localeconv() };
    if conv.is_null() {
        return NumberFormat::default();
    }
    // SAFETY: `conv` is non-null and points to a valid lconv.
    let (decimal, thousands) = unsafe { ((*conv).decimal_point, (*conv).thousands_sep) };
    NumberFormat {
        decimal: single_byte(decimal).unwrap_or(b'.'),
        thousands: single_byte(thousands),
    }
}

impl Collator {
    pub fn is_utf8(self) -> bool {
        matches!(self, Collator::Locale { utf8: true })
    }

    pub fn compare(self, a: &[u8], b: &[u8]) -> Ordering {
        match self {
            Collator::Bytes => a.cmp(b),
            Collator::Locale { utf8 } => {
                if utf8 && (std::str::from_utf8(a).is_err() || std::str::from_utf8(b).is_err()) {
                    return a.cmp(b);
                }
                // strcoll stops at NUL: collate up to the first one, then bytes.
                let cut = |s: &[u8]| s.iter().position(|&c| c == 0).unwrap_or(s.len());
                strcoll(&a[..cut(a)], &b[..cut(b)]).then_with(|| a.cmp(b))
            }
        }
    }
}

thread_local! {
    static BUFFERS: RefCell<(Vec<u8>, Vec<u8>)> = const { RefCell::new((Vec::new(), Vec::new())) };
}

/// Append the strxfrm of `out[start..end]` (up to its first NUL) to `out`
/// and return the appended range. Comparing these bytes orders keys as
/// strcoll does, so each key is collated once instead of per comparison.
/// `None` when strxfrm failed: macOS then copies the input unchanged (for
/// example for a precomposed `Å`) although strcoll still collates it.
pub fn xfrm_from(out: &mut Vec<u8>, start: usize, end: usize) -> Option<(usize, usize)> {
    BUFFERS.with_borrow_mut(|(src, _)| {
        load_c_string(src, &out[start..end]);
        xfrm_into(src, out)
    })
}

/// Like [`xfrm_from`] for a key that is not in `out`.
pub fn xfrm(key: &[u8], out: &mut Vec<u8>) -> Option<(usize, usize)> {
    BUFFERS.with_borrow_mut(|(src, _)| {
        load_c_string(src, key);
        xfrm_into(src, out)
    })
}

/// `buf` = `key` up to its first NUL, NUL-terminated.
fn load_c_string(buf: &mut Vec<u8>, key: &[u8]) {
    let cut = key.iter().position(|&c| c == 0).unwrap_or(key.len());
    buf.clear();
    buf.extend_from_slice(&key[..cut]);
    buf.push(0);
}

fn xfrm_into(src: &[u8], out: &mut Vec<u8>) -> Option<(usize, usize)> {
    let start = out.len();
    // macOS keys run about 6 bytes per input byte; retry once if short.
    let mut room = src.len() * 8 + 8;
    loop {
        out.reserve(room);
        // SAFETY: `src` is NUL-terminated with no other NUL; the destination
        // has `room` writable bytes; strxfrm writes at most `room` bytes and
        // returns the full length, which we only keep when it fits.
        let n = unsafe {
            libc::strxfrm(
                out.as_mut_ptr().add(start).cast(),
                src.as_ptr().cast(),
                room,
            )
        };
        if n < room {
            // SAFETY: strxfrm initialized `n` bytes after `start`.
            unsafe { out.set_len(start + n) };
            // A real key never equals its input (it encodes weights), so an
            // unchanged copy is the failure fallback.
            if out[start..] == src[..src.len() - 1] {
                out.truncate(start);
                return None;
            }
            return Some((start, start + n));
        }
        room = n + 1;
    }
}

/// strcoll on NUL-free byte strings, reusing buffers for the terminators.
fn strcoll(a: &[u8], b: &[u8]) -> Ordering {
    BUFFERS.with_borrow_mut(|(x, y)| {
        for (buf, s) in [(&mut *x, a), (&mut *y, b)] {
            buf.clear();
            buf.extend_from_slice(s);
            buf.push(0);
        }
        // SAFETY: both buffers are NUL-terminated and contain no other NUL.
        let r = unsafe { libc::strcoll(x.as_ptr().cast(), y.as_ptr().cast()) };
        r.cmp(&0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key| pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    #[test]
    fn locale_precedence() {
        assert_eq!(locale_name(&env(&[])), None);
        assert_eq!(locale_name(&env(&[("LANG", "x")])).as_deref(), Some("x"));
        assert_eq!(
            locale_name(&env(&[("LANG", "x"), ("LC_COLLATE", "y")])).as_deref(),
            Some("y")
        );
        assert_eq!(
            locale_name(&env(&[("LC_ALL", ""), ("LC_COLLATE", "y"), ("LANG", "x")])).as_deref(),
            Some("y")
        );
        assert_eq!(
            locale_name(&env(&[("LC_ALL", "z"), ("LC_COLLATE", "y")])).as_deref(),
            Some("z")
        );
    }

    #[test]
    fn c_posix_and_unknown_locales_are_bytes() {
        for name in [
            None,
            Some("C"),
            Some("POSIX"),
            Some("no_SUCH.locale"),
            Some("bad\0name"),
        ] {
            assert_eq!(Locale::activate(name), Locale::default(), "{name:?}");
        }
        assert_eq!(NumberFormat::default().decimal, b'.');
        assert!(!Collator::Bytes.is_utf8());
        assert_eq!(Collator::Bytes.compare(b"a", b"B"), Ordering::Greater);
    }

    #[test]
    fn locale_compare_breaks_ties_by_bytes_and_skips_invalid_utf8() {
        // Byte fallbacks hold whatever locale the process has.
        let c = Collator::Locale { utf8: true };
        assert_eq!(c.compare(b"\xff", b"a"), Ordering::Greater);
        assert_eq!(c.compare(b"a\0b", b"a\0c"), Ordering::Less);
        assert_eq!(c.compare(b"a", b"a"), Ordering::Equal);
    }

    #[test]
    fn xfrm_treats_an_unchanged_copy_as_failure() {
        // The test process runs in the C locale, where strxfrm copies.
        let mut out = b"ab\0c".to_vec();
        assert_eq!(xfrm_from(&mut out, 0, 4), None);
        assert_eq!(xfrm(&[b'x'; 100], &mut out), None);
        assert_eq!(out, b"ab\0c", "a failed key leaves nothing behind");
    }
}
