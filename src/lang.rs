//! The interface language: Turkish by default, English as a choice on the Settings tab.
//! Every visible string is written as `tl("Türkçe", "English")` or `tf!("…{x}…", "…{x}…")`.

use std::sync::atomic::{AtomicBool, Ordering};

static EN: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Tr,
    En,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::Tr, Lang::En];

    pub fn key(self) -> &'static str {
        match self {
            Lang::Tr => "tr",
            Lang::En => "en",
        }
    }

    pub fn from_key(s: &str) -> Lang {
        if s == "en" { Lang::En } else { Lang::Tr }
    }

    pub fn index(self) -> usize {
        Lang::ALL.iter().position(|l| *l == self).unwrap_or(0)
    }

    /// Each language's own name, so either can be found whatever is set.
    pub fn name(self) -> &'static str {
        match self {
            Lang::Tr => "Türkçe",
            Lang::En => "English",
        }
    }
}

pub fn set(l: Lang) {
    EN.store(l == Lang::En, Ordering::Relaxed);
}

pub fn en() -> bool {
    EN.load(Ordering::Relaxed)
}

/// The string of the interface language.
pub fn tl(tr: &'static str, en_text: &'static str) -> &'static str {
    if en() { en_text } else { tr }
}

/// `format!` in the interface language.
#[macro_export]
macro_rules! tf {
    ($tr:literal, $en:literal $(, $a:expr)* $(,)?) => {
        if $crate::lang::en() { format!($en $(, $a)*) } else { format!($tr $(, $a)*) }
    };
}

/// The question particle after `w`, by its last vowel: hala mı, hâlâ mı, bekâr mı, gün mü.
pub fn mi(w: &str) -> &'static str {
    let last = w.chars().rev().find(|c| "aâıeiîoöuüûAÂIİEOÖUÜ".contains(*c));
    match last.map(|c| crate::turkish::lower(&c.to_string())).as_deref() {
        Some("e" | "i" | "î") => "mi",
        Some("o" | "u" | "û") => "mu",
        Some("ö" | "ü") => "mü",
        _ => "mı",
    }
}

/// A decimal number as the language writes it: 12,5 in Turkish, 12.5 in English.
pub fn decimal(x: f64, places: usize) -> String {
    let s = format!("{x:.places$}");
    if en() { s } else { s.replace('.', ",") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn particle_follows_the_last_vowel() {
        assert_eq!(mi("hala"), "mı");
        assert_eq!(mi("hâlâ"), "mı");
        assert_eq!(mi("bekâr"), "mı");
        assert_eq!(mi("yeraltı"), "mı");
        assert_eq!(mi("yer altında"), "mı");
        assert_eq!(mi("hakim"), "mi");
        assert_eq!(mi("aşık"), "mı");
        assert_eq!(mi("Usul"), "mu");
        assert_eq!(mi("gün"), "mü");
    }
}
