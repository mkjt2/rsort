//! Locale support, as sort sets it up with `setlocale(LC_ALL, "")`: text
//! collation (byte order in the C and POSIX locales, strcoll otherwise), the
//! character set (UTF-8 or single bytes), and the number format.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::ffi::{CStr, CString};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Collator {
    /// Compare bytes (the C and POSIX locales, or no usable locale).
    #[default]
    Bytes,
    /// strcoll in the process's LC_COLLATE locale; `utf8` when the
    /// LC_CTYPE encoding is UTF-8 (lengths then count characters).
    Locale { utf8: bool },
}

unsafe extern "C" {
    fn mbtowc(pwc: *mut libc::wchar_t, s: *const libc::c_char, n: libc::size_t) -> libc::c_int;
}

/// Whether LC_CTYPE decodes UTF-8: mbtowc reads "€" as one character,
/// U+20AC. Asked of the C library because macOS names don't tell: bare
/// "en_US" or "ja_JP" are UTF-8 with an empty CODESET, and
/// "sr_RS.UTF-8@latin" reports "UTF-8@latin".
fn decodes_utf8() -> bool {
    let euro = c"\u{20ac}";
    let mut wc: libc::wchar_t = 0;
    // SAFETY: a null `s` resets mbtowc's shift state; then `euro` is a
    // valid NUL-terminated string of which at most 4 bytes are read.
    unsafe {
        mbtowc(std::ptr::null_mut(), std::ptr::null(), 0);
        mbtowc(&mut wc, euro.as_ptr(), 4) == 3 && wc as u32 == 0x20ac
    }
}

/// The categories `setlocale(LC_ALL, "")` sets, with their variables.
const CATEGORIES: [(libc::c_int, &str); 6] = [
    (libc::LC_COLLATE, "LC_COLLATE"),
    (libc::LC_CTYPE, "LC_CTYPE"),
    (libc::LC_MONETARY, "LC_MONETARY"),
    (libc::LC_NUMERIC, "LC_NUMERIC"),
    (libc::LC_TIME, "LC_TIME"),
    (libc::LC_MESSAGES, "LC_MESSAGES"),
];

/// The locale the environment names for a category: the first non-empty
/// of LC_ALL, the category's own variable and LANG, else C.
pub fn category_name(env: &dyn Fn(&str) -> Option<String>, var: &str) -> String {
    ["LC_ALL", var, "LANG"]
        .into_iter()
        .filter_map(env)
        .find(|v| !v.is_empty())
        .unwrap_or_else(|| "C".into())
}

/// One character of the number format, as sort holds it in a wint_t: a
/// byte, or one UTF-8 character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Symbol {
    bytes: [u8; 4],
    len: u8,
}

impl Symbol {
    pub const fn byte(b: u8) -> Symbol {
        Symbol {
            bytes: [b, 0, 0, 0],
            len: 1,
        }
    }

    /// The first character of `s` (UTF-8 when `utf8`, else the first byte),
    /// as mbtowc reads it; `None` when there is none.
    pub fn first(s: &[u8], utf8: bool) -> Option<Symbol> {
        let n = if utf8 {
            let head = &s[..s.len().min(4)];
            let valid = match std::str::from_utf8(head) {
                Ok(t) => t,
                Err(e) => std::str::from_utf8(&head[..e.valid_up_to()]).ok()?,
            };
            valid.chars().next()?.len_utf8()
        } else {
            usize::from(!s.is_empty())
        };
        if n == 0 || s[0] == 0 {
            return None;
        }
        let mut bytes = [0; 4];
        bytes[..n].copy_from_slice(&s[..n]);
        Some(Symbol {
            bytes,
            len: n as u8,
        })
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }

    /// Its length in bytes.
    pub fn width(&self) -> usize {
        usize::from(self.len)
    }

    /// Whether `s` has this symbol at `i`.
    pub fn is_at(&self, s: &[u8], i: usize) -> bool {
        s.get(i..)
            .is_some_and(|rest| rest.starts_with(self.as_bytes()))
    }
}

