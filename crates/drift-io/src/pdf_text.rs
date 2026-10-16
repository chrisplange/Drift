//! Reading the text of a PDF: what a font's character codes stand for (its `ToUnicode` map,
//! else its encoding), and how far each one advances, so that the runs a page shows can be
//! put back together as lines of text.

use std::collections::HashMap;

use lopdf::{Dictionary, Document, Object};

/// What is known of a font: how its codes read and how wide they are.
#[derive(Clone, Debug, Default)]
pub(crate) struct Font {
    /// Codes are two bytes (composite fonts), else one.
    pub two_byte: bool,
    /// Code → text, from the font's `ToUnicode` map.
    pub unicode: HashMap<u32, String>,
    /// Code → text from a simple font's encoding, when it has no map.
    pub encoding: Option<Box<[Option<char>; 256]>>,
    /// Code → advance, in thousandths of the font size.
    pub widths: HashMap<u32, f64>,
    pub default_width: f64,
    /// A composite font without a map: its codes are glyph numbers, which can't be read.
    pub unreadable: bool,
}

impl Font {
    pub fn read(doc: &Document, dict: &Dictionary) -> Font {
        let deref = |o: &Object| doc.dereference(o).ok().map(|(_, o)| o.clone());
        let subtype = dict.get(b"Subtype").and_then(Object::as_name).ok();
        let two_byte = subtype == Some(b"Type0".as_slice());
        let mut font = Font {
            two_byte,
            default_width: if two_byte { 1000.0 } else { 500.0 },
            ..Default::default()
        };
        if let Some(Object::Stream(s)) = dict.get(b"ToUnicode").ok().and_then(deref) {
            let data = s
                .decompressed_content()
                .unwrap_or_else(|_| s.content.clone());
            font.unicode = parse_cmap(&String::from_utf8_lossy(&data));
        }
        if two_byte {
            let descendant = dict
                .get(b"DescendantFonts")
                .ok()
                .and_then(deref)
                .and_then(|a| a.as_array().ok().and_then(|a| a.first().cloned()))
                .and_then(|d| deref(&d))
                .and_then(|d| d.as_dict().ok().cloned());
            if let Some(d) = descendant {
                if let Some(dw) = d.get(b"DW").ok().and_then(num) {
                    font.default_width = dw;
                }
                if let Some(Object::Array(w)) = d.get(b"W").ok().and_then(deref) {
                    cid_widths(doc, &w, &mut font.widths);
                }
            }
            font.unreadable = font.unicode.is_empty();
        } else {
            let first = dict.get(b"FirstChar").ok().and_then(num).unwrap_or(0.0) as u32;
            if let Some(Object::Array(w)) = dict.get(b"Widths").ok().and_then(deref) {
                for (i, o) in w.iter().enumerate() {
                    if let Some(v) = deref(o).as_ref().and_then(num) {
                        font.widths.insert(first + i as u32, v);
                    }
                }
            }
            if font.unicode.is_empty() {
                font.encoding = Some(Box::new(encoding(doc, dict)));
            }
        }
        font
    }

    /// The codes of a string shown in this font.
    pub fn codes(&self, bytes: &[u8]) -> Vec<u32> {
        if self.two_byte {
            bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u32::from(u16::from_be_bytes(*c)))
                .collect()
        } else {
            bytes.iter().map(|b| u32::from(*b)).collect()
        }
    }

    /// What a code reads as (nothing for one that can't be read).
    pub fn text(&self, code: u32) -> String {
        if let Some(t) = self.unicode.get(&code) {
            return t.clone();
        }
        if let Some(enc) = &self.encoding
            && let Some(Some(c)) = enc.get(code as usize)
        {
            return c.to_string();
        }
        String::new()
    }

    pub fn width(&self, code: u32) -> f64 {
        self.widths
            .get(&code)
            .copied()
            .filter(|w| *w > 0.0)
            .unwrap_or(self.default_width)
    }
}

fn num(o: &Object) -> Option<f64> {
    match o {
        Object::Integer(i) => Some(*i as f64),
        Object::Real(r) => Some(f64::from(*r)),
        _ => None,
    }
}

/// A composite font's `W` array: `c [w1 w2 …]` and `c_first c_last w`.
fn cid_widths(doc: &Document, w: &[Object], out: &mut HashMap<u32, f64>) {
    let deref = |o: &Object| doc.dereference(o).ok().map(|(_, o)| o.clone());
    let mut i = 0;
    while i < w.len() {
        let Some(first) = deref(&w[i]).as_ref().and_then(num) else {
            break;
        };
        match w.get(i + 1).and_then(deref) {
            Some(Object::Array(list)) => {
                for (k, o) in list.iter().enumerate() {
                    if let Some(v) = deref(o).as_ref().and_then(num) {
                        out.insert(first as u32 + k as u32, v);
                    }
                }
                i += 2;
            }
            Some(o) => {
                let (Some(last), Some(v)) =
                    (num(&o), w.get(i + 2).and_then(deref).as_ref().and_then(num))
                else {
                    break;
                };
                for c in first as u32..=(last as u32).min(first as u32 + 65_535) {
                    out.insert(c, v);
                }
                i += 3;
            }
            None => break,
        }
    }
}

