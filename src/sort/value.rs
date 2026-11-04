//! Comparable key values, one kind per ordering mode, computed once per line.

use super::collate::NumberFormat;
use super::key::{Mode, Order, is_blank, skip_blanks};
use std::cmp::Ordering;
use std::hash::Hasher;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// Untransformed text: a byte range of the line.
    Range(usize, usize),
    /// Text after -d/-f/-i.
    Text(Vec<u8>),
    Numeric(Decimal),
    General(General),
    Human(Human),
    Month(u8),
    Version(Vec<u8>),
    Random(u64, Vec<u8>),
}

/// What key values depend on beyond the key itself.
#[derive(Debug, Clone, Copy, Default)]
pub struct Context {
    /// Seed for `-R` hashes.
    pub seed: u64,
    /// UTF-8 locale: text options work by character.
    pub utf8: bool,
    pub numbers: NumberFormat,
}

/// Compute the value of the key at `begin..end` of `line`. As in sort, -d,
/// -i and -f rewrite the key before any mode interprets it.
pub fn value(line: &[u8], begin: usize, end: usize, order: &Order, cx: &Context) -> Value {
    let end = end.max(begin);
    if order.mode == Mode::Text && !(order.fold || order.dictionary || order.nonprinting) {
        return Value::Range(begin, end);
    }
    let key = transform(&line[begin..end], order, cx.utf8);
    match order.mode {
        Mode::Text => Value::Text(key),
        Mode::Numeric => Value::Numeric(Decimal::parse(&key, &cx.numbers).0),
        Mode::General => Value::General(General::parse(&key, cx.numbers.decimal)),
        Mode::Human => Value::Human(Human::parse(&key, &cx.numbers)),
        Mode::Month => Value::Month(month(&key)),
        Mode::Version => Value::Version(key),
        Mode::Random => {
            let mut h = std::hash::DefaultHasher::new();
            h.write_u64(cx.seed);
            h.write(&key);
            Value::Random(h.finish(), key)
        }
    }
}

/// Compare two values of the same key. `text` compares text bytes (the
/// collation); `a_line`/`b_line` resolve `Range`s.
pub fn compare(
    a: &Value,
    a_line: &[u8],
    b: &Value,
    b_line: &[u8],
    text: &dyn Fn(&[u8], &[u8]) -> Ordering,
) -> Ordering {
    match (a, b) {
        (Value::Range(a0, a1), Value::Range(b0, b1)) => text(&a_line[*a0..*a1], &b_line[*b0..*b1]),
        (Value::Text(x), Value::Text(y)) => text(x, y),
        (Value::Numeric(x), Value::Numeric(y)) => x.cmp(y),
        (Value::General(x), Value::General(y)) => x.compare(y),
        (Value::Human(x), Value::Human(y)) => x.cmp(y),
        (Value::Month(x), Value::Month(y)) => x.cmp(y),
        (Value::Version(x), Value::Version(y)) => version_cmp(x, y),
        (Value::Random(hx, x), Value::Random(hy, y)) => hx.cmp(hy).then_with(|| text(x, y)),
        // Every record's key k has the same kind.
        _ => Ordering::Equal,
    }
}

/// Apply -d (keep blanks and alphanumerics) or else -i (keep printable),
/// then -f (fold to uppercase). As in sort, -d wins over -i: with both, a
/// tab is kept. In a UTF-8 locale valid text is handled by character;
/// otherwise byte by byte as in the C locale.
pub fn transform(raw: &[u8], order: &Order, utf8: bool) -> Vec<u8> {
    if utf8 && let Ok(text) = std::str::from_utf8(raw) {
        return transform_chars(text, order).into_bytes();
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
    raw.iter()
        .filter(|&&b| keep(b))
        .map(|&b| {
            if order.fold {
                b.to_ascii_uppercase()
            } else {
                b
            }
        })
        .collect()
}

fn transform_chars(text: &str, order: &Order) -> String {
    let keep = |c: char| {
        if order.dictionary {
            c == ' ' || c == '\t' || c.is_alphanumeric()
        } else if order.nonprinting {
            !c.is_control()
        } else {
            true
        }
    };
    let mut out = String::with_capacity(text.len());
    for c in text.chars().filter(|&c| keep(c)) {
        let mut upper = c.to_uppercase();
        match (order.fold, upper.next(), upper.next()) {
            (true, Some(u), None) => out.push(u),
            _ => out.push(c),
        }
    }
    out
}

// --- -n --------------------------------------------------------------------

/// An exact decimal: sign, integer digits without leading zeros, fraction
/// digits without trailing zeros. Zero is never negative.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Decimal {
    negative: bool,
    int: Vec<u8>,
    frac: Vec<u8>,
}

