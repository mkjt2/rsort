//! The sort engine: records with precomputed keys, the comparator, and the
//! sort, unique, merge and check operations built on it.

pub mod collate;
pub mod key;
pub mod value;

use collate::{Collator, NumberFormat};
use key::KeyDef;
use std::cmp::Ordering;
use value::Value;

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

/// One input line: its bytes, a u64 summary of its first key that decides
/// most comparisons, and where its key values are in [`Keys`].
#[derive(Debug, Clone, Copy)]
pub struct Record<'a> {
    pub line: &'a [u8],
    prefix: u64,
    id: usize,
}

/// Key values for every record, in one flat list (`keys.len()` per record),
/// and the arena their derived bytes live in.
#[derive(Debug, Default)]
pub struct Keys {
    values: Vec<Value>,
    arena: Vec<u8>,
    /// Record prefixes are comparable (see [`Value::prefix`]).
    prefixes: bool,
    /// The only key is the whole line as plain text: equal keys mean equal
    /// lines, so there is no last-resort comparison.
    plain: bool,
    /// `plain` in byte order: compare lines directly, store no values.
    direct: bool,
}

impl Sorter {
    /// Empty key storage for this sorter's records.
    pub fn keys(&self) -> Keys {
        let plain = matches!(self.keys.as_slice(), [k] if k.is_plain_whole_line());
        Keys {
            prefixes: true,
            plain,
            direct: plain && self.collator == Collator::Bytes,
            ..Keys::default()
        }
    }