/// A `ToUnicode` CMap's `bfchar` and `bfrange` entries.
fn parse_cmap(text: &str) -> HashMap<u32, String> {
    let mut out = HashMap::new();
    // Hex strings, arrays and keywords, in order.
    let mut tokens: Vec<String> = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '<' => {
                let mut hex = String::from("<");
                for d in chars.by_ref() {
                    if d == '>' {
                        break;
                    }
                    if !d.is_whitespace() {
                        hex.push(d);
                    }
                }
                tokens.push(hex);
            }
            '[' | ']' => tokens.push(c.to_string()),
            c if c.is_alphanumeric() => {
                let mut word = c.to_string();
                while let Some(d) = chars.peek().copied().filter(|d| d.is_alphanumeric()) {
                    word.push(d);
                    chars.next();
                }
                tokens.push(word);
            }
            _ => {}
        }
    }
    let hex = |t: &str| -> Option<Vec<u8>> {
        let h = t.strip_prefix('<')?;
        (0..h.len() / 2)
            .map(|i| u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).ok())
            .collect()
    };
    let code = |t: &str| -> Option<u32> {
        hex(t).map(|b| b.iter().fold(0u32, |a, x| (a << 8) | u32::from(*x)))
    };
    let utf16 = |b: &[u8]| -> String {
        let units: Vec<u16> = b
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_be_bytes(*c))
            .collect();
        String::from_utf16_lossy(&units)
    };
    let mut i = 0;
    let mut section = "";
    while i < tokens.len() {
        let t = tokens[i].as_str();
        match t {
            "beginbfchar" | "beginbfrange" => {
                section = if t == "beginbfchar" { "char" } else { "range" };
                i += 1;
                continue;
            }
            "endbfchar" | "endbfrange" => {
                section = "";
                i += 1;
                continue;
            }
            _ => {}
        }
        match section {
            "char" if i + 1 < tokens.len() => {
                if let (Some(c), Some(dst)) = (code(t), hex(&tokens[i + 1])) {
                    out.insert(c, utf16(&dst));
                }
                i += 2;
            }
            "range" if i + 2 < tokens.len() => {
                let (lo, hi) = (code(t), code(&tokens[i + 1]));
                if tokens[i + 2] == "[" {
                    let mut k = i + 3;
                    let mut c = lo.unwrap_or(0);
                    while k < tokens.len() && tokens[k] != "]" {
                        if let Some(dst) = hex(&tokens[k]) {
                            out.insert(c, utf16(&dst));
                        }
                        c += 1;
                        k += 1;
                    }
                    i = k + 1;
                } else {
                    if let (Some(lo), Some(hi), Some(dst)) = (lo, hi, hex(&tokens[i + 2]))
                        && hi >= lo
                        && hi - lo < 65_536
                        && dst.len() >= 2
                    {
                        // The last character steps along the range.
                        let n = dst.len();
                        let last = u16::from_be_bytes([dst[n - 2], dst[n - 1]]);
                        for (k, c) in (lo..=hi).enumerate() {
                            let mut d = dst.clone();
                            let [a, b] = last.wrapping_add(k as u16).to_be_bytes();
                            d[n - 2] = a;
                            d[n - 1] = b;
                            out.insert(c, utf16(&d));
                        }
                    }
                    i += 3;
                }
            }
            _ => i += 1,
        }
    }
    out
}

/// A simple font's encoding: its base (WinAnsi unless it says otherwise) with its
/// `Differences`.
fn encoding(doc: &Document, dict: &Dictionary) -> [Option<char>; 256] {
    let mut table = win_ansi();
    let enc = dict
        .get(b"Encoding")
        .ok()
        .and_then(|o| doc.dereference(o).ok().map(|(_, o)| o.clone()));
    let differences = match &enc {
        Some(Object::Dictionary(d)) => d.get(b"Differences").ok().cloned(),
        _ => None,
    };
    if let Some(Object::Array(diffs)) = differences {
        let mut code = 0usize;
        for o in &diffs {
            match o {
                Object::Integer(c) => code = (*c).clamp(0, 255) as usize,
                Object::Name(n) => {
                    if code < 256 {
                        table[code] = glyph_char(&String::from_utf8_lossy(n));
                    }
                    code += 1;
                }
                _ => {}
            }
        }
    }
    table
}

