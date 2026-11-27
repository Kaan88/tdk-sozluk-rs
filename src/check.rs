//! The line check: every word of a text against the headword index. A word is in TDK as typed
//! or by its stem, differs from TDK's spelling (a circumflex, joined or apart), is one of two
//! TDK words whose meaning decides (hala / hâlâ, yeraltı / yer altı), a name, or not found.

use crate::dict::Index;
use crate::turkish::{capitalize, is_capitalized, is_word_char, lower};

/// Written together by mistake, with the TDK form (Yazım Kılavuzu; these are phrases, not
/// headwords, so the index alone cannot say). The same list as the subtitle skill's `sfx.py`.
const JOINED: &[(&str, &str)] = &[
    ("hiçbirşey", "hiçbir şey"),
    ("herşey", "her şey"),
    ("birşey", "bir şey"),
    ("şuan", "şu an"),
    ("birsürü", "bir sürü"),
    ("değilmi", "değil mi"),
    ("herkez", "herkes"),
    ("yanlız", "yalnız"),
    ("tabiki", "tabii ki"),
];

/// Both spellings are TDK headwords, and the plain one is often typed for the other. Only these
/// are flagged: the generic rule would flag `ama` (âmâ), `dahi`, `tabii`, `metin` on every line.
const MEANING_PAIRS: &[&str] = &[
    "hala", "kar", "karlı", "hal", "alem", "adet", "aşık", "dahil", "hakim", "bekar", "yar",
    "fani", "varis", "milli",
];

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// In TDK, as typed or by this stem.
    Ok(Option<String>),
    /// TDK writes it so.
    Spelling { fix: String, why: &'static str },
    /// Both are TDK words; the meaning decides.
    Pair { other: String },
    /// Capitalised and not in TDK: taken as a name.
    Name,
    /// Neither the word nor a stem of it is a headword.
    Unknown,
}

#[derive(Clone, Debug)]
pub struct Token {
    /// Byte range in the text.
    pub start: usize,
    pub end: usize,
    pub word: String,
    pub kind: Kind,
}

/// Matches the case of `like` (a capital first letter) on `w`.
fn cased(w: &str, like: &str) -> String {
    if is_capitalized(like) {
        capitalize(w)
    } else {
        w.to_string()
    }
}

fn pick_variant<'a>(vs: &[&'a str], typed_has_space: bool) -> Option<&'a str> {
    vs.iter()
        .find(|v| v.contains(' ') == typed_has_space)
        .or_else(|| vs.first())
        .copied()
}