    /// Compute a line's keys into `keys`.
    pub fn record<'a>(&self, keys: &mut Keys, line: &'a [u8]) -> Record<'a> {
        if keys.direct {
            let p = value::prefix8(line);
            return Record {
                line,
                prefix: if self.keys[0].order.reverse { !p } else { p },
                id: 0,
            };
        }
        let cx = value::Context {
            seed: self.seed,
            utf8: self.collator.is_utf8(),
            numbers: self.numbers,
            collator: self.collator,
        };
        let id = keys.values.len();
        let mut prefix = 0;
        for (i, k) in self.keys.iter().enumerate() {
            let (begin, end) = k.range(line, self.separator, cx.utf8);
            let v = value::value(line, begin, end, &k.order, &cx, &mut keys.arena);
            if i == 0 {
                match v.prefix(line, &keys.arena, self.collator) {
                    Some(p) => prefix = if k.order.reverse { !p } else { p },
                    None => keys.prefixes = false,
                }
            }
            keys.values.push(v);
        }
        Record { line, prefix, id }
    }

    /// Compare by keys only (what `-u` uses to decide equality).
    pub fn compare_keys(&self, keys: &Keys, a: &Record, b: &Record) -> Ordering {
        if keys.prefixes {
            let ord = a.prefix.cmp(&b.prefix);
            if ord != Ordering::Equal {
                return ord;
            }
        }
        if keys.direct {
            let ord = a.line.cmp(b.line);
            return if self.keys[0].order.reverse {
                ord.reverse()
            } else {
                ord
            };
        }
        let n = self.keys.len();
        let values_a = &keys.values[a.id..a.id + n];
        let values_b = &keys.values[b.id..b.id + n];
        for ((key, va), vb) in self.keys.iter().zip(values_a).zip(values_b) {
            let ord = value::compare(va, a.line, vb, b.line, &keys.arena, self.collator);
            if ord != Ordering::Equal {
                return if key.order.reverse {
                    ord.reverse()
                } else {
                    ord
                };
            }
        }
        Ordering::Equal
    }

    fn last_resort(&self, a: &Record, b: &Record) -> Ordering {
        let ord = self.collator.compare(a.line, b.line);
        if self.reverse { ord.reverse() } else { ord }
    }

    /// Full comparison: keys, then the whole-line last resort unless `-s`/`-u`.
    pub fn compare(&self, keys: &Keys, a: &Record, b: &Record) -> Ordering {
        let ord = self.compare_keys(keys, a, b);
        if ord != Ordering::Equal || self.stable || self.unique || keys.plain {
            return ord;
        }
        self.last_resort(a, b)
    }

    /// Sort, then apply `-u` (keeping the first of equal records).
    pub fn sort(&self, keys: &Keys, records: &mut Vec<Record>) {
        let cmp = |a: &Record, b: &Record| self.compare(keys, a, b);
        if self.unique && keys.plain {
            // Equal whole-line keys are equal lines: drop repeats first.
            let mut seen = std::collections::HashSet::with_capacity(records.len());
            records.retain(|r| seen.insert(r.line));
            records.sort_unstable_by(cmp);
        } else if self.stable || self.unique {
            records.sort_by(cmp);
            if self.unique {
                records.dedup_by(|later, kept| {
                    self.compare_keys(keys, kept, later) == Ordering::Equal
                });
            }
        } else {
            // Without -s/-u every tie is broken down to identical lines,
            // so stability is unobservable.
            records.sort_unstable_by(cmp);
        }
    }

    /// Merge already-sorted inputs; on ties the earlier input wins.
    pub fn merge<'a>(&self, keys: &Keys, inputs: Vec<Vec<Record<'a>>>) -> Vec<Record<'a>> {
        let total = inputs.iter().map(Vec::len).sum();
        let mut out: Vec<Record> = Vec::with_capacity(total);
        let mut heads: Vec<_> = inputs
            .into_iter()
            .map(|v| v.into_iter().peekable())
            .collect();
        loop {
            // The earliest input whose head is smallest.
            let mut best: Option<(usize, &Record)> = None;
            for (i, head) in heads.iter_mut().enumerate() {
                let Some(candidate) = head.peek() else {
                    continue;
                };
                if best.is_none_or(|(_, cur)| self.compare(keys, candidate, cur) == Ordering::Less)
                {
                    best = Some((i, candidate));
                }
            }
            let Some(next) = best.map(|(i, _)| i).and_then(|i| heads[i].next()) else {
                break;
            };
            let duplicate = self.unique
                && out
                    .last()
                    .is_some_and(|prev| self.compare_keys(keys, prev, &next) == Ordering::Equal);
            if !duplicate {
                out.push(next);
            }
        }
        out
    }

    /// Index of the first record out of order (or a duplicate key with `-u`).
    pub fn first_disorder(&self, keys: &Keys, records: &[Record]) -> Option<usize> {
        (1..records.len()).find(|&i| {
            let ord = self.compare(keys, &records[i - 1], &records[i]);
            ord == Ordering::Greater || (self.unique && ord == Ordering::Equal)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use key::Order;

    fn whole_line(reverse: bool) -> Sorter {
        let order = Order {
            reverse,
            ..Order::default()
        };
        Sorter {
            keys: vec![KeyDef::whole_line(order, false)],
            reverse,
            ..Sorter::default()
        }
    }

    fn records<'a>(s: &Sorter, keys: &mut Keys, lines: &[&'a str]) -> Vec<Record<'a>> {
        lines.iter().map(|l| s.record(keys, l.as_bytes())).collect()
    }

    fn lines(records: &[Record]) -> Vec<String> {
        records
            .iter()
            .map(|r| String::from_utf8_lossy(r.line).into_owned())
            .collect()
    }

    #[test]
    fn sorts_bytewise_and_reverses() {
        for (reverse, want) in [(false, ["B", "a", "b"]), (true, ["b", "a", "B"])] {
            let s = whole_line(reverse);
            let mut keys = s.keys();
            let mut r = records(&s, &mut keys, &["b", "B", "a"]);
            s.sort(&keys, &mut r);
            assert_eq!(lines(&r), want);
        }
    }

    #[test]
    fn prefixes_agree_with_full_comparisons() {
        // Shared and short prefixes, embedded NULs, and lines longer than 8.
        let input = [
            "abcdefgh1",
            "abcdefgh",
            "abcdefgh0",
            "a",
            "a\0",
            "a\0b",
            "",
            "abcdefgi",
        ];
        let s = whole_line(false);
        let mut keys = s.keys();
        let mut r = records(&s, &mut keys, &input);
        s.sort(&keys, &mut r);
        let mut want: Vec<&str> = input.to_vec();
        want.sort();
        assert_eq!(lines(&r), want);
    }

    #[test]
    fn merge_takes_the_smallest_head_and_prefers_earlier_inputs() {
        let s = whole_line(false);
        let mut keys = s.keys();
        let a = records(&s, &mut keys, &["b", "d"]);
        let b = records(&s, &mut keys, &["a", "b", "c"]);
        assert_eq!(
            lines(&s.merge(&keys, vec![a, b])),
            ["a", "b", "b", "c", "d"]
        );
        let u = Sorter {
            unique: true,
            ..whole_line(false)
        };
        let mut keys = u.keys();
        let a = records(&u, &mut keys, &["b", "d"]);
        let b = records(&u, &mut keys, &["a", "b"]);
        assert_eq!(lines(&u.merge(&keys, vec![a, b])), ["a", "b", "d"]);
    }

    #[test]
    fn first_disorder_finds_descents_and_unique_duplicates() {
        let s = whole_line(false);
        let mut keys = s.keys();
        let ok = records(&s, &mut keys, &["a", "b", "b"]);
        let bad = records(&s, &mut keys, &["a", "c", "b"]);
        assert_eq!(s.first_disorder(&keys, &ok), None);
        assert_eq!(s.first_disorder(&keys, &bad), Some(2));
        let u = Sorter {
            unique: true,
            ..whole_line(false)
        };
        let mut keys = u.keys();
        let dup = records(&u, &mut keys, &["a", "a"]);
        assert_eq!(u.first_disorder(&keys, &dup), Some(1));
    }
}