/// The locale's number symbols, as sort reads them from localeconv.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumberFormat {
    /// -n and -h: the decimal point, thousands separator and minus sign.
    /// `None` disables one, as a `-t` equal to it does.
    pub point: Option<Symbol>,
    pub thousands: Option<Symbol>,
    pub minus: Option<Symbol>,
    /// -g (strtod): the whole decimal point string, whatever `-t` is.
    pub decimal: Symbol,
}

impl Default for NumberFormat {
    fn default() -> Self {
        NumberFormat {
            point: Some(Symbol::byte(b'.')),
            thousands: None,
            minus: Some(Symbol::byte(b'-')),
            decimal: Symbol::byte(b'.'),
        }
    }
}

impl NumberFormat {
    /// What `-t SEP` does to the symbols in sort (unless
    /// GNUSORT_NUMERIC_COMPATIBILITY is set): one equal to it is disabled.
    pub fn separated_by(mut self, sep: u8) -> NumberFormat {
        for sym in [&mut self.point, &mut self.thousands, &mut self.minus] {
            if *sym == Some(Symbol::byte(sep)) {
                *sym = None;
            }
        }
        self
    }
}

/// What the active locale means for sorting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Locale {
    pub collator: Collator,
    pub numbers: NumberFormat,
    /// LC_CTYPE is UTF-8: `-k` offsets and `-d`/`-f`/`-i` go by character.
    pub utf8: bool,
    /// GNUSORT_NUMERIC_COMPATIBILITY is set: `-t` leaves the number symbols
    /// alone.
    pub gnu_numbers: bool,
}

impl Locale {
    /// Set every category from the environment as `setlocale(LC_ALL, "")`
    /// does: if any of them names a locale that doesn't exist, all stay C.
    pub fn from_env(env: &dyn Fn(&str) -> Option<String>) -> Locale {
        let gnu_numbers = env("GNUSORT_NUMERIC_COMPATIBILITY").is_some();
        let gnu_blanks = env("GNUSORT_COMPATIBLE_BLANKS").is_some();
        // SAFETY: setlocale is called with valid NUL-terminated strings
        // before any other thread exists or compares text.
        let set = |category, name: &str| {
            CString::new(name)
                .is_ok_and(|n| unsafe { !libc::setlocale(category, n.as_ptr()).is_null() })
        };
        let ok = CATEGORIES
            .iter()
            .all(|&(category, var)| set(category, &category_name(env, var)));
        if !ok {
            for (category, _) in CATEGORIES {
                set(category, "C");
            }
            super::ctype::init(false, gnu_blanks);
            return Locale {
                gnu_numbers,
                ..Locale::default()
            };
        }
        // SAFETY: nl_langinfo and setlocale(…, NULL) return NUL-terminated
        // strings that stay valid until the next setlocale; both are copied.
        let utf8 = decodes_utf8();
        let collate = unsafe { libc::setlocale(libc::LC_COLLATE, std::ptr::null()) };
        let collate = if collate.is_null() {
            "C".into()
        } else {
            unsafe { CStr::from_ptr(collate) }.to_string_lossy()
        };
        let collator = if matches!(collate.as_ref(), "C" | "POSIX") {
            Collator::Bytes
        } else {
            Collator::Locale { utf8 }
        };
        super::ctype::init(utf8, gnu_blanks);
        Locale {
            collator,
            numbers: number_format(utf8),
            utf8,
            gnu_numbers,
        }
    }
}

/// The current LC_NUMERIC symbols, as sort reads them with mbtowc: the
/// first character of each (in a single-byte locale its first byte, so
/// Big5HKSCS's two-byte decimal point counts by its first byte), or the
/// default ('.', none, '-') when empty.
fn number_format(utf8: bool) -> NumberFormat {
    // SAFETY: localeconv returns a pointer to static data that stays valid
    // until the next setlocale/localeconv call; we copy out of it at once.
    let conv = unsafe { libc::localeconv() };
    if conv.is_null() {
        return NumberFormat::default();
    }
    let bytes = |ptr: *const libc::c_char| -> Vec<u8> {
        if ptr.is_null() {
            return Vec::new();
        }
        // SAFETY: localeconv strings are NUL-terminated.
        unsafe { CStr::from_ptr(ptr) }.to_bytes().to_vec()
    };
    // SAFETY: `conv` is non-null and points to a valid lconv.
    let (decimal, thousands, minus) = unsafe {
        (
            bytes((*conv).decimal_point),
            bytes((*conv).thousands_sep),
            bytes((*conv).negative_sign),
        )
    };
    let default = NumberFormat::default();
    NumberFormat {
        point: Symbol::first(&decimal, utf8).or(default.point),
        thousands: Symbol::first(&thousands, utf8),
        minus: Symbol::first(&minus, utf8).or(default.minus),
        // strtod matches the whole string; longer than 4 bytes never occurs.
        decimal: match decimal.len() {
            1..=4 => {
                let mut bytes = [0; 4];
                bytes[..decimal.len()].copy_from_slice(&decimal);
                Symbol {
                    bytes,
                    len: decimal.len() as u8,
                }
            }
            _ => default.decimal,
        },
    }
}

