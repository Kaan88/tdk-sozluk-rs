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
