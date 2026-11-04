//! `-k POS1[,POS2]` key definitions: parsing and extracting the key bytes
//! from a line.

/// How a key's bytes are interpreted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Text,
    Numeric,
    General,
    Human,
    Month,
    Version,
    Random,
}

impl Mode {
    /// The option letter for this mode (`-n`, `-g`, ...).
    pub fn letter(self) -> char {
        match self {
            Mode::Text => 't',
            Mode::Numeric => 'n',
            Mode::General => 'g',
            Mode::Human => 'h',
            Mode::Month => 'M',
            Mode::Version => 'V',
            Mode::Random => 'R',
        }
    }

    fn from_letter(c: char) -> Option<Mode> {
        Some(match c {
            'n' => Mode::Numeric,
            'g' => Mode::General,
            'h' => Mode::Human,
            'M' => Mode::Month,
            'V' => Mode::Version,
            'R' => Mode::Random,
            _ => return None,
        })
    }
}

/// Ordering options, global or attached to one key.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Order {
    pub mode: Mode,
    pub reverse: bool,
    pub fold: bool,
    pub dictionary: bool,
    pub nonprinting: bool,
}

impl Order {
    /// Set `mode`, failing if a different mode is already set.
    pub fn set_mode(&mut self, mode: Mode) -> Result<(), String> {
        if self.mode != Mode::Text && self.mode != mode {
            return Err(format!(
                "-{} cannot be used with -{}",
                mode.letter(),
                self.mode.letter()
            ));
        }
        self.mode = mode;
        Ok(())
    }
}

/// A key position: 1-based field and character; `char == 0` in an end
/// position means the end of the field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pos {
    pub field: usize,
    pub char: usize,
    pub blanks: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyDef {
    pub start: Pos,
    pub end: Option<Pos>,
    pub order: Order,
}

impl KeyDef {
    /// The implicit key when no `-k` is given: the whole line.
    pub fn whole_line(order: Order, blanks: bool) -> KeyDef {
        KeyDef {
            start: Pos {
                field: 1,
                char: 1,
                blanks,
            },
            end: None,
            order,
        }
    }

    /// Parse `F[.C][OPTS][,F[.C][OPTS]]`. A key with no option letters takes
    /// the global ordering and blank handling.
    pub fn parse(spec: &str, global: Order, global_blanks: bool) -> Result<KeyDef, String> {
        let (start, end) = match spec.split_once(',') {
            Some((s, e)) => (s, Some(e)),
            None => (spec, None),
        };
        let mut order = Order::default();
        let (mut start, start_opts) = parse_pos(start, true, &mut order)?;
        let mut end = match end {
            Some(e) => Some(parse_pos(e, false, &mut order)?),
            None => None,
        };
        let has_opts = start_opts || end.is_some_and(|(_, opts)| opts);
        if !has_opts {
            order = global;
            start.blanks = global_blanks;
            if let Some((pos, _)) = end.as_mut() {
                pos.blanks = global_blanks;
            }
        }
        Ok(KeyDef {
            start,
            end: end.map(|(pos, _)| pos),
            order,
        })
    }

    /// The bytes of this key in `line`, split by `sep` (or runs of blanks).
    pub fn extract<'a>(&self, line: &'a [u8], sep: Option<u8>, utf8: bool) -> &'a [u8] {
        let (begin, end) = self.range(line, sep, utf8);
        &line[begin..end.max(begin)]
    }

    /// Byte range of the key; `end < begin` means an empty key. Character
    /// offsets count UTF-8 characters when `utf8`, bytes otherwise.
    pub fn range(&self, line: &[u8], sep: Option<u8>, utf8: bool) -> (usize, usize) {
        let mut begin = field_start(line, self.start.field, sep);
        if self.start.blanks {
            begin = skip_blanks(line, begin);
        }
        let begin = advance(line, begin, self.start.char - 1, utf8);
        let end = match self.end {
            None => line.len(),
            Some(pos) if pos.char == 0 => field_end(line, field_start(line, pos.field, sep), sep),
            Some(pos) => {
                let mut e = field_start(line, pos.field, sep);
                if pos.blanks {
                    e = skip_blanks(line, e);
                }
                advance(line, e, pos.char, utf8)
            }
        };
        (begin, end)
    }
}

/// Parse one position; returns it and whether it carried option letters.
fn parse_pos(text: &str, is_start: bool, order: &mut Order) -> Result<(Pos, bool), String> {
    let invalid = || "invalid key".to_string();
    let digits_end = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let field = number(&text[..digits_end]).ok_or_else(invalid)??;
    let mut rest = &text[digits_end..];
    let mut char = if is_start { 1 } else { 0 };
    if let Some(after_dot) = rest.strip_prefix('.') {
        let n = after_dot
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after_dot.len());
        char = number(&after_dot[..n]).ok_or_else(invalid)??;
        rest = &after_dot[n..];
    }
    if field == 0 {
        return Err("fields are numbered from 1".into());
    }
    if is_start && char == 0 {
        return Err("characters are numbered from 1".into());
    }
    let mut blanks = false;
    for c in rest.chars() {
        match c {
            'b' => blanks = true,
            'd' => order.dictionary = true,
            'f' => order.fold = true,
            'i' => order.nonprinting = true,
            'r' => order.reverse = true,
            _ => order.set_mode(Mode::from_letter(c).ok_or_else(invalid)?)?,
        }
    }
    let pos = Pos {
        field,
        char,
        blanks,
    };
    Ok((pos, !rest.is_empty()))
}