impl Decimal {
    /// Parse `[blanks][-]digits[.digits]`, with the locale's decimal point
    /// and thousands separators (each followed by a digit or the decimal
    /// point) in the integer part; returns the value and the index just
    /// after it. Anything else is zero.
    pub fn parse(s: &[u8], fmt: &NumberFormat) -> (Decimal, usize) {
        let mut i = skip_blanks(s, 0);
        let negative = s.get(i) == Some(&b'-');
        if negative {
            i += 1;
        }
        let mut int = Vec::new();
        loop {
            match s.get(i) {
                Some(&d) if d.is_ascii_digit() => int.push(d),
                Some(&t)
                    if Some(t) == fmt.thousands
                        && s.get(i + 1)
                            .is_some_and(|&n| n.is_ascii_digit() || n == fmt.decimal) => {}
                _ => break,
            }
            i += 1;
        }
        let mut frac: &[u8] = &[];
        if s.get(i) == Some(&fmt.decimal) {
            let f_start = i + 1;
            let mut j = f_start;
            while j < s.len() && s[j].is_ascii_digit() {
                j += 1;
            }
            frac = &s[f_start..j];
            if !int.is_empty() || !frac.is_empty() {
                i = j;
            }
        }
        let leading = int.iter().take_while(|&&d| d == b'0').count();
        int.drain(..leading);
        let frac_len = frac.iter().rposition(|&d| d != b'0').map_or(0, |p| p + 1);
        let frac = frac[..frac_len].to_vec();
        let negative = negative && !(int.is_empty() && frac.is_empty());
        (
            Decimal {
                negative,
                int,
                frac,
            },
            i,
        )
    }

    fn is_zero(&self) -> bool {
        self.int.is_empty() && self.frac.is_empty()
    }

    fn cmp_magnitude(&self, other: &Decimal) -> Ordering {
        self.int
            .len()
            .cmp(&other.int.len())
            .then_with(|| self.int.cmp(&other.int))
            .then_with(|| self.frac.cmp(&other.frac))
    }
}

impl Ord for Decimal {
    fn cmp(&self, other: &Decimal) -> Ordering {
        match (self.negative, other.negative) {
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (false, false) => self.cmp_magnitude(other),
            (true, true) => other.cmp_magnitude(self),
        }
    }
}

impl PartialOrd for Decimal {
    fn partial_cmp(&self, other: &Decimal) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// --- -h --------------------------------------------------------------------

/// Sign, then SI suffix, then value (suffix and value inverted for negatives).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Human {
    sign: i8,
    suffix: u8,
    magnitude: Decimal,
}

impl Human {
    pub fn parse(s: &[u8], fmt: &NumberFormat) -> Human {
        let (mut value, end) = Decimal::parse(s, fmt);
        let suffix = match s.get(end) {
            Some(b'k') => 1,
            Some(&c) => b"KMGTPEZY"
                .iter()
                .position(|&x| x == c)
                .map_or(0, |p| p as u8 + 1),
            None => 0,
        };
        let sign = if value.is_zero() {
            0
        } else if value.negative {
            -1
        } else {
            1
        };
        value.negative = false;
        Human {
            sign,
            suffix,
            magnitude: value,
        }
    }
}

impl Ord for Human {
    fn cmp(&self, other: &Human) -> Ordering {
        self.sign.cmp(&other.sign).then_with(|| {
            if self.sign == 0 {
                return Ordering::Equal;
            }
            let ord = self
                .suffix
                .cmp(&other.suffix)
                .then_with(|| self.magnitude.cmp(&other.magnitude));
            if self.sign < 0 { ord.reverse() } else { ord }
        })
    }
}

