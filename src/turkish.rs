//! Turkish text rules the dictionary needs: casing with the dotted and dotless i, a key that
//! ignores circumflexes, case and spacing, and letter counts.

/// Lowercase by Turkish rules: `I` → `ı`, `İ` → `i`.
pub fn lower(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            'I' => out.push('ı'),
            'İ' => out.push('i'),
            _ => out.extend(c.to_lowercase()),
        }
    }
    // `İ`.to_lowercase() is "i̇" in Unicode; the arm above keeps it a plain `i`.
    out
}

/// Uppercase the first letter by Turkish rules (`i` → `İ`, `ı` → `I`).
pub fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some('i') => format!("İ{}", chars.as_str()),
        Some('ı') => format!("I{}", chars.as_str()),
        Some(c) => c.to_uppercase().chain(chars).collect(),
    }
}

/// Whether the first letter is a capital.
pub fn is_capitalized(s: &str) -> bool {
    s.chars().next().is_some_and(char::is_uppercase)
}

/// Uppercase by Turkish rules: `i` → `İ`, `ı` → `I`.
pub fn upper(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            'i' => out.push('İ'),
            'ı' => out.push('I'),
            _ => out.extend(c.to_uppercase()),
        }
    }
    out
}

/// Whether a word is written in capitals: two letters or more, and none of them small.
pub fn is_all_caps(s: &str) -> bool {
    let letters = || s.chars().filter(|c| c.is_alphabetic());
    letters().count() >= 2 && letters().all(char::is_uppercase)
}

/// The text of a file: UTF-8 or UTF-16 by its byte order mark, UTF-8 when it is UTF-8 (a few
/// stray bytes cost a character each), and otherwise Windows-1254, the code page Turkish
/// subtitles are written in when they are not UTF-8 (where ç ğ ı ö ş ü would otherwise all
/// become U+FFFD).
pub fn decode_text(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    fn utf16(rest: &[u8], unit: fn([u8; 2]) -> u16) -> String {
        let units = rest.as_chunks::<2>().0.iter().map(|&b| unit(b));
        char::decode_utf16(units)
            .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect()
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, u16::from_le_bytes);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, u16::from_be_bytes);
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_owned();
    }
    // In Windows-1254 nearly every byte above 0x7F breaks UTF-8; in UTF-8 with a stray byte,
    // nearly none does.
    let lossy = String::from_utf8_lossy(bytes);
    let broken = lossy
        .chars()
        .filter(|&c| c == char::REPLACEMENT_CHARACTER)
        .count();
    let high = bytes.iter().filter(|&&b| b > 0x7F).count();
    if broken * 10 < high {
        lossy.into_owned()
    } else {
        bytes.iter().map(|&b| cp1254(b)).collect()
    }
}

/// One Windows-1254 byte: Latin-1 but for six Turkish letters and the 0x80–0x9F block.
fn cp1254(b: u8) -> char {
    const NONE: char = char::REPLACEMENT_CHARACTER;
    const HIGH: [char; 32] = [
        '\u{20AC}', NONE, '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
        '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', NONE, NONE, NONE, NONE,
        '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
        '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', NONE, NONE, '\u{0178}',
    ];
    match b {
        0x80..=0x9F => HIGH[(b - 0x80) as usize],
        0xD0 => 'Ğ',
        0xDD => 'İ',
        0xDE => 'Ş',
        0xF0 => 'ğ',
        0xFD => 'ı',
        0xFE => 'ş',
        _ => b as char,
    }
}

/// The key that ignores circumflexes, case, spaces and hyphens: `Kâğıt` and `kağıt` meet, so do
/// `yer altı` and `yeraltı`.
pub fn fold(s: &str) -> String {
    lower(s)
        .chars()
        .filter_map(|c| match c {
            'â' => Some('a'),
            'î' => Some('i'),
            'û' => Some('u'),
            ' ' | '-' => None,
            c => Some(c),
        })
        .collect()
}

/// Lowercase, without circumflexes and with the Turkish letters as their plain Latin ones
/// (ç ğ ı ö ş ü → c g i o s u), spacing kept: the form a search compares by prefix, so `kagit`
/// finds `kâğıt` on a keyboard without Turkish letters.
pub fn search_key(s: &str) -> String {
    lower(s)
        .chars()
        .map(|c| match c {
            'â' => 'a',
            'î' | 'ı' => 'i',
            'û' => 'u',
            'ç' => 'c',
            'ğ' => 'g',
            'ö' => 'o',
            'ş' => 's',
            'ü' => 'u',
            c => c,
        })
        .collect()
}

/// Letters in a word or phrase (spaces and punctuation not counted), for a CPS cut.
pub fn letters(s: &str) -> usize {
    s.chars().filter(|c| c.is_alphabetic()).count()
}

/// A letter that can be part of a Turkish word.
pub fn is_word_char(c: char) -> bool {
    c.is_alphabetic()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotted_and_dotless() {
        assert_eq!(lower("IRMAK İzmir"), "ırmak izmir");
        assert_eq!(capitalize("istanbul"), "İstanbul");
        assert_eq!(capitalize("ılık"), "Ilık");
    }

    #[test]
    fn capitals_by_turkish_rules() {
        assert_eq!(upper("kâğıt istanbul ılık"), "KÂĞIT İSTANBUL ILIK");
        assert!(is_all_caps("HERŞEY"));
        assert!(is_all_caps("KAĞIT,"));
        assert!(!is_all_caps("Herşey"));
        assert!(
            !is_all_caps("A"),
            "one capital letter is a capitalized word"
        );
        assert!(!is_all_caps("12"));
    }

    #[test]
    fn a_file_is_read_in_its_own_encoding() {
        let word = "görüşürüz ılık İğne";
        assert_eq!(decode_text(word.as_bytes()), word);
        let bom: Vec<u8> = [0xEF, 0xBB, 0xBF]
            .iter()
            .chain(word.as_bytes())
            .copied()
            .collect();
        assert_eq!(decode_text(&bom), word);
        let le: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain(word.encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(decode_text(&le), word);
        let be: Vec<u8> = [0xFE, 0xFF]
            .into_iter()
            .chain(word.encode_utf16().flat_map(u16::to_be_bytes))
            .collect();
        assert_eq!(decode_text(&be), word);
        // The same text in Windows-1254.
        let ansi = b"g\xF6r\xFC\xFE\xFCr\xFCz \xFDl\xFDk \xDD\xF0ne";
        assert_eq!(decode_text(ansi), word);
        assert_eq!(decode_text(b"\x80 \x93a\x94"), "\u{20AC} \u{201C}a\u{201D}");
        // UTF-8 with one stray byte stays UTF-8: only that byte is lost.
        let mut stray = "görüşürüz ılık İğne görüşürüz".as_bytes().to_vec();
        stray.insert(0, 0xFF);
        assert_eq!(decode_text(&stray), "\u{FFFD}görüşürüz ılık İğne görüşürüz");
    }

    #[test]
    fn fold_meets_variants() {
        assert_eq!(fold("Kâğıt"), fold("kağıt"));
        assert_eq!(fold("yer altı"), fold("yeraltı"));
        assert_ne!(fold("kâğıt"), fold("kağit"));
        assert_eq!(search_key("Hâlâ"), "hala");
        assert_eq!(search_key("Kâğıt"), "kagit");
    }

    #[test]
    fn letter_count() {
        assert_eq!(letters("yasa dışı"), 8);
        assert_eq!(letters("yeraltı"), 7);
    }
}