/// `None` for no digits; `Some(Err)` when too large.
fn number(digits: &str) -> Option<Result<usize, String>> {
    if digits.is_empty() {
        return None;
    }
    Some(digits.parse().map_err(|_| "number too large".to_string()))
}

/// The index `n` characters (or bytes) after `i`, at most the line length.
fn advance(line: &[u8], mut i: usize, n: usize, utf8: bool) -> usize {
    if !utf8 {
        return i.saturating_add(n).min(line.len());
    }
    for _ in 0..n {
        if i >= line.len() {
            break;
        }
        i += 1;
        while i < line.len() && line[i] & 0xC0 == 0x80 {
            i += 1;
        }
    }
    i
}

pub fn is_blank(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

pub fn skip_blanks(line: &[u8], mut i: usize) -> usize {
    while i < line.len() && is_blank(line[i]) {
        i += 1;
    }
    i
}

fn skip_non_blanks(line: &[u8], mut i: usize) -> usize {
    while i < line.len() && !is_blank(line[i]) {
        i += 1;
    }
    i
}

/// Index where 1-based `field` starts (the line length if there is none).
/// Without a separator a field includes its leading blanks.
fn field_start(line: &[u8], field: usize, sep: Option<u8>) -> usize {
    let mut i = 0;
    for _ in 1..field {
        if i >= line.len() {
            return line.len();
        }
        i = match sep {
            Some(c) => match line[i..].iter().position(|&b| b == c) {
                Some(p) => i + p + 1,
                None => return line.len(),
            },
            None => skip_non_blanks(line, skip_blanks(line, i)),
        };
    }
    i
}

fn field_end(line: &[u8], start: usize, sep: Option<u8>) -> usize {
    match sep {
        Some(c) => line[start..]
            .iter()
            .position(|&b| b == c)
            .map_or(line.len(), |p| start + p),
        None => skip_non_blanks(line, skip_blanks(line, start)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(spec: &str) -> KeyDef {
        KeyDef::parse(spec, Order::default(), false).unwrap()
    }

    fn get<'a>(spec: &str, line: &'a str, sep: Option<u8>) -> &'a str {
        std::str::from_utf8(key(spec).extract(line.as_bytes(), sep, false)).unwrap()
    }

    #[test]
    fn fields_without_separator_keep_leading_blanks() {
        assert_eq!(get("2,2", "a  bc d", None), "  bc");
        assert_eq!(get("2b,2", "a  bc d", None), "bc");
        assert_eq!(get("2", "a  bc d", None), "  bc d");
        assert_eq!(get("3,3", "a b", None), "");
        assert_eq!(get("1,1", "  a b", None), "  a");
    }

    #[test]
    fn fields_with_separator_split_on_every_char() {
        assert_eq!(get("2,2", "a::c", Some(b':')), "");
        assert_eq!(get("3,3", "a::c", Some(b':')), "c");
        assert_eq!(get("2", "a:b:c", Some(b':')), "b:c");
        assert_eq!(get("4", "a:b", Some(b':')), "");
    }

    #[test]
    fn character_offsets() {
        assert_eq!(get("1.2,1.3", "abcd", None), "bc");
        assert_eq!(get("1.3,1.2", "abcd", None), "");
        assert_eq!(get("1.9", "abcd", None), "");
        assert_eq!(get("2.2,2.3", "x   zab", None), "  ");
        assert_eq!(get("2.2b,2.3", "x   zab", None), "");
        assert_eq!(get("2.2b,2.3b", "x   zab", None), "ab");
        assert_eq!(get("1,1.0", "ab cd", None), "ab");
    }

    #[test]
    fn utf8_offsets_count_characters() {
        let k = key("1.2,1.3");
        assert_eq!(k.extract("éxyz".as_bytes(), None, true), b"xy");
        assert_eq!(k.extract("éxyz".as_bytes(), None, false), b"\xa9x");
        assert_eq!(k.extract("é".as_bytes(), None, true), b"");
    }

    #[test]
    fn options_replace_the_global_ordering() {
        let global = Order {
            mode: Mode::Numeric,
            reverse: true,
            ..Order::default()
        };
        let k = KeyDef::parse("1,1", global, true).unwrap();
        assert_eq!(k.order, global);
        assert!(k.start.blanks && k.end.unwrap().blanks);
        let k = KeyDef::parse("1b,1", global, false).unwrap();
        assert_eq!(k.order, Order::default());
        assert!(k.start.blanks && !k.end.unwrap().blanks);
        let k = KeyDef::parse("1,1dfirM", global, false).unwrap();
        assert!(k.order.dictionary && k.order.fold && k.order.nonprinting && k.order.reverse);
        assert_eq!(k.order.mode, Mode::Month);
    }

    #[test]
    fn invalid_specs() {
        for (spec, message) in [
            ("0", "fields are numbered from 1"),
            ("1.0", "characters are numbered from 1"),
            ("1,0", "fields are numbered from 1"),
            ("1.", "invalid key"),
            ("1,1z", "invalid key"),
            ("1ng", "-g cannot be used with -n"),
            ("1n,1V", "-V cannot be used with -n"),
            ("1.99999999999999999999", "number too large"),
        ] {
            assert_eq!(
                KeyDef::parse(spec, Order::default(), false),
                Err(message.to_string()),
                "{spec}"
            );
        }
    }

    #[test]
    fn mode_letters() {
        for mode in [
            Mode::Numeric,
            Mode::General,
            Mode::Human,
            Mode::Month,
            Mode::Version,
            Mode::Random,
        ] {
            assert_eq!(Mode::from_letter(mode.letter()), Some(mode));
        }
        assert_eq!(Mode::Text.letter(), 't');
    }
}
