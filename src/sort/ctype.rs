//! The active locale's character classes and month names (LC_CTYPE and
//! LC_TIME), as sort uses them: `-d` keeps alphanumerics and blanks, `-i`
//! keeps printable characters, `-f` and `-M` uppercase, and `-M` matches the
//! locale's abbreviated month names. Single-byte locales use libc's byte
//! classes (`isalnum`, `isprint`, `toupper`); UTF-8 uses the wide ones
//! (`iswalnum`, `iswprint`, `towupper`).
//!
//! The tables are read once, after [`crate::sort::collate::Locale`] has set
//! the locale; before that (in unit tests) they are the C locale's.

use std::ffi::CStr;
use std::sync::OnceLock;

// Wide-character classes; libc doesn't bind them. wint_t is 32 bits on
// macOS and Linux.
unsafe extern "C" {
    fn iswalnum(wc: u32) -> libc::c_int;
    fn iswprint(wc: u32) -> libc::c_int;
    fn towupper(wc: u32) -> u32;
}

struct Tables {
    alnum: [bool; 256],
    print: [bool; 256],
    upper: [u8; 256],
    /// Abbreviated month names, uppercased; empty when the locale has none.
    months: [Vec<u8>; 12],
}

static TABLES: OnceLock<Tables> = OnceLock::new();

/// Read the current locale's tables (the first call wins).
pub fn init(utf8: bool) {
    TABLES.get_or_init(|| read(utf8));
}

fn tables() -> &'static Tables {
    TABLES.get_or_init(|| read(false))
}

fn read(utf8: bool) -> Tables {
    let mut t = Tables {
        alnum: [false; 256],
        print: [false; 256],
        upper: [0; 256],
        months: Default::default(),
    };
    for b in 0..=255u8 {
        let c = libc::c_int::from(b);
        // SAFETY: the ctype functions accept any unsigned char value.
        unsafe {
            t.alnum[usize::from(b)] = libc::isalnum(c) != 0;
            t.print[usize::from(b)] = libc::isprint(c) != 0;
            t.upper[usize::from(b)] = u8::try_from(libc::toupper(c)).unwrap_or(b);
        }
    }
    for (i, month) in t.months.iter_mut().enumerate() {
        // SAFETY: nl_langinfo returns a NUL-terminated string that stays
        // valid until the next setlocale; it is copied at once.
        let name = unsafe { CStr::from_ptr(libc::nl_langinfo(libc::ABMON_1 + i as libc::nl_item)) };
        let name = name.to_bytes();
        *month = if utf8 {
            match std::str::from_utf8(name) {
                Ok(s) => {
                    let mut out = Vec::new();
                    for c in s.chars() {
                        let mut buf = [0u8; 4];
                        out.extend_from_slice(upper(c).encode_utf8(&mut buf).as_bytes());
                    }
                    out
                }
                Err(_) => Vec::new(),
            }
        } else {
            // sort uppercases these with toupper on a signed char, which
            // leaves bytes from 0x80 unchanged. Keys are uppercased fully, so
            // a name with lowercase non-ASCII letters (Russian ones in KOI8-R)
            // never matches; rsort keeps that.
            name.iter()
                .map(|&b| if b < 0x80 { t.upper[usize::from(b)] } else { b })
                .collect()
        };
    }
    t
}

/// `isalnum` in the locale.
pub fn is_alnum(b: u8) -> bool {
    tables().alnum[usize::from(b)]
}

/// `isprint` in the locale.
pub fn is_print(b: u8) -> bool {
    tables().print[usize::from(b)]
}

/// `toupper` in the locale.
pub fn to_upper(b: u8) -> u8 {
    tables().upper[usize::from(b)]
}

/// `iswalnum` in the locale.
pub fn is_walnum(c: char) -> bool {
    // SAFETY: iswalnum accepts any wide character value.
    unsafe { iswalnum(u32::from(c)) != 0 }
}

/// `iswprint` in the locale.
pub fn is_wprint(c: char) -> bool {
    // SAFETY: iswprint accepts any wide character value.
    unsafe { iswprint(u32::from(c)) != 0 }
}

/// `towupper` in the locale.
pub fn upper(c: char) -> char {
    // SAFETY: towupper accepts any wide character value.
    char::from_u32(unsafe { towupper(u32::from(c)) }).unwrap_or(c)
}

/// The locale's uppercased month abbreviations, January first.
pub fn months() -> &'static [Vec<u8>; 12] {
    &tables().months
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c_locale_classes_are_ascii() {
        assert!(is_alnum(b'a') && is_alnum(b'Z') && is_alnum(b'7'));
        assert!(!is_alnum(b'-') && !is_alnum(0xe9));
        assert!(is_print(b' ') && is_print(b'~') && !is_print(b'\t') && !is_print(0x7f));
        assert_eq!(to_upper(b'q'), b'Q');
        assert_eq!(to_upper(0xe9), 0xe9);
        assert_eq!(months()[0], b"JAN");
        assert_eq!(months()[11], b"DEC");
        assert!(is_walnum('a') && !is_walnum('-'));
        assert!(is_wprint('a') && !is_wprint('\u{7}'));
        assert_eq!(upper('a'), 'A');
    }
}