/// What one word is.
pub fn classify(ix: &Index, word: &str, ignore: &[String]) -> Kind {
    let l = lower(word);
    if ignore.iter().any(|i| lower(i) == l) {
        return Kind::Name;
    }
    // The question particle and its persons (mı, misin, muyuz, müydü…) is written apart and is
    // not a headword.
    if is_question_particle(&l) {
        return Kind::Ok(None);
    }
    for (bad, good) in JOINED {
        if let Some(rest) = l.strip_prefix(bad) {
            return Kind::Spelling {
                fix: cased(&format!("{good}{rest}"), word),
                why: "written apart",
            };
        }
    }
    if ix.contains(&l) {
        if MEANING_PAIRS.contains(&l.as_str())
            && let Some(o) = ix.variants(&l).first()
        {
            return Kind::Pair {
                other: o.to_string(),
            };
        }
        // Joined and apart both exist (yeraltı / yer altı).
        if let Some(o) = ix.variants(&l).into_iter().find(|v| v.contains(' ')) {
            return Kind::Pair {
                other: o.to_string(),
            };
        }
        return Kind::Ok(None);
    }
    let vs = ix.variants(&l);
    if let Some(v) = pick_variant(&vs, false) {
        return Kind::Spelling {
            fix: cased(v, word),
            why: "TDK spelling",
        };
    }
    // The longest stem that is a headword, a verb stem, or a misspelt headword.
    let chars: Vec<(usize, char)> = l.char_indices().collect();
    for n in (2..chars.len()).rev() {
        let cut = chars[n].0;
        let (stem, rest) = l.split_at(cut);
        if !suffix_like(rest) || !stem.chars().any(|c| "aeıioöuüâîû".contains(c)) {
            continue;
        }
        // Consonant softening before a vowel: kâğıd-ı is kâğıt, kitab-ı is kitap.
        let hard = unsoften(stem);
        let found = [Some(stem.to_string()), hard]
            .into_iter()
            .flatten()
            .find(|s| ix.contains(s) || ix.is_stem(s));
        if let Some(found) = found {
            if MEANING_PAIRS.contains(&found.as_str())
                && let Some(o) = ix.variants(&found).first()
            {
                return Kind::Pair {
                    other: cased(&format!("{}{}", soften_like(o, stem, &found), rest), word),
                };
            }
            if let Some(o) = ix.variants(&found).into_iter().find(|v| v.contains(' ')) {
                return Kind::Pair {
                    other: cased(&format!("{}{}", soften_like(o, stem, &found), rest), word),
                };
            }
            return Kind::Ok(Some(found));
        }
        if n >= 4 {
            let vs = ix.variants(stem);
            if let Some(v) = pick_variant(&vs, false).filter(|v| !v.contains(' ')) {
                return Kind::Spelling {
                    fix: cased(&format!("{v}{rest}"), word),
                    why: "TDK spelling of the stem",
                };
            }
        }
    }
    if is_capitalized(word) {
        Kind::Name
    } else {
        Kind::Unknown
    }
}

fn is_question_particle(l: &str) -> bool {
    let mut cs = l.chars();
    if cs.next() != Some('m') {
        return false;
    }
    let Some(v) = cs.next() else { return false };
    if !"ıiuü".contains(v) {
        return false;
    }
    let rest: String = cs.collect();
    const ENDS: &[&str] = &[
        "", "yım", "yim", "yum", "yüm", "sın", "sin", "sun", "sün", "yız", "yiz", "yuz", "yüz",
        "sınız", "siniz", "sunuz", "sünüz", "ydı", "ydi", "ydu", "ydü", "ymış", "ymiş", "ymuş",
        "ymüş", "dır", "dir", "dur", "dür", "ydım", "ydim", "ydum", "ydüm", "ydın", "ydin", "ydun",
        "ydün", "ydık", "ydik", "yduk", "ydük", "ydınız", "ydiniz", "ydunuz", "ydünüz", "ydılar",
        "ydiler", "ydular", "ydüler",
    ];
    ENDS.contains(&rest.as_str())
}

/// Whether `rest` can be Turkish suffixes: no suffix starts with two consonants.
fn suffix_like(rest: &str) -> bool {
    const VOWELS: &str = "aeıioöuüâîû";
    let mut cs = rest.chars();
    match (cs.next(), cs.next()) {
        // n and y are buffer consonants: yeraltı-nda, yeraltı-yla.
        (Some(a), Some(b)) => {
            VOWELS.contains(a)
                || VOWELS.contains(b)
                || (a == 'n' && matches!(b, 'd' | 'c'))
                || (a == 'y' && b == 'l')
        }
        _ => true,
    }
}

/// The stem with its last consonant hardened (d → t, b → p, c → ç, ğ → k), if it ends so.
fn unsoften(stem: &str) -> Option<String> {
    let last = stem.chars().last()?;
    let hard = match last {
        'd' => 't',
        'b' => 'p',
        'c' => 'ç',
        'ğ' => 'k',
        _ => return None,
    };
    Some(format!("{}{}", &stem[..stem.len() - last.len_utf8()], hard))
}

/// `other` with the same softening the typed stem had (if `stem` is the softened `found`).
fn soften_like(other: &str, stem: &str, found: &str) -> String {
    if stem == found {
        return other.to_string();
    }
    let (Some(soft), Some(hard)) = (stem.chars().last(), found.chars().last()) else {
        return other.to_string();
    };
    match other.strip_suffix(hard) {
        Some(head) => format!("{head}{soft}"),
        None => other.to_string(),
    }
}