impl Collator {
    /// Compare as sort's `bwscoll` does. In a locale this is strcoll on
    /// each NUL-separated part in turn (a part that ends first sorts first),
    /// then the shorter string (in characters) first. Strings that collate
    /// the same and have the same length are equal, even if their bytes
    /// differ: macOS gives no weight to some scripts (all Cyrillic collates
    /// equal in en_US.UTF-8), and sort keeps such lines in input order.
    pub fn compare(self, a: &[u8], b: &[u8]) -> Ordering {
        match self {
            Collator::Bytes => a.cmp(b),
            Collator::Locale { utf8 } => {
                if utf8 && (std::str::from_utf8(a).is_err() || std::str::from_utf8(b).is_err()) {
                    return a.cmp(b);
                }
                collate_parts(a, b, utf8)
            }
        }
    }
}

/// Length in characters: UTF-8 code points, or bytes.
fn units(s: &[u8], utf8: bool) -> usize {
    if utf8 {
        s.iter().filter(|&&c| (c as i8) >= -0x40).count()
    } else {
        s.len()
    }
}

/// [`Collator::compare`] for a locale, on valid text.
fn collate_parts(a: &[u8], b: &[u8], utf8: bool) -> Ordering {
    fn part(s: &[u8], at: usize) -> &[u8] {
        let rest = &s[at..];
        &rest[..rest.iter().position(|&c| c == 0).unwrap_or(rest.len())]
    }
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        match (a[i], b[j]) {
            (0, 0) => {
                i += 1;
                j += 1;
                continue;
            }
            (0, _) => return Ordering::Less,
            (_, 0) => return Ordering::Greater,
            _ => {}
        }
        let (pa, pb) = (part(a, i), part(b, j));
        let ord = strcoll(pa, pb);
        if ord != Ordering::Equal {
            return ord;
        }
        // Collated the same: the part that is shorter ends first.
        match units(pa, utf8).cmp(&units(pb, utf8)) {
            Ordering::Equal => {
                i += pa.len();
                j += pb.len();
            }
            shorter => {
                let (ra, rb) = (a.len() - i, b.len() - j);
                // Reaching the end of a whole string decides by total length.
                if (shorter == Ordering::Less && pa.len() == ra)
                    || (shorter == Ordering::Greater && pb.len() == rb)
                {
                    break;
                }
                return shorter;
            }
        }
    }
    units(a, utf8).cmp(&units(b, utf8))
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

/// Whether strxfrm keys may stand in for strcoll on `key`. macOS strxfrm
/// disagrees with strcoll on tabs in every UTF-8 locale tried, on bytes
/// from 0x80 in single-byte ones (en_US.ISO8859-15), and in kk_KZ.PT154 even
/// on ASCII. Scans of every BMP and plane-1 character in several UTF-8
/// locales found no other disagreement (`Å`, whose strxfrm copies the input,
/// is caught by [`xfrm`]). So: in UTF-8 any key without control characters,
/// otherwise printable ASCII only, and only if the locale passes
/// [`xfrm_agrees`].
pub fn xfrm_usable(key: &[u8], utf8: bool) -> bool {
    static AGREES: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let plain = if utf8 {
        key.iter().all(|&b| b >= 0x20 && b != 0x7f)
    } else {
        key.iter().all(|&b| (0x20..0x7f).contains(&b))
    };
    plain && *AGREES.get_or_init(xfrm_agrees)
}

