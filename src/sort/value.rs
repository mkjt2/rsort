//! Key values: a fixed-size summary per key that decides most comparisons,
//! and the full comparison of two keys for when summaries can't.
//!
//! Parts of this follow FreeBSD sort's source closely; see NOTICE.

use super::collate::{self, Collator, NumberFormat, Symbol};
use super::ctype;
use super::key::{Mode, Order, is_blank, skip_blanks};
use std::cell::RefCell;
use std::cmp::Ordering;
use std::hash::Hasher;

/// What key comparisons depend on beyond the key bytes.
#[derive(Debug, Clone, Copy, Default)]
pub struct Context {
    /// Seed for `-R` hashes.
    pub seed: u64,
    /// UTF-8 locale: text options work by character.
    pub utf8: bool,
    pub numbers: NumberFormat,
    pub collator: Collator,
}

/// A 128-bit summary of a key.
pub type Bits = u128;

/// What a record keeps about one key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Summary {
    /// Summaries order like the keys they summarize: a < b implies key a
    /// sorts before key b. Equal summaries mean equal keys only if `exact`.
    Ordered { bits: Bits, exact: bool },
    /// Not ordered, but saves work in [`compare`]: the `-V` version length
    /// ([`version_prefix`]).
    Version(usize),
    /// Nothing usable: compare the keys.
    None,
}

/// Summarize a key (already rewritten by -d/-f/-i).
pub fn summarize(key: &[u8], mode: Mode, cx: &Context) -> Summary {
    let ordered = |bits, exact| Summary::Ordered { bits, exact };
    match mode {
        Mode::Text => match cx.collator {
            Collator::Bytes => {
                let (bits, exact) = bytes_bits(key);
                ordered(bits, exact)
            }
            Collator::Locale { utf8 } => {
                if utf8 && std::str::from_utf8(key).is_err() {
                    return Summary::None;
                }
                if !collate::xfrm_usable(key, utf8) {
                    return Summary::None;
                }
                // Equal collation keys may still differ: never exact.
                with_scratch(|buf| match collate::xfrm(key, buf) {
                    Some((b, e)) => ordered(bytes_bits(&buf[b..e]).0, false),
                    None => Summary::None,
                })
            }
        },
        Mode::Numeric | Mode::Human => {
            match Number::parse(key, &cx.numbers).bits(mode == Mode::Human) {
                Some((bits, exact)) => ordered(bits, exact),
                None => Summary::None,
            }
        }
        Mode::General => match General::parse(key, cx).bits() {
            Some(bits) => ordered(bits, true),
            None => Summary::None,
        },
        Mode::Month => ordered(Bits::from(month(key)), true),
        Mode::Random => ordered(Bits::from(random_hash(key, cx.seed)) << 64, false),
        Mode::Version => Summary::Version(version_prefix(key)),
    }
}

/// Compare two keys (already rewritten by -d/-f/-i) in `mode`. `prefixes`
/// are the keys' cached [`Summary::Version`] values, if any.
pub fn compare(
    a: &[u8],
    b: &[u8],
    mode: Mode,
    cx: &Context,
    prefixes: Option<(usize, usize)>,
) -> Ordering {
    match mode {
        Mode::Text => text(a, b, cx.collator),
        Mode::Numeric | Mode::Human => Number::parse(a, &cx.numbers)
            .compare(&Number::parse(b, &cx.numbers), mode == Mode::Human),
        Mode::General => General::parse(a, cx).compare(&General::parse(b, cx)),
        Mode::Month => month(a).cmp(&month(b)),
        Mode::Random => random_hash(a, cx.seed)
            .cmp(&random_hash(b, cx.seed))
            .then_with(|| text(a, b, cx.collator)),
        Mode::Version => match prefixes {
            Some((pa, pb)) => version_cmp_with(a, pa, b, pb),
            None => version_cmp(a, b),
        },
    }
}

/// Text order: collation (when the locale has one), then bytes.
fn text(a: &[u8], b: &[u8], collator: Collator) -> Ordering {
    if a == b {
        return Ordering::Equal;
    }
    collator.compare(a, b)
}

fn random_hash(key: &[u8], seed: u64) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    h.write_u64(seed);
    h.write(key);
    h.finish()
}

