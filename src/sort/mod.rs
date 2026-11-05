//! The sort engine: records with key summaries, the comparator, and the
//! sort, unique and check operations built on it.

pub mod collate;
pub mod key;
pub mod value;

use collate::{Collator, NumberFormat};
use key::KeyDef;
use std::cell::RefCell;
use std::cmp::Ordering;
use value::{Bits, Summary};

/// How lines are compared.
#[derive(Debug, Clone, Default)]
pub struct Sorter {
    /// `-k` keys in priority order (the whole line when none is given).
    pub keys: Vec<KeyDef>,
    /// `-t`: the field separator, or runs of blanks when `None`.
    pub separator: Option<u8>,
    /// Seed for `-R` hashes.
    pub seed: u64,
    /// Reverse the whole-line last-resort comparison (global `-r`).
    pub reverse: bool,
    /// `-u`: drop lines whose keys equal the previous line's.
    pub unique: bool,
    /// `-s`: no last-resort comparison.
    pub stable: bool,
    /// How text compares (bytes or the locale).
    pub collator: Collator,
    /// The locale's decimal point and thousands separator.
    pub numbers: NumberFormat,
}

/// Keys beyond this many have no summary and are always compared in full.
const SUMMARIZED: usize = 16;

/// One input line with a summary of each key (see [`Summary`]): the first
/// key's here, the others' in [`Keys`]. 40 bytes plus the line itself.
#[derive(Debug, Clone, Copy)]
pub struct Record<'a> {
    pub line: &'a [u8],
    hi: u64,
    lo: u64,
    /// Index of this record's further summaries in [`Keys`].
    id: u32,
    /// Two bits per key: 0 none, 1 ordered, 3 ordered and exact, 2 a `-V`
    /// suffix (stored as start + 1, 0 for none).
    flags: u32,
}

const NONE: u32 = 0;
const ORDERED: u32 = 1;
const VERSION: u32 = 2;
const EXACT: u32 = 3;

/// Summaries of keys after the first, `keys - 1` per record.
#[derive(Debug, Default)]
pub struct Keys {
    extra: Vec<[u64; 2]>,
    /// The only key is the whole line as plain text: equal keys mean equal
    /// lines, so there is no last-resort comparison.
    plain: bool,
}

thread_local! {
    /// Buffers for keys rewritten by -d/-f/-i, one per side of a comparison.
    static REWRITTEN: [RefCell<Vec<u8>>; 2] = const { [RefCell::new(Vec::new()), RefCell::new(Vec::new())] };
}

fn split(bits: Bits) -> [u64; 2] {
    [(bits >> 64) as u64, bits as u64]
}

fn join([hi, lo]: [u64; 2]) -> Bits {
    (Bits::from(hi) << 64) | Bits::from(lo)
}

impl Sorter {
    /// Storage for summaries of `lines` records.
    pub fn keys(&self, lines: usize) -> Keys {
        Keys {
            extra: Vec::with_capacity(lines.saturating_mul(self.further())),
            plain: self.plain(),
        }
    }

    /// Summarized keys after the first.
    fn further(&self) -> usize {
        self.keys.len().clamp(1, SUMMARIZED) - 1
    }

    fn plain(&self) -> bool {
        matches!(self.keys.as_slice(), [k] if k.is_plain_whole_line())
    }

    fn context(&self) -> value::Context {
        value::Context {
            seed: self.seed,
            utf8: self.collator.is_utf8(),
            numbers: self.numbers,
            collator: self.collator,
        }
    }

    /// Run `f` on key `k` of `line`, rewritten by -d/-f/-i if it asks;
    /// `slot` picks one of two buffers so two keys can be held at once.
    fn with_key<T>(&self, k: &KeyDef, line: &[u8], slot: usize, f: impl FnOnce(&[u8]) -> T) -> T {
        let utf8 = self.collator.is_utf8();
        let (begin, end) = k.range(line, self.separator, utf8);
        let raw = &line[begin..end.max(begin)];
        let o = &k.order;
        if !(o.fold || o.dictionary || o.nonprinting) {
            return f(raw);
        }
        REWRITTEN.with(|bufs| {
            let mut buf = bufs[slot].borrow_mut();
            buf.clear();
            value::transform_into(raw, o, utf8, &mut buf);
            f(&buf)
        })
    }