/// Compare strxfrm with strcoll on a few hundred printable-ASCII pairs, most
/// of them one edit apart, as a scan of every macOS locale found the bad
/// one (kk_KZ.PT154) with these.
fn xfrm_agrees() -> bool {
    let mut state: u32 = 0x9e37_79b9;
    let mut next = move |n: u32| {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state % n
    };
    let (mut xa, mut xb) = (Vec::new(), Vec::new());
    for _ in 0..400 {
        let a: Vec<u8> = (0..next(9)).map(|_| 0x20 + next(95) as u8).collect();
        let mut b = a.clone();
        match (next(3), b.len()) {
            (0, _) => b.insert(next(b.len() as u32 + 1) as usize, 0x20 + next(95) as u8),
            (1, n) if n > 0 => {
                b.remove(next(n as u32) as usize);
            }
            (_, n) if n > 0 => b[next(n as u32) as usize] = 0x20 + next(95) as u8,
            _ => b = (0..next(9)).map(|_| 0x20 + next(95) as u8).collect(),
        }
        xa.clear();
        xb.clear();
        let (Some((s, e)), Some((t, f))) = (xfrm(&a, &mut xa), xfrm(&b, &mut xb)) else {
            continue;
        };
        let by_xfrm = xa[s..e].cmp(&xb[t..f]);
        if by_xfrm != Ordering::Equal && by_xfrm != strcoll(&a, &b) {
            return false;
        }
    }
    true
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

    #[test]
    fn locale_order_collates_each_nul_separated_part_then_length() {
        // The test process is in the C locale, where strcoll is strcmp.
        let c = Collator::Locale { utf8: true };
        let o = |a: &[u8], b: &[u8]| c.compare(a, b);
        assert_eq!(o(b"a\0b", b"a\0c"), Ordering::Less);
        assert_eq!(o(b"a\0c", b"ab"), Ordering::Less, "a part ends first");
        assert_eq!(o(b"ab", b"a\0c"), Ordering::Greater);
        assert_eq!(o(b"\0\0b", b"\0\0a"), Ordering::Greater);
        assert_eq!(o(b"\0a", b"a"), Ordering::Less);
        assert_eq!(o(b"a", b"a\0"), Ordering::Less, "then length");
        assert_eq!(o(b"a\0", b"a"), Ordering::Greater);
        assert_eq!(o("é".as_bytes(), "é".as_bytes()), Ordering::Equal);
        assert_eq!(o(b"\xff", b"a"), Ordering::Greater, "invalid UTF-8: bytes");
        assert_eq!(units("aé".as_bytes(), true), 2);
        assert_eq!(units("aé".as_bytes(), false), 3);
    }

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key| pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    #[test]
    fn each_category_reads_lc_all_then_its_own_variable_then_lang() {
        let name = |pairs: &[(&str, &str)]| category_name(&env(pairs), "LC_COLLATE");
        assert_eq!(name(&[]), "C");
        assert_eq!(name(&[("LANG", "x")]), "x");
        assert_eq!(name(&[("LANG", "x"), ("LC_COLLATE", "y")]), "y");
        assert_eq!(name(&[("LANG", "x"), ("LC_CTYPE", "y")]), "x");
        assert_eq!(
            name(&[("LC_ALL", ""), ("LC_COLLATE", "y"), ("LANG", "x")]),
            "y"
        );
        assert_eq!(name(&[("LC_ALL", "z"), ("LC_COLLATE", "y")]), "z");
    }

    #[test]
    fn c_posix_and_unknown_locales_are_bytes() {
        for pairs in [
            &[][..],
            &[("LANG", "C")],
            &[("LC_ALL", "POSIX")],
            &[("LANG", "no_SUCH.locale")],
            &[("LANG", "bad\0name")],
            // One unknown category leaves every category C.
            &[("LANG", "C"), ("LC_MESSAGES", "no_SUCH.locale")],
        ] {
            assert_eq!(
                Locale::from_env(&env(pairs)),
                Locale::default(),
                "{pairs:?}"
            );
        }
        assert_eq!(NumberFormat::default().point, Some(Symbol::byte(b'.')));
        assert_eq!(Collator::Bytes.compare(b"a", b"B"), Ordering::Greater);
    }

    #[test]
    fn locale_compare_skips_invalid_utf8() {
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