/// Bytes as a summary: the first 15, zero-padded, then the length (255 when
/// longer than 15). Comparing these never contradicts comparing the bytes,
/// and equal summaries of keys up to 15 bytes mean equal keys.
fn bytes_bits(b: &[u8]) -> (Bits, bool) {
    let mut buf = [0u8; 16];
    let n = b.len().min(15);
    buf[..n].copy_from_slice(&b[..n]);
    let exact = b.len() <= 15;
    buf[15] = if exact { n as u8 } else { u8::MAX };
    (Bits::from_be_bytes(buf), exact)
}

thread_local! {
    static SCRATCH: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

fn with_scratch<T>(f: impl FnOnce(&mut Vec<u8>) -> T) -> T {
    SCRATCH.with_borrow_mut(|buf| {
        buf.clear();
        f(buf)
    })
}

/// The bits of a non-negative number with `int` and `frac` digits, `digits`
/// of them kept: the integer length (255 when 255 or more, with no digits
/// kept), then up to `digits` digits as nibbles, then 1 if some were cut.
/// `8 + 4 * digits + 1` bits wide; higher means larger.
fn magnitude_bits(int: &[u8], frac: &[u8], digits: usize) -> (Bits, bool) {
    if int.len() >= 255 {
        return ((0xFF << (4 * digits + 1)) | 1, false);
    }
    let mut bits = int.len() as Bits;
    let mut all = int.iter().chain(frac);
    for _ in 0..digits {
        let d = all.next().map_or(0, |&d| d - b'0');
        bits = (bits << 4) | Bits::from(d);
    }
    let cut = all.next().is_some();
    ((bits << 1) | Bits::from(cut), !cut)
}

/// Sign class (0 negative, 1 zero, 2 positive) in the top two bits, then
/// `field` (`width` bits, larger for larger magnitudes), inverted for
/// negatives so that larger magnitudes sort first.
fn signed_bits(sign: Ordering, field: Bits, width: u32) -> Bits {
    let mask = (1 << width) - 1;
    match sign {
        Ordering::Less => field ^ mask,
        Ordering::Equal => 1 << 126,
        Ordering::Greater => (2 << 126) | field,
    }
}

/// Apply -d (keep blanks and alphanumerics) or else -i (keep printable),
/// then -f (uppercase; -M uppercases too). As in sort, -d wins over -i:
/// with both, a tab is kept. Classes are the locale's: by character for
/// valid text in a UTF-8 locale, by byte otherwise (ASCII for invalid
/// UTF-8).
pub fn transform(raw: &[u8], order: &Order, utf8: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    transform_into(raw, order, utf8, &mut out);
    out
}

/// [`transform`], appending to `out`.
pub fn transform_into(raw: &[u8], order: &Order, utf8: bool, out: &mut Vec<u8>) {
    let fold = order.fold || order.mode == Mode::Month;
    if utf8 {
        if let Ok(text) = std::str::from_utf8(raw) {
            return transform_chars(text, order, fold, out);
        }
        let keep = |b: u8| {
            if order.dictionary {
                is_blank(b) || b.is_ascii_alphanumeric()
            } else if order.nonprinting {
                (0x20..0x7f).contains(&b)
            } else {
                true
            }
        };
        let up = |b: u8| if fold { b.to_ascii_uppercase() } else { b };
        out.extend(raw.iter().filter(|&&b| keep(b)).map(|&b| up(b)));
        return;
    }
    let keep = |b: u8| {
        if order.dictionary {
            is_blank(b) || ctype::is_alnum(b)
        } else if order.nonprinting {
            ctype::is_print(b)
        } else {
            true
        }
    };
    let up = |b: u8| if fold { ctype::to_upper(b) } else { b };
    out.extend(raw.iter().filter(|&&b| keep(b)).map(|&b| up(b)));
}

fn transform_chars(text: &str, order: &Order, fold: bool, out: &mut Vec<u8>) {
    let keep = |c: char| {
        if order.dictionary {
            ctype::is_wblank(c) || ctype::is_walnum(c)
        } else if order.nonprinting {
            ctype::is_wprint(c)
        } else {
            true
        }
    };
    let mut buf = [0u8; 4];
    for c in text.chars().filter(|&c| keep(c)) {
        let c = if fold { ctype::upper(c) } else { c };
        out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
    }
}

// --- -n and -h --------------------------------------------------------------

/// Digits sort keeps of each part of a number (`MAX_NUM_SIZE`).
const MAX_DIGITS: usize = 128;

/// A number as sort's `read_number` sees it: a sign (-1, 0 for no digits,
/// 1), the integer digits with leading zeros dropped (only those before any
/// thousands separator), the fraction digits without trailing zeros, at most
/// 128 of each, and the `-h` suffix after them (1 for K or k, up to 8 for Y).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Number {
    sign: i8,
    int: Vec<u8>,
    frac: Vec<u8>,
    suffix: u8,
}