/// Windows' Latin encoding (as PDF's WinAnsiEncoding has it).
fn win_ansi() -> [Option<char>; 256] {
    let mut t = [None; 256];
    for (c, slot) in t.iter_mut().enumerate().skip(32) {
        *slot = char::from_u32(c as u32);
    }
    let high = [
        (0x80, '€'),
        (0x82, '‚'),
        (0x83, 'ƒ'),
        (0x84, '„'),
        (0x85, '…'),
        (0x86, '†'),
        (0x87, '‡'),
        (0x88, 'ˆ'),
        (0x89, '‰'),
        (0x8a, 'Š'),
        (0x8b, '‹'),
        (0x8c, 'Œ'),
        (0x8e, 'Ž'),
        (0x91, '\u{2018}'),
        (0x92, '\u{2019}'),
        (0x93, '\u{201c}'),
        (0x94, '\u{201d}'),
        (0x95, '•'),
        (0x96, '–'),
        (0x97, '—'),
        (0x98, '˜'),
        (0x99, '™'),
        (0x9a, 'š'),
        (0x9b, '›'),
        (0x9c, 'œ'),
        (0x9e, 'ž'),
        (0x9f, 'Ÿ'),
    ];
    t[0x80..0xa0].fill(None);
    for (c, ch) in high {
        t[c] = Some(ch);
    }
    t[0x7f] = None;
    t
}

/// The character a glyph name stands for (the common ones, and `uniXXXX`).
fn glyph_char(name: &str) -> Option<char> {
    if name.chars().count() == 1 {
        return name.chars().next();
    }
    if let Some(hex) = name.strip_prefix("uni").or_else(|| name.strip_prefix('u'))
        && (4..=6).contains(&hex.len())
        && let Ok(code) = u32::from_str_radix(hex, 16)
    {
        return char::from_u32(code);
    }
    Some(match name {
        "space" | "nbspace" => ' ',
        "zero" => '0',
        "one" => '1',
        "two" => '2',
        "three" => '3',
        "four" => '4',
        "five" => '5',
        "six" => '6',
        "seven" => '7',
        "eight" => '8',
        "nine" => '9',
        "period" => '.',
        "comma" => ',',
        "colon" => ':',
        "semicolon" => ';',
        "hyphen" | "minus" => '-',
        "endash" => '–',
        "emdash" => '—',
        "quotesingle" => '\'',
        "quotedbl" => '"',
        "quoteright" => '\u{2019}',
        "quoteleft" => '\u{2018}',
        "quotedblleft" => '\u{201c}',
        "quotedblright" => '\u{201d}',
        "minute" | "prime" => '′',
        "second" | "dblprime" => '″',
        "parenleft" => '(',
        "parenright" => ')',
        "bracketleft" => '[',
        "bracketright" => ']',
        "braceleft" => '{',
        "braceright" => '}',
        "slash" => '/',
        "backslash" => '\\',
        "fraction" => '⁄',
        "equal" => '=',
        "plus" => '+',
        "plusminus" => '±',
        "multiply" => '×',
        "less" => '<',
        "greater" => '>',
        "degree" => '°',
        "percent" => '%',
        "ampersand" => '&',
        "at" => '@',
        "numbersign" => '#',
        "asterisk" => '*',
        "underscore" => '_',
        "exclam" => '!',
        "question" => '?',
        "dollar" => '$',
        "bullet" => '•',
        "periodcentered" => '·',
        "onehalf" => '½',
        "onequarter" => '¼',
        "threequarters" => '¾',
        "Oslash" => 'Ø',
        "oslash" => 'ø',
        "mu" => 'µ',
        "section" => '§',
        "paragraph" => '¶',
        "copyright" => '©',
        "registered" => '®',
        "trademark" => '™',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmaps_map_codes_singly_and_in_ranges() {
        let cmap = "/CIDInit /ProcSet findresource begin\n\
            2 beginbfchar\n<0003> <0020>\n<0011> <00D8>\nendbfchar\n\
            1 beginbfrange\n<0014> <001D> <0030>\nendbfrange\n\
            1 beginbfrange\n<0024> <0025> [<0041> <0042>]\nendbfrange\nend";
        let map = parse_cmap(cmap);
        assert_eq!(map[&3], " ");
        assert_eq!(map[&0x11], "Ø");
        assert_eq!(map[&0x14], "0");
        assert_eq!(map[&0x1d], "9");
        assert_eq!(map[&0x25], "B");
    }

    #[test]
    fn glyph_names_read_as_characters() {
        assert_eq!(glyph_char("three"), Some('3'));
        assert_eq!(glyph_char("quoteright"), Some('\u{2019}'));
        assert_eq!(glyph_char("uni00D8"), Some('Ø'));
        assert_eq!(glyph_char("g123"), None);
        assert_eq!(win_ansi()[0x92], Some('\u{2019}'));
        assert_eq!(win_ansi()[b'A' as usize], Some('A'));
    }
}