    /// Summarize a line's keys; `None` past 2^32 records with several keys.
    pub fn record<'a>(&self, keys: &mut Keys, line: &'a [u8]) -> Option<Record<'a>> {
        let cx = self.context();
        let id = match self.further() {
            0 => 0,
            n => u32::try_from(keys.extra.len() / n).ok()?,
        };
        let mut record = Record {
            line,
            hi: 0,
            lo: 0,
            id,
            flags: 0,
        };
        for (i, k) in self.keys.iter().take(SUMMARIZED).enumerate() {
            let summary = self.with_key(k, line, 0, |key| value::summarize(key, k.order.mode, &cx));
            let (flag, bits) = match summary {
                Summary::Ordered { bits, exact } => {
                    let bits = if k.order.reverse { !bits } else { bits };
                    (if exact { EXACT } else { ORDERED }, bits)
                }
                Summary::Version(suffix) => (VERSION, suffix.map_or(0, |s| s as Bits + 1)),
                Summary::None => (NONE, 0),
            };
            record.flags |= flag << (2 * i);
            if i == 0 {
                [record.hi, record.lo] = split(bits);
            } else {
                keys.extra.push(split(bits));
            }
        }
        Some(record)
    }

    fn summary(&self, keys: &Keys, r: &Record, i: usize) -> (u32, Bits) {
        if i >= SUMMARIZED {
            return (NONE, 0);
        }
        let flag = (r.flags >> (2 * i)) & 3;
        let bits = if i == 0 {
            join([r.hi, r.lo])
        } else {
            join(keys.extra[r.id as usize * self.further() + i - 1])
        };
        (flag, bits)
    }

    /// Compare key `i` of two lines in full, `suffixes` being cached `-V`
    /// suffix starts.
    fn compare_key(
        &self,
        i: usize,
        a: &[u8],
        b: &[u8],
        suffixes: Option<(Option<usize>, Option<usize>)>,
    ) -> Ordering {
        let k = &self.keys[i];
        let cx = self.context();
        let ord = self.with_key(k, a, 0, |x| {
            self.with_key(k, b, 1, |y| {
                value::compare(x, y, k.order.mode, &cx, suffixes)
            })
        });
        if k.order.reverse { ord.reverse() } else { ord }
    }

    /// Compare by keys only (what `-u` uses to decide equality).
    pub fn compare_keys(&self, keys: &Keys, a: &Record, b: &Record) -> Ordering {
        for i in 0..self.keys.len() {
            let (fa, sa) = self.summary(keys, a, i);
            let (fb, sb) = self.summary(keys, b, i);
            let ord = if fa & ORDERED != 0 && fb & ORDERED != 0 {
                match sa.cmp(&sb) {
                    Ordering::Equal if fa == EXACT && fb == EXACT => Ordering::Equal,
                    Ordering::Equal => self.compare_key(i, a.line, b.line, None),
                    ord => ord,
                }
            } else if fa == VERSION && fb == VERSION {
                let suffix = |s: Bits| s.checked_sub(1).map(|s| s as usize);
                self.compare_key(i, a.line, b.line, Some((suffix(sa), suffix(sb))))
            } else {
                self.compare_key(i, a.line, b.line, None)
            };
            if ord != Ordering::Equal {
                return ord;
            }
        }
        Ordering::Equal
    }

    /// Compare two lines by keys only, without summaries.
    pub fn compare_line_keys(&self, a: &[u8], b: &[u8]) -> Ordering {
        (0..self.keys.len())
            .map(|i| self.compare_key(i, a, b, None))
            .find(|&o| o != Ordering::Equal)
            .unwrap_or(Ordering::Equal)
    }

    fn last_resort(&self, a: &[u8], b: &[u8]) -> Ordering {
        let ord = self.collator.compare(a, b);
        if self.reverse { ord.reverse() } else { ord }
    }

    /// Without -s/-u, key ties fall back to the whole line, unless the only
    /// key is the plain whole line (equal keys are then equal lines).
    fn needs_last_resort(&self) -> bool {
        !(self.stable || self.unique || self.plain())
    }

    /// Full comparison: keys, then the whole-line last resort unless `-s`/`-u`.
    pub fn compare(&self, keys: &Keys, a: &Record, b: &Record) -> Ordering {
        let ord = self.compare_keys(keys, a, b);
        if ord != Ordering::Equal || keys.plain || self.stable || self.unique {
            return ord;
        }
        self.last_resort(a.line, b.line)
    }

    /// [`Sorter::compare`] for two lines without summaries.
    pub fn compare_lines(&self, a: &[u8], b: &[u8]) -> Ordering {
        let ord = self.compare_line_keys(a, b);
        if ord != Ordering::Equal || !self.needs_last_resort() {
            return ord;
        }
        self.last_resort(a, b)
    }

    /// Sort, then apply `-u` (keeping the first of equal records).
    pub fn sort(&self, keys: &Keys, records: &mut Vec<Record>) {
        let cmp = |a: &Record, b: &Record| self.compare(keys, a, b);
        if self.stable || (self.unique && !keys.plain) {
            records.sort_by(cmp);
        } else {
            // Ties are identical lines (the last resort, or -u on whole
            // lines), so stability is unobservable.
            records.sort_unstable_by(cmp);
        }
        if self.unique {
            records.dedup_by(|later, kept| self.compare_keys(keys, kept, later) == Ordering::Equal);
        }
    }

    /// Whether a line is out of order after `prev` (or a duplicate with `-u`).
    pub fn disorder(&self, prev: &[u8], line: &[u8]) -> bool {
        let ord = self.compare_lines(prev, line);
        ord == Ordering::Greater || (self.unique && ord == Ordering::Equal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use key::{Mode, Order};

    fn sorter(order: Order) -> Sorter {
        Sorter {
            keys: vec![KeyDef::whole_line(order, false)],
            reverse: order.reverse,
            ..Sorter::default()
        }
    }

    fn whole_line(reverse: bool) -> Sorter {
        sorter(Order {
            reverse,
            ..Order::default()
        })
    }

    fn sorted<'a>(s: &Sorter, lines: &[&'a str]) -> Vec<&'a str> {
        let mut keys = s.keys(lines.len());
        let mut records: Vec<Record> = lines
            .iter()
            .map(|l| s.record(&mut keys, l.as_bytes()).unwrap())
            .collect();
        s.sort(&keys, &mut records);
        records
            .iter()
            .map(|r| std::str::from_utf8(r.line).unwrap())
            .collect()
    }

    #[test]
    fn sorts_bytewise_and_reverses() {
        assert_eq!(
            sorted(&whole_line(false), &["b", "B", "a"]),
            ["B", "a", "b"]
        );
        assert_eq!(sorted(&whole_line(true), &["b", "B", "a"]), ["b", "a", "B"]);
    }

    #[test]
    fn summaries_agree_with_full_comparisons() {
        // Shared and short prefixes, embedded NULs, keys around 15 bytes.
        let input = [
            "abcdefghijklmno1",
            "abcdefghijklmno",
            "abcdefghijklmno0",
            "abcdefghijklmn",
            "a",
            "a\0",
            "a\0b",
            "",
            "abcdefghijklmnp",
        ];
        let mut want = input.to_vec();
        want.sort();
        assert_eq!(sorted(&whole_line(false), &input), want);
        want.reverse();
        assert_eq!(sorted(&whole_line(true), &input), want);
    }

    #[test]
    fn number_summaries_order_like_numbers() {
        let n = sorter(Order {
            mode: Mode::Numeric,
            ..Order::default()
        });
        let long = "1234567890123456789012345678901234";
        let longer = "1234567890123456789012345678901235";
        let huge = "9".repeat(300);
        let huger = format!("1{}", "0".repeat(300));
        let input = [
            "-2", "-10", "0", "-0", "3", "1.5", "1.25", "10", long, longer, &huge, &huger, "-.5",
        ];
        assert_eq!(
            sorted(&n, &input),
            [
                "-10", "-2", "-.5", "-0", "0", "1.25", "1.5", "3", "10", long, longer, &huge,
                &huger
            ]
        );
        let h = sorter(Order {
            mode: Mode::Human,
            ..Order::default()
        });
        assert_eq!(
            sorted(&h, &["2K", "-1M", "-2K", "1G", "0", "999", "1.5K"]),
            ["-1M", "-2K", "0", "999", "1.5K", "2K", "1G"]
        );
        let g = sorter(Order {
            mode: Mode::General,
            ..Order::default()
        });
        assert_eq!(
            sorted(
                &g,
                &["1e3", "-inf", "x", "nan", "-0", "0", "-1e-3", "1e400"]
            ),
            ["x", "nan", "-inf", "-1e-3", "-0", "0", "1e3", "1e400"]
        );
    }

    #[test]
    fn unique_keeps_the_first_of_equal_keys() {
        let s = Sorter {
            unique: true,
            ..sorter(Order {
                fold: true,
                ..Order::default()
            })
        };
        assert_eq!(sorted(&s, &["b", "A", "a", "B"]), ["A", "b"]);
    }

    #[test]
    fn disorder_finds_descents_and_unique_duplicates() {
        let s = whole_line(false);
        assert!(!s.disorder(b"a", b"b"));
        assert!(!s.disorder(b"b", b"b"));
        assert!(s.disorder(b"c", b"b"));
        let u = Sorter {
            unique: true,
            ..whole_line(false)
        };
        assert!(u.disorder(b"a", b"a"));
    }
}