impl PartialOrd for Human {
    fn partial_cmp(&self, other: &Human) -> Option<Ordering> {
        Some(self.cmp(other))
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

    pub fn compare(&self, other: &General) -> Ordering {
        match (self, other) {
            (General::Number(x), General::Number(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
            _ => self.rank().cmp(&other.rank()),
        }
    }

    /// Parse the longest strtod prefix after leading whitespace, with
    /// `decimal` as the decimal point.
    pub fn parse(s: &[u8], decimal: u8) -> General {
        let mut i = 0;
        while i < s.len() && s[i].is_ascii_whitespace() {
            i += 1;
        }
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
            let text: String = rest[..len]
                .iter()
                .map(|&b| if b == decimal { '.' } else { b as char })
                .collect();
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
fn decimal_prefix(s: &[u8], decimal: u8) -> usize {
    let digits = |from: usize| s[from..].iter().take_while(|b| b.is_ascii_digit()).count();
    let int = digits(0);
    let mut i = int;
    let mut frac = 0;
    if s.get(i) == Some(&decimal) {
        frac = digits(i + 1);
        if int + frac > 0 {
            i += 1 + frac;
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
fn parse_hex(s: &[u8], decimal: u8) -> Option<f64> {
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
    if body.get(i) == Some(&decimal) {
        let mut scale = 1.0 / 16.0;
        i += 1;
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

/// 1..=12 for a month abbreviation after leading blanks, 0 otherwise.
pub fn month(s: &[u8]) -> u8 {
    const MONTHS: [&[u8]; 12] = [
        b"JAN", b"FEB", b"MAR", b"APR", b"MAY", b"JUN", b"JUL", b"AUG", b"SEP", b"OCT", b"NOV",
        b"DEC",
    ];
    let s = &s[skip_blanks(s, 0)..];
    if s.len() < 3 {
        return 0;
    }
    MONTHS
        .iter()
        .position(|m| s[..3].eq_ignore_ascii_case(m))
        .map_or(0, |p| p as u8 + 1)
}

// --- -V --------------------------------------------------------------------

/// GNU filevercmp: hidden files first, suffixes like `.tar.gz` compared last,
/// digit runs compared numerically, `~` before everything.
pub fn version_cmp(a: &[u8], b: &[u8]) -> Ordering {
    let simple = a.cmp(b);
    if simple == Ordering::Equal {
        return simple;
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
    let a_suffix = suffix_start(a);
    let b_suffix = suffix_start(b);
    let (mut a_len, mut b_len) = (a_suffix.unwrap_or(a.len()), b_suffix.unwrap_or(b.len()));
    if (a_suffix.is_some() || b_suffix.is_some()) && a[..a_len] == b[..b_len] {
        (a_len, b_len) = (a.len(), b.len());
    }
    match verrevcmp(&a[..a_len], &b[..b_len]) {
        Ordering::Equal => simple,
        ord => ord,
    }
}

/// Start of the trailing `(\.[A-Za-z~][A-Za-z0-9~]*)*` suffix, if any.
fn suffix_start(s: &[u8]) -> Option<usize> {
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
    found
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

    const C: NumberFormat = NumberFormat {
        decimal: b'.',
        thousands: None,
    };
    const US: NumberFormat = NumberFormat {
        decimal: b'.',
        thousands: Some(b','),
    };
    const DE: NumberFormat = NumberFormat {
        decimal: b',',
        thousands: Some(b'.'),
    };

    fn dec(s: &str) -> Decimal {
        Decimal::parse(s.as_bytes(), &C).0
    }

    #[test]
    fn decimals() {
        assert_eq!(dec("-0"), dec("0"));
        assert_eq!(dec("abc"), dec(""));
        assert_eq!(dec("007.50"), dec("7.5"));
        assert_eq!(dec("-"), dec("0"));
        assert!(dec("-2") < dec("-1.5"));
        assert!(dec(".5") < dec("1"));
        assert!(dec("10") > dec("9.99"));
        assert_eq!(Decimal::parse(b" 12.5x", &C).1, 5);
        assert_eq!(Decimal::parse(b"1.x", &C).1, 2);
        assert_eq!(Decimal::parse(b".x", &C).1, 0);
    }

    #[test]
    fn decimals_with_locale_separators() {
        let us = |s: &str| Decimal::parse(s.as_bytes(), &US).0;
        assert_eq!(us("1,000"), dec("1000"));
        assert_eq!(us(",5"), dec("5"));
        assert_eq!(us("1,,0"), dec("1"));
        assert_eq!(us(",.9"), dec("0.9"));
        assert_eq!(us("1,.5"), dec("1.5"));
        assert_eq!(us("1,"), dec("1"));
        assert_eq!(us("-1,000.5"), dec("-1000.5"));
        assert_eq!(us("1.5,0"), dec("1.5"), "no separators in the fraction");
        let de = |s: &str| Decimal::parse(s.as_bytes(), &DE).0;
        assert_eq!(de("1.234,5"), dec("1234.5"));
        assert_eq!(de("1,5"), dec("1.5"));
    }

    #[test]
    fn human() {
        let h = |s: &str| Human::parse(s.as_bytes(), &C);
        assert!(h("2") < h("1K"));
        assert!(h("1K") == h("1k"));
        assert!(h("999K") < h("1M"));
        assert!(h("-1M") < h("-1K"));
        assert!(h("-2K") < h("-1K"));
        assert!(h("0K") == h("K"));
        assert!(h("1Y") > h("1Z"));
        assert!(Human::parse(b"1,0K", &US) > h("2K"));
    }

    #[test]
    fn general() {
        let g = |s: &str| General::parse(s.as_bytes(), b'.');
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
        assert_eq!(g("-0").compare(&g("0")), Ordering::Equal);
        assert_eq!(g("nan").compare(&g("abc")), Ordering::Greater);
        assert_eq!(General::parse(b"1,5e1", b','), General::Number(15.0));
        assert_eq!(General::parse(b"1.5", b','), General::Number(1.0));
        assert_eq!(General::parse(b"0x1,8", b','), General::Number(1.5));
    }

    #[test]
    fn months() {
        assert_eq!(month(b" jan"), 1);
        assert_eq!(month(b"December"), 12);
        assert_eq!(month(b"Ja"), 0);
        assert_eq!(month(b"xyz"), 0);
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
        assert_eq!(v("f.tar.gz", "f.tar.gz~"), Ordering::Greater);
        assert_eq!(v("f-1.tar.gz", "f.tar.gz"), Ordering::Greater);
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
        assert_eq!(
            transform("é\u{1}\tß".as_bytes(), &order, true),
            "éß".as_bytes()
        );
        let fold = Order {
            fold: true,
            dictionary: true,
            ..Order::default()
        };
        assert_eq!(
            transform("é-ß x".as_bytes(), &fold, true),
            "Éß X".as_bytes()
        );
        assert_eq!(
            transform(b"\xffa", &fold, true),
            b"A",
            "invalid UTF-8 goes by bytes"
        );
    }
}
