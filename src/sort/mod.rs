//! The sort engine: records with precomputed keys, the comparator, and the
//! sort, unique, merge and check operations built on it.

use std::cmp::Ordering;

/// How lines are compared.
#[derive(Debug, Clone, Default)]
pub struct Sorter {
    /// Reverse the whole-line last-resort comparison (global `-r`).
    pub reverse: bool,
    /// `-u`: drop lines whose keys equal the previous line's.
    pub unique: bool,
    /// `-s`: no last-resort comparison.
    pub stable: bool,
}

/// One input line and the keys computed from it.
#[derive(Debug, Clone)]
pub struct Record {
    pub line: Vec<u8>,
}

impl Sorter {
    /// Decorate a line with its sort keys.
    pub fn record(&self, line: Vec<u8>) -> Record {
        Record { line }
    }

    /// Compare by keys only (what `-u` uses to decide equality).
    pub fn compare_keys(&self, a: &Record, b: &Record) -> Ordering {
        let ord = a.line.cmp(&b.line);
        if self.reverse { ord.reverse() } else { ord }
    }

    /// Full comparison: keys, then the whole-line last resort unless `-s`/`-u`.
    pub fn compare(&self, a: &Record, b: &Record) -> Ordering {
        let ord = self.compare_keys(a, b);
        if ord != Ordering::Equal || self.stable || self.unique {
            return ord;
        }
        let last = a.line.cmp(&b.line);
        if self.reverse { last.reverse() } else { last }
    }

    /// Sort in place (stable), then apply `-u`.
    pub fn sort(&self, records: &mut Vec<Record>) {
        records.sort_by(|a, b| self.compare(a, b));
        if self.unique {
            self.dedup(records);
        }
    }

    /// Keep the first of each run of records with equal keys.
    fn dedup(&self, records: &mut Vec<Record>) {
        records.dedup_by(|later, kept| self.compare_keys(kept, later) == Ordering::Equal);
    }

    /// Merge already-sorted inputs; on ties the earlier input wins.
    pub fn merge(&self, inputs: Vec<Vec<Record>>) -> Vec<Record> {
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
                if best.is_none_or(|(_, cur)| self.compare(candidate, cur) == Ordering::Less) {
                    best = Some((i, candidate));
                }
            }
            let Some(next) = best.map(|(i, _)| i).and_then(|i| heads[i].next()) else {
                break;
            };
            let duplicate = self.unique
                && out
                    .last()
                    .is_some_and(|prev| self.compare_keys(prev, &next) == Ordering::Equal);
            if !duplicate {
                out.push(next);
            }
        }
        out
    }

    /// Index of the first record out of order (or a duplicate key with `-u`).
    pub fn first_disorder(&self, records: &[Record]) -> Option<usize> {
        (1..records.len()).find(|&i| {
            let ord = self.compare(&records[i - 1], &records[i]);
            ord == Ordering::Greater || (self.unique && ord == Ordering::Equal)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn records(s: &Sorter, lines: &[&str]) -> Vec<Record> {
        lines
            .iter()
            .map(|l| s.record(l.as_bytes().to_vec()))
            .collect()
    }

    fn lines(records: &[Record]) -> Vec<String> {
        records
            .iter()
            .map(|r| String::from_utf8_lossy(&r.line).into_owned())
            .collect()
    }

    #[test]
    fn sorts_bytewise_and_reverses() {
        let s = Sorter::default();
        let mut r = records(&s, &["b", "B", "a"]);
        s.sort(&mut r);
        assert_eq!(lines(&r), ["B", "a", "b"]);
        let s = Sorter {
            reverse: true,
            ..Sorter::default()
        };
        s.sort(&mut r);
        assert_eq!(lines(&r), ["b", "a", "B"]);
    }

    #[test]
    fn merge_takes_the_smallest_head_and_prefers_earlier_inputs() {
        let s = Sorter::default();
        let merged = s.merge(vec![
            records(&s, &["b", "d"]),
            records(&s, &["a", "b", "c"]),
        ]);
        assert_eq!(lines(&merged), ["a", "b", "b", "c", "d"]);
        let u = Sorter {
            unique: true,
            ..Sorter::default()
        };
        let merged = u.merge(vec![records(&u, &["b", "d"]), records(&u, &["a", "b"])]);
        assert_eq!(lines(&merged), ["a", "b", "d"]);
    }

    #[test]
    fn first_disorder_finds_descents_and_unique_duplicates() {
        let s = Sorter::default();
        assert_eq!(s.first_disorder(&records(&s, &["a", "b", "b"])), None);
        assert_eq!(s.first_disorder(&records(&s, &["a", "c", "b"])), Some(2));
        let u = Sorter {
            unique: true,
            ..Sorter::default()
        };
        assert_eq!(u.first_disorder(&records(&u, &["a", "a"])), Some(1));
    }
}