impl Number {
    /// Parse `[blanks][minus][digits, with thousands separators][point
    /// digits][suffix]`. A thousands separator may not follow another.
    pub fn parse(s: &[u8], fmt: &NumberFormat) -> Number {
        let at = |sym: Option<Symbol>, i: usize| sym.filter(|m| m.is_at(s, i)).map(|m| m.width());
        let digit = |i: usize| s.get(i).is_some_and(u8::is_ascii_digit);
        let mut i = skip_blanks(s, 0);
        let negative = at(fmt.minus, i);
        i += negative.unwrap_or(0);
        while s.get(i) == Some(&b'0') {
            i += 1;
        }
        let mut int = Vec::new();
        let mut after_separator = false;
        while int.len() < MAX_DIGITS {
            if digit(i) {
                int.push(s[i]);
                i += 1;
                after_separator = false;
            } else if let Some(n) = at(fmt.thousands, i).filter(|_| !after_separator) {
                i += n;
                after_separator = true;
            } else {
                break;
            }
        }
        let mut frac = Vec::new();
        if let Some(n) = at(fmt.point, i) {
            i += n;
            while digit(i) && frac.len() < MAX_DIGITS {
                frac.push(s[i]);
                i += 1;
            }
            let keep = frac.iter().rposition(|&d| d != b'0').map_or(0, |p| p + 1);
            frac.truncate(keep);
        }
        let suffix = match s.get(i) {
            Some(b'k') => 1,
            Some(&c) => b"KMGTPEZY"
                .iter()
                .position(|&x| x == c)
                .map_or(0, |p| p as u8 + 1),
            None => 0,
        };
        let sign = if int.is_empty() && frac.is_empty() {
            0
        } else if negative.is_some() {
            -1
        } else {
            1
        };
        Number {
            sign,
            int,
            frac,
            suffix,
        }
    }

    /// The integer part as sort's hint holds it (wcstoull), `None` when it
    /// overflows to ULLONG_MAX.
    fn hint(&self) -> Option<u64> {
        let mut n: u64 = 0;
        for &d in &self.int {
            n = n.checked_mul(10)?.checked_add(u64::from(d - b'0'))?;
        }
        (n != u64::MAX).then_some(n)
    }

    /// sort's `numcoll_impl`: no digits sorts as zero; negatives first; with
    /// `suffix` (-h) the suffix decides next; then the integer values, and
    /// on a tie the digit strings (integer length, integer, fraction). The
    /// two differ only for integers with leading zeros, which a thousands
    /// separator can leave (`,05` is 5, but after every other 5).
    pub fn compare(&self, other: &Number, suffix: bool) -> Ordering {
        let (a, b) = (self, other);
        if a.sign == 0 || b.sign == 0 {
            return a.sign.cmp(&b.sign);
        }
        if a.sign != b.sign {
            return a.sign.cmp(&b.sign);
        }
        let flip = |o: Ordering| if a.sign < 0 { o.reverse() } else { o };
        if suffix && a.suffix != b.suffix {
            return flip(a.suffix.cmp(&b.suffix));
        }
        if let (Some(x), Some(y)) = (a.hint(), b.hint())
            && x != y
        {
            return flip(x.cmp(&y));
        }
        flip(
            a.int
                .len()
                .cmp(&b.int.len())
                .then_with(|| a.int.cmp(&b.int))
                .then_with(|| a.frac.cmp(&b.frac)),
        )
    }

    /// Summary bits (see [`Summary::Ordered`]) and whether they are exact,
    /// or `None` for an integer with leading zeros, which orders by value
    /// first and so doesn't fit the digit-string summary.
    fn bits(&self, suffix: bool) -> Option<(Bits, bool)> {
        if self.int.first() == Some(&b'0') {
            return None;
        }
        let sign = self.sign.cmp(&0);
        if !suffix {
            // 8 + 4 * 29 + 1 = 125 bits, shifted left once: 126 below the sign.
            let (field, exact) = magnitude_bits(&self.int, &self.frac, 29);
            return Some((signed_bits(sign, field << 1, 126), exact));
        }
        // 4 + 8 + 4 * 28 + 1 = 125 bits, shifted left once: 126 below the sign.
        let (magnitude, exact) = magnitude_bits(&self.int, &self.frac, 28);
        let field = ((Bits::from(self.suffix) << 121) | magnitude) << 1;
        Some((signed_bits(sign, field, 126), exact || self.sign == 0))
    }
}