/// Every word of `text`. A suffix after an apostrophe (Kaan'ın) is not looked up.
pub fn check(ix: &Index, text: &str, ignore: &[String]) -> Vec<Token> {
    let mut out = Vec::new();
    let mut it = text.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        if !is_word_char(c) {
            continue;
        }
        let mut end = i + c.len_utf8();
        while let Some(&(j, d)) = it.peek() {
            if is_word_char(d) {
                end = j + d.len_utf8();
                it.next();
            } else {
                break;
            }
        }
        let word = &text[i..end];
        let kind = classify(ix, word, ignore);
        out.push(Token {
            start: i,
            end,
            word: word.to_string(),
            kind,
        });
        // Skip a suffix after an apostrophe.
        if let Some(&(_, '\'' | '’')) = it.peek() {
            it.next();
            while let Some(&(_, d)) = it.peek() {
                if is_word_char(d) {
                    it.next();
                } else {
                    break;
                }
            }
        }
    }
    out
}

/// The text with every spelling fix applied (pairs are left for a person).
pub fn fixed(text: &str, tokens: &[Token]) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut at = 0;
    for t in tokens {
        if let Kind::Spelling { fix, .. } = &t.kind {
            out.push_str(&text[at..t.start]);
            out.push_str(fix);
            at = t.end;
        }
    }
    out.push_str(&text[at..]);
    out
}

/// Replaces one token with `with` (a pair decided by the user).
pub fn replace(text: &str, t: &Token, with: &str) -> String {
    format!("{}{}{}", &text[..t.start], with, &text[t.end..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(String, Kind)> {
        let ix = Index::load();
        check(&ix, text, &[])
            .into_iter()
            .map(|t| (t.word, t.kind))
            .collect()
    }

    #[test]
    fn spelling_and_pairs() {
        let ix = Index::load();
        let text = "Herşey kağıt üzerinde rengarenk görünüyordu, ama Brennan hala yeraltında. Bekarlar mı?";
        let toks = check(&ix, text, &[]);
        let k = |w: &str| toks.iter().find(|t| t.word == w).unwrap().kind.clone();
        assert_eq!(
            k("Herşey"),
            Kind::Spelling {
                fix: "Her şey".into(),
                why: "written apart"
            }
        );
        assert_eq!(
            k("kağıt"),
            Kind::Spelling {
                fix: "kâğıt".into(),
                why: "TDK spelling"
            }
        );
        assert_eq!(
            k("rengarenk"),
            Kind::Spelling {
                fix: "rengârenk".into(),
                why: "TDK spelling"
            }
        );
        assert!(matches!(k("görünüyordu"), Kind::Ok(Some(_))));
        assert_eq!(k("ama"), Kind::Ok(None));
        assert_eq!(k("Brennan"), Kind::Name);
        assert_eq!(k("mı"), Kind::Ok(None));
        assert_eq!(classify(&ix, "misiniz", &[]), Kind::Ok(None));
        assert_eq!(
            k("hala"),
            Kind::Pair {
                other: "hâlâ".into()
            }
        );
        assert_eq!(
            k("yeraltında"),
            Kind::Pair {
                other: "yer altında".into()
            }
        );
        assert_eq!(
            k("Bekarlar"),
            Kind::Pair {
                other: "Bekârlar".into()
            }
        );
        assert_eq!(
            fixed(text, &toks),
            "Her şey kâğıt üzerinde rengârenk görünüyordu, ama Brennan hala yeraltında. Bekarlar mı?"
        );
    }

    #[test]
    fn apostrophe_suffix_skipped() {
        let ks = kinds("Kaan'ın kâğıdı");
        assert_eq!(ks.len(), 2);
        assert_eq!(ks[0].1, Kind::Name);
        assert_eq!(ks[1].1, Kind::Ok(Some("kâğıt".into())));
    }
}