// --- -g --------------------------------------------------------------------

/// A strtod-style value. Order: not a number < NaN < negative overflow <
/// numbers (-inf..inf) < positive overflow.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum General {
    NotNumber,
    NaN,
    NegOverflow,
    Number(f64),
    PosOverflow,
}

impl General {
    fn rank(&self) -> u8 {
        match self {
            General::NotNumber => 0,
            General::NaN => 1,
            General::NegOverflow => 2,
            General::Number(_) => 3,
            General::PosOverflow => 4,
        }
    }

    /// Summary bits: the rank, then the number's bits in an order-preserving
    /// form (-0 as 0). `None` for a NaN number, which compares equal to
    /// every number.
    fn bits(&self) -> Option<Bits> {
        let number = match self {
            General::Number(x) if x.is_nan() => return None,
            General::Number(x) if *x == 0.0 => 1 << 63,
            General::Number(x) => {
                let b = x.to_bits();
                if b >> 63 == 1 { !b } else { b | (1 << 63) }
            }
            _ => 0,
        };
        Some((Bits::from(self.rank()) << 64) | Bits::from(number))
    }

    pub fn compare(&self, other: &General) -> Ordering {
        match (self, other) {
            (General::Number(x), General::Number(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
            _ => self.rank().cmp(&other.rank()),
        }
    }

    /// Parse as sort's `bwstod`: after leading blanks, a key that doesn't
    /// start with a printable character is not a number; otherwise the
    /// longest strtod prefix, with the locale's decimal point.
    pub fn parse(s: &[u8], cx: &Context) -> General {
        let mut i = skip_blanks(s, 0);
        let printable = if cx.utf8 {
            ctype::first_char(&s[i..]).is_some_and(ctype::is_wprint)
        } else {
            s.get(i).is_some_and(|&b| ctype::is_print(b))
        };
        if !printable {
            return General::NotNumber;
        }
        // Then strtod (wcstod in UTF-8) skips white space, Unicode spaces
        // like U+00A0 included.
        loop {
            if cx.utf8 {
                match ctype::first_char(&s[i..]) {
                    Some(c) if ctype::is_wspace(c) => i += c.len_utf8(),
                    _ => break,
                }
            } else if s.get(i).is_some_and(|&b| ctype::is_space(b)) {
                i += 1;
            } else {
                break;
            }
        }
        let decimal = cx.numbers.decimal;
        let negative = s.get(i) == Some(&b'-');
        if matches!(s.get(i), Some(b'-' | b'+')) {
            i += 1;
        }
        let rest = &s[i..];
        let starts =
            |word: &[u8]| rest.len() >= word.len() && rest[..word.len()].eq_ignore_ascii_case(word);
        let value = if starts(b"inf") {
            f64::INFINITY
        } else if starts(b"nan") {
            return General::NaN;
        } else if let Some(v) = parse_hex(rest, decimal) {
            v
        } else {
            let len = decimal_prefix(rest, decimal);
            if len == 0 {
                return General::NotNumber;
            }
            let text = String::from_utf8_lossy(&rest[..len])
                .replace(&*String::from_utf8_lossy(decimal.as_bytes()), ".");
            let v: f64 = text.parse().unwrap_or(0.0);
            if v.is_infinite() {
                return if negative {
                    General::NegOverflow
                } else {
                    General::PosOverflow
                };
            }
            v
        };
        General::Number(if negative { -value } else { value })
    }
}

/// Length of `digits[.digits][e[+-]digits]` with at least one mantissa digit.
fn decimal_prefix(s: &[u8], decimal: Symbol) -> usize {
    let digits = |from: usize| s[from..].iter().take_while(|b| b.is_ascii_digit()).count();
    let int = digits(0);
    let mut i = int;
    let mut frac = 0;
    if decimal.is_at(s, i) {
        frac = digits(i + decimal.width());
        if int + frac > 0 {
            i += decimal.width() + frac;
        }
    }
    if int + frac == 0 {
        return 0;
    }
    if matches!(s.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(s.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        let exp = digits(j);
        if exp > 0 {
            i = j + exp;
        }
    }
    i
}

/// `0x` hex floats: hex digits, optional fraction, optional `p` exponent.
fn parse_hex(s: &[u8], decimal: Symbol) -> Option<f64> {
    let body = s.strip_prefix(b"0x").or_else(|| s.strip_prefix(b"0X"))?;
    let hex = |b: &u8| (*b as char).to_digit(16);
    let mut value = 0f64;
    let mut i = 0;
    let mut any = false;
    while let Some(d) = body.get(i).and_then(hex) {
        value = value * 16.0 + f64::from(d);
        i += 1;
        any = true;
    }
    if decimal.is_at(body, i) {
        let mut scale = 1.0 / 16.0;
        i += decimal.width();
        while let Some(d) = body.get(i).and_then(hex) {
            value += f64::from(d) * scale;
            scale /= 16.0;
            i += 1;
            any = true;
        }
    }
    if !any {
        return None;
    }
    if matches!(body.get(i), Some(b'p' | b'P')) {
        let mut j = i + 1;
        let neg = body.get(j) == Some(&b'-');
        if matches!(body.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        let digits: String = body[j..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .map(|&b| b as char)
            .collect();
        if let Ok(e) = digits.parse::<i32>() {
            value *= 2f64.powi(if neg { -e } else { e });
        }
    }
    Some(value)
}

// --- -M --------------------------------------------------------------------

/// 1..=12 for a key (already uppercased) that starts with one of the
/// locale's month abbreviations after leading blanks, 0 otherwise. Like
/// sort, December is tried first.
pub fn month(s: &[u8]) -> u8 {
    let s = &s[skip_blanks(s, 0)..];
    let months = ctype::months();
    (0..12)
        .rev()
        .find(|&i| !months[i].is_empty() && s.starts_with(&months[i]))
        .map_or(0, |i| i as u8 + 1)
}

// --- -V --------------------------------------------------------------------

/// sort's `vcmp`: `.` and `..` and hidden files first, then the version
/// parts (before any `.ext` suffix) compared with digit runs as numbers and
/// `~` before everything, then bytes.
pub fn version_cmp(a: &[u8], b: &[u8]) -> Ordering {
    version_cmp_with(a, version_prefix(a), b, version_prefix(b))
}

/// The length of the version part of `s` (after one leading dot, which
/// `vcmp` skips): up to a trailing `(\.[A-Za-z~][A-Za-z0-9~]*)*` suffix, and
/// never past a NUL, where sort's `find_suffix` stops reading.
pub fn version_prefix(s: &[u8]) -> usize {
    let s = &s[usize::from(s.first() == Some(&b'.'))..];
    let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    let s = &s[..end];
    let mut found = None;
    let mut after_dot = false;
    for (i, &c) in s.iter().enumerate() {
        if after_dot {
            after_dot = false;
            if !c.is_ascii_alphabetic() && c != b'~' {
                found = None;
            }
        } else if c == b'.' {
            after_dot = true;
            found.get_or_insert(i);
        } else if !c.is_ascii_alphanumeric() && c != b'~' {
            found = None;
        }
    }
    found.unwrap_or(end)
}

/// [`version_cmp`] with each side's [`version_prefix`] precomputed.
pub fn version_cmp_with(a: &[u8], a_len: usize, b: &[u8], b_len: usize) -> Ordering {
    let bytes = a.cmp(b);
    if bytes == Ordering::Equal {
        return bytes;
    }
    for special in [&b""[..], b".", b".."] {
        if a == special {
            return Ordering::Less;
        }
        if b == special {
            return Ordering::Greater;
        }
    }
    let (a, b) = match (a.first() == Some(&b'.'), b.first() == Some(&b'.')) {
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        (true, true) => (&a[1..], &b[1..]),
        (false, false) => (a, b),
    };
    let (a, b) = (&a[..a_len], &b[..b_len]);
    if a == b {
        return bytes;
    }
    match verrevcmp(a, b) {
        Ordering::Equal => bytes,
        ord => ord,
    }
}

fn char_order(c: Option<&u8>) -> i32 {
    match c {
        None => 0,
        Some(&c) if c.is_ascii_digit() => 0,
        Some(&c) if c.is_ascii_alphabetic() => i32::from(c),
        Some(b'~') => -1,
        Some(&c) => i32::from(c) + 256,
    }
}

fn verrevcmp(a: &[u8], b: &[u8]) -> Ordering {
    let digit = |s: &[u8], i: usize| s.get(i).is_some_and(u8::is_ascii_digit);
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        while (i < a.len() && !digit(a, i)) || (j < b.len() && !digit(b, j)) {
            let (ca, cb) = (char_order(a.get(i)), char_order(b.get(j)));
            if ca != cb {
                return ca.cmp(&cb);
            }
            i += 1;
            j += 1;
        }
        while a.get(i) == Some(&b'0') {
            i += 1;
        }
        while b.get(j) == Some(&b'0') {
            j += 1;
        }
        let mut first_diff = Ordering::Equal;
        while digit(a, i) && digit(b, j) {
            if first_diff == Ordering::Equal {
                first_diff = a[i].cmp(&b[j]);
            }
            i += 1;
            j += 1;
        }
        if digit(a, i) {
            return Ordering::Greater;
        }
        if digit(b, j) {
            return Ordering::Less;
        }
        if first_diff != Ordering::Equal {
            return first_diff;
        }
    }
    Ordering::Equal
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format(point: u8, thousands: Option<u8>) -> NumberFormat {
        NumberFormat {
            point: Some(Symbol::byte(point)),
            thousands: thousands.map(Symbol::byte),
            decimal: Symbol::byte(point),
            ..NumberFormat::default()
        }
    }

    fn cmp_with(fmt: &NumberFormat, a: &str, b: &str, suffix: bool) -> Ordering {
        Number::parse(a.as_bytes(), fmt).compare(&Number::parse(b.as_bytes(), fmt), suffix)
    }

    fn n(a: &str, b: &str) -> Ordering {
        cmp_with(&NumberFormat::default(), a, b, false)
    }

    fn us(a: &str, b: &str) -> Ordering {
        cmp_with(&format(b'.', Some(b',')), a, b, false)
    }

    use Ordering::{Equal, Greater, Less};

    #[test]
    fn numbers() {
        assert_eq!(n("-0", "0"), Equal);
        assert_eq!(n("abc", ""), Equal);
        assert_eq!(n("007.50", "7.5"), Equal);
        assert_eq!(n("-", "0"), Equal);
        assert_eq!(n("-2", "-1.5"), Less);
        assert_eq!(n(".5", "1"), Less);
        assert_eq!(n("10", "9.99"), Greater);
        assert_eq!(n("-1", ""), Less);
        assert_eq!(n("", "0.1"), Less);
        assert_eq!(n("99999999999999999999", "99999999999999999998"), Greater);
        assert_eq!(n("18446744073709551616", "18446744073709551615"), Greater);
    }

    #[test]
    fn numbers_keep_128_digits() {
        let long = |last: char| format!("{}{last}", "1".repeat(128));
        assert_eq!(n(&long('1'), &long('2')), Equal);
        let frac = |last: char| format!("0.{}{last}", "1".repeat(128));
        assert_eq!(n(&frac('1'), &frac('2')), Equal);
    }

    #[test]
    fn thousands_separators_follow_sort() {
        assert_eq!(us("1,000", "1000"), Equal);
        assert_eq!(us("1,,0", "1"), Equal, "not after another");
        assert_eq!(us(",.9", "0.9"), Equal);
        assert_eq!(us("1,.5", "1.5"), Equal);
        assert_eq!(us("1,", "1"), Equal);
        assert_eq!(us("-1,000.5", "-1000.5"), Equal);
        assert_eq!(us("1.5,0", "1.5"), Equal, "none in the fraction");
        // Leading zeros are dropped only before a separator, so these keep
        // one: they order by value, then after the plain numbers equal to it.
        assert_eq!(us(",05", "5"), Greater);
        assert_eq!(us(",05", "5.9"), Greater);
        assert_eq!(us(",05", "6"), Less);
        assert_eq!(us("0,0", "0"), Greater);
        assert_eq!(us("0,0", "0.5"), Greater);
        assert_eq!(us("0,0", "1"), Less);
        let de = format(b',', Some(b'.'));
        assert_eq!(cmp_with(&de, "1.234,5", "1234,5", false), Equal);
        assert_eq!(cmp_with(&de, "1,5", "1,4", false), Greater);
        let whole = NumberFormat {
            point: None,
            ..format(b'.', Some(b','))
        };
        assert_eq!(cmp_with(&whole, "1,000.5", "1000", false), Equal);
        assert_eq!(cmp_with(&whole, "1,.5", "1", false), Equal);
    }

    #[test]
    fn locale_symbols_may_be_multibyte() {
        let sym = |s: &str| Symbol::first(s.as_bytes(), true);
        let sv = NumberFormat {
            point: sym(","),
            thousands: sym("\u{a0}"),
            minus: sym("\u{2212}"),
            decimal: Symbol::byte(b','),
        };
        let c = |a: &str, b: &str| cmp_with(&sv, a, b, false);
        assert_eq!(c("1\u{a0}000", "1000"), Equal);
        assert_eq!(c("\u{2212}5", "0"), Less);
        assert_eq!(c("-5", "0"), Equal, "'-' is not this locale's minus");
        assert_eq!(c("1,5", "1,4"), Greater);
        assert_eq!(Symbol::first(b"", true), None);
        assert_eq!(Symbol::first(b"\xff", true), None);
        assert_eq!(Symbol::first(b"\xf3\xae", false), Some(Symbol::byte(0xf3)));
        let fmt = NumberFormat::default().separated_by(b'-');
        assert_eq!(
            cmp_with(&fmt, "-5", "0", false),
            Equal,
            "-t - disables minus"
        );
        assert_eq!(fmt.separated_by(b'.').point, None);
    }

    #[test]
    fn human() {
        let h = |a: &str, b: &str| cmp_with(&NumberFormat::default(), a, b, true);
        assert_eq!(h("2", "1K"), Less);
        assert_eq!(h("1K", "1k"), Equal);
        assert_eq!(h("999K", "1M"), Less);
        assert_eq!(h("-1M", "-1K"), Less);
        assert_eq!(h("-2K", "-1K"), Less);
        assert_eq!(h("0K", "K"), Equal);
        assert_eq!(h("1Y", "1Z"), Greater);
        assert_eq!(
            cmp_with(&format(b'.', Some(b',')), "1,0K", "2K", true),
            Greater
        );
        assert_eq!(
            cmp_with(&format(b'.', Some(b',')), "1,K", "2", true),
            Greater
        );
    }

    #[test]
    fn summaries_agree_with_comparison() {
        let fmt = format(b'.', Some(b','));
        let keys = [
            "",
            "0",
            "-0",
            "1",
            "-1",
            "1.5",
            "-1.5",
            "10",
            "9.99",
            "1,000",
            ",05",
            "0,0",
            "5",
            "5.9",
            "6",
            "1K",
            "2M",
            "-3G",
            "99999999999999999999999999999999",
        ];
        for suffix in [false, true] {
            for a in keys {
                for b in keys {
                    let (x, y) = (
                        Number::parse(a.as_bytes(), &fmt),
                        Number::parse(b.as_bytes(), &fmt),
                    );
                    let (Some((bx, ex)), Some((by, ey))) = (x.bits(suffix), y.bits(suffix)) else {
                        continue;
                    };
                    let ord = x.compare(&y, suffix);
                    match bx.cmp(&by) {
                        Equal if ex && ey => assert_eq!(ord, Equal, "{a} {b} {suffix}"),
                        Equal => {}
                        bits => assert_eq!(ord, bits, "{a} {b} {suffix}"),
                    }
                }
            }
        }
        assert_eq!(Number::parse(b",05", &fmt).bits(false), None);
    }

    #[test]
    fn general() {
        let cx = Context {
            seed: 0,
            utf8: false,
            numbers: NumberFormat::default(),
            collator: Collator::Bytes,
        };
        let g = |s: &str| General::parse(s.as_bytes(), &cx);
        assert_eq!(g("abc"), General::NotNumber);
        assert_eq!(g("-nan"), General::NaN);
        assert_eq!(g("  -INFINITY"), General::Number(f64::NEG_INFINITY));
        assert_eq!(g("1e400"), General::PosOverflow);
        assert_eq!(g("-1e400"), General::NegOverflow);
        assert_eq!(g("1e"), General::Number(1.0));
        assert_eq!(g("1e+"), General::Number(1.0));
        assert_eq!(g("+.5e1x"), General::Number(5.0));
        assert_eq!(g("0x1.8"), General::Number(1.5));
        assert_eq!(g("0X1p-1"), General::Number(0.5));
        assert_eq!(g("0xg"), General::Number(0.0));
        assert_eq!(g("."), General::NotNumber);
        // After blanks the key must start with a printable character.
        assert_eq!(g("\x0c5"), General::NotNumber);
        assert_eq!(g("\r5"), General::NotNumber);
        assert_eq!(g(" \t5"), General::Number(5.0));
        assert_eq!(
            g(" \x0b5"),
            General::NotNumber,
            "a vertical tab is not a blank"
        );
        assert_eq!(g("-0").compare(&g("0")), Ordering::Equal);
        assert_eq!(g("nan").compare(&g("abc")), Ordering::Greater);
        let comma = Context {
            numbers: format(b',', None),
            ..cx
        };
        let g = |s: &str| General::parse(s.as_bytes(), &comma);
        assert_eq!(g("1,5e1"), General::Number(15.0));
        assert_eq!(g("1.5"), General::Number(1.0));
        assert_eq!(g("0x1,8"), General::Number(1.5));
        let two_bytes = Context {
            numbers: NumberFormat {
                decimal: Symbol::first("\u{66b}".as_bytes(), true).unwrap(),
                ..NumberFormat::default()
            },
            ..cx
        };
        let g = |s: &str| General::parse(s.as_bytes(), &two_bytes);
        assert_eq!(g("1\u{66b}5"), General::Number(1.5));
        assert_eq!(g("1.5"), General::Number(1.0));
    }

    #[test]
    fn months() {
        // Keys arrive uppercased; the unit tests run in the C locale.
        assert_eq!(month(b" JAN"), 1);
        assert_eq!(month(b"DECEMBER"), 12);
        assert_eq!(month(b"jan"), 0);
        assert_eq!(month(b"JA"), 0);
        assert_eq!(month(b"XYZ"), 0);
    }

    #[test]
    fn versions() {
        let v = |a: &str, b: &str| version_cmp(a.as_bytes(), b.as_bytes());
        assert_eq!(v("a9", "a10"), Ordering::Less);
        assert_eq!(v("1.022", "1.23"), Ordering::Less);
        assert_eq!(v("1.23.1", "1.024"), Ordering::Less);
        assert_eq!(v("a~1", "a"), Ordering::Less);
        assert_eq!(v("a", "a"), Ordering::Equal);
        assert_eq!(v("a01", "a1"), Ordering::Less, "falls back to bytes");
        assert_eq!(v(".a", "a"), Ordering::Less);
        assert_eq!(v("a", ".a"), Ordering::Greater);
        assert_eq!(v(".b", ".a"), Ordering::Greater);
        assert_eq!(v("x", ""), Ordering::Greater);
        assert_eq!(v("x", "."), Ordering::Greater);
        assert_eq!(v("f-1.tar.gz", "f.tar.gz"), Ordering::Greater);
        // Equal versions compare as bytes, whatever the suffixes (sort's
        // vcmp; GNU would compare whole strings as versions here).
        assert_eq!(v("f.tar.gz", "f.tar.gz~"), Ordering::Less);
        assert_eq!(v("foo.a", "foo.a~"), Ordering::Less);
        assert_eq!(v("a.b10", "a.b2"), Ordering::Less);
        // The version part ends at a NUL.
        assert_eq!(v("sort.h\0z", "sort.o"), Ordering::Less);
        assert_eq!(v("a2\0x", "a10"), Ordering::Less);
        assert_eq!(v("a\0b", "a\0a"), Ordering::Greater);
    }

    #[test]
    fn transforms() {
        let order = Order {
            fold: true,
            dictionary: true,
            nonprinting: true,
            ..Order::default()
        };
        assert_eq!(transform(b"a-b\x01\t c\xe9", &order, false), b"AB\t C");
        let order = Order {
            nonprinting: true,
            ..Order::default()
        };
        assert_eq!(transform(b"a-b\x01\t c\xe9", &order, false), b"a-b c");
        // By character in UTF-8, with the C locale's classes here (the
        // locale tests compare real locales with sort).
        assert_eq!(
            transform("é\u{1}\t~".as_bytes(), &order, true),
            "~".as_bytes()
        );
        let fold = Order {
            fold: true,
            dictionary: true,
            ..Order::default()
        };
        assert_eq!(
            transform("a-b x".as_bytes(), &fold, true),
            "AB X".as_bytes()
        );
        let month = Order {
            mode: Mode::Month,
            ..Order::default()
        };
        assert_eq!(transform(b"jan", &month, false), b"JAN", "-M uppercases");
        assert_eq!(
            transform(b"\xffa", &fold, true),
            b"A",
            "invalid UTF-8 goes by bytes"
        );
    }
}
