//! How an entry used to read: the notes shown under it ("Meaning 3 was added after 2019") and the
//! bar for a spelling TDK no longer has. They come from `history.sqlite`, built by
//! `tools/build-history.py` and kept apart from `gts.sqlite`, which holds only the current entries.
//! The file is optional; without it the app shows entries as it always did.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use crate::dict::Entry;
use crate::lang::{en, tl};
use crate::tf;
use crate::turkish::{lower, year_ablative, year_dative};

/// The most lines the notes box holds.
const MAX_NOTES: usize = 4;
/// The most removed meanings told of.
const MAX_REMOVED: usize = 2;
/// How alike two meanings' texts are (Dice coefficient on letter pairs) for the one to be the other
/// reworded.
const SIMILAR: f32 = 0.6;
/// A removed meaning is quoted up to this many characters.
const QUOTE: usize = 60;

/// Where `history.sqlite` may be: `$SOZLUK_HISTORY`, beside the program, in the app's data folder.
pub fn find_history_db(data_dir: Option<&Path>) -> Option<PathBuf> {
    let mut c: Vec<PathBuf> = Vec::new();
    if let Ok(p) = std::env::var("SOZLUK_HISTORY") {
        c.push(PathBuf::from(p));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(d) = exe.parent()
    {
        c.push(d.join("history.sqlite"));
    }
    if let Some(d) = data_dir {
        c.push(d.join("history.sqlite"));
    }
    c.into_iter().find(|p| p.is_file())
}

/// One form an entry had.
#[derive(Clone, Debug)]
struct Version {
    last: String,
    sources: String,
    texts: Vec<String>,
    plain: Vec<String>,
}

impl Version {
    /// A copy of the site's page only can miss homographs, so what it lacks proves nothing.
    fn wayback_only(&self) -> bool {
        self.sources.split(',').all(|s| s.trim() == "Wayback")
    }
}

/// How a word's spelling changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Respelt {
    /// It was two words and is one now.
    Apart,
    /// It was one word and is two now.
    Together,
    Other,
}

/// One line of the notes box. Kept as data, so a change of language rewords it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Note {
    /// Written differently until `year`.
    Spelling {
        how: Respelt,
        old: String,
        year: String,
    },
    /// Meanings (numbered as the page numbers them; `label` names the entry when there are several)
    /// that were not there in the forms up to `year`.
    Added {
        label: Option<String>,
        nums: Vec<usize>,
        year: String,
    },
    Reworded {
        label: Option<String>,
        nums: Vec<usize>,
        year: String,
    },
    Removed {
        text: String,
        year: String,
    },
    /// The changes that did not fit.
    More(usize),
}

/// A word TDK no longer spells so, and what replaced it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OldSpelling {
    pub typed: String,
    pub new: String,
    pub year: String,
}

impl OldSpelling {
    pub fn text(&self) -> String {
        tf!(
            "\u{201c}{0}\u{201d} {1} böyle yazılıyordu; bugün: {2}",
            "\u{201c}{0}\u{201d} was written this way until {1}; today: {2}",
            self.typed,
            until(&self.year),
            self.new
        )
    }
}

/// "until 2019": the bare year after the English "until", "2019'a kadar" in Turkish.
fn until(year: &str) -> String {
    if en() {
        year.to_string()
    } else {
        format!("{} kadar", year_dative(year))
    }
}

/// The numbers as a sentence: `3`, `3 and 4`, `1, 3 and 4` (Turkish `3. ve 4.`).
fn number_list(nums: &[usize]) -> String {
    let (and, dot) = if en() { ("and", "") } else { ("ve", ".") };
    let item = |n: &usize| format!("{n}{dot}");
    match nums {
        [] => String::new(),
        [one] => item(one),
        [head @ .., last] => format!(
            "{} {and} {}",
            head.iter().map(item).collect::<Vec<_>>().join(", "),
            item(last)
        ),
    }
}

/// "Meanings 3 and 4 were added after 2019."
fn meanings_note(label: &Option<String>, nums: &[usize], year: &str, reworded: bool) -> String {
    let list = number_list(nums);
    let one = nums.len() == 1;
    let text = if en() {
        let verb = if reworded { "reworded" } else { "added" };
        if one {
            format!("Meaning {list} was {verb} after {year}.")
        } else {
            format!("Meanings {list} were {verb} after {year}.")
        }
    } else {
        let verb = if reworded {
            "yeniden yazıldı"
        } else {
            "eklendi"
        };
        let plural = if one { "" } else { "lar" };
        format!("{list} anlam{plural} {} sonra {verb}.", year_ablative(year))
    };
    match label {
        Some(l) => format!("{l}: {text}"),
        None => text,
    }
}

impl Note {
    pub fn text(&self) -> String {
        match self {
            Note::Spelling { how, old, year } => match how {
                Respelt::Apart => tf!(
                    "{1} ayrı yazılıyordu: \u{201c}{0}\u{201d}.",
                    "Written as two words until {1}: \u{201c}{0}\u{201d}.",
                    old,
                    until(year)
                ),
                Respelt::Together => tf!(
                    "{1} bitişik yazılıyordu: \u{201c}{0}\u{201d}.",
                    "Written as one word until {1}: \u{201c}{0}\u{201d}.",
                    old,
                    until(year)
                ),
                Respelt::Other => tf!(
                    "{1} \u{201c}{0}\u{201d} diye yazılıyordu.",
                    "Written \u{201c}{0}\u{201d} until {1}.",
                    old,
                    until(year)
                ),
            },
            Note::Added { label, nums, year } => meanings_note(label, nums, year, false),
            Note::Reworded { label, nums, year } => meanings_note(label, nums, year, true),
            Note::Removed { text, year } => tf!(
                "{1} sonra bir anlam çıkarıldı: \u{201c}{0}\u{201d}.",
                "A meaning was removed after {1}: \u{201c}{0}\u{201d}.",
                text,
                if en() {
                    year.clone()
                } else {
                    year_ablative(year)
                }
            ),
            Note::More(n) => tf!("…ve {} değişiklik daha.", "…and {} more changes.", n),
        }
    }
}

/// The year of a date (`2019-06-06` → `2019`).
fn year(date: &str) -> String {
    date.chars().take(4).collect()
}

// ---- comparing meanings ---------------------------------------------------------------------

/// A meaning's text as it is compared: no "►" or "bk.", Turkish lower case, no circumflexes, every
/// run of other characters one space.
fn plain(s: &str) -> String {
    let s = lower(&s.replace('►', " ").replace("bk.", " "));
    let mut out = String::with_capacity(s.len());
    let mut gap = false;
    for c in s.chars() {
        let c = match c {
            'â' => 'a',
            'î' => 'i',
            'û' => 'u',
            'ô' => 'o',
            c => c,
        };
        if c.is_alphanumeric() {
            if gap && !out.is_empty() {
                out.push(' ');
            }
            gap = false;
            out.push(c);
        } else {
            gap = true;
        }
    }
    out
}

/// 2·(pairs in common) / (pairs of a + pairs of b), on pairs of neighbouring characters.
fn dice(a: &str, b: &str) -> f32 {
    fn pairs(s: &str) -> HashMap<(char, char), usize> {
        let cs: Vec<char> = s.chars().collect();
        let mut m = HashMap::new();
        for w in cs.windows(2) {
            *m.entry((w[0], w[1])).or_insert(0) += 1;
        }
        m
    }
    let (pa, pb) = (pairs(a), pairs(b));
    let total: usize = pa.values().sum::<usize>() + pb.values().sum::<usize>();
    if total == 0 {
        return 0.0;
    }
    let common: usize = pa
        .iter()
        .map(|(k, n)| (*n).min(pb.get(k).copied().unwrap_or(0)))
        .sum();
    2.0 * common as f32 / total as f32
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Match {
    None,
    Similar,
    Exact,
}

/// Whether two meanings read alike: a Dice coefficient of at least `SIMILAR`, or one only the start of
/// the other (a clause dropped or added at the end).
fn alike(a: &str, b: &str) -> bool {
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    let starts = short.len() >= 4
        && long
            .strip_prefix(short)
            .is_some_and(|rest| rest.starts_with(' '));
    starts || dice(a, b) >= SIMILAR
}

/// How well the meaning `p` is found among `plains` (an exact match wins over a similar one).
fn find_in(p: &str, plains: &[String]) -> Match {
    if plains.iter().any(|q| q == p) {
        Match::Exact
    } else if plains.iter().any(|q| alike(p, q)) {
        Match::Similar
    } else {
        Match::None
    }
}

/// The notes on the meanings of `entries` from the forms `hist` (oldest first) had.
fn meaning_notes(entries: &[Entry], hist: &[Version]) -> Vec<Note> {
    if hist.is_empty() {
        return Vec::new();
    }
    let last = hist.len() - 1;
    // The meanings as the page numbers them: each entry from 1.
    let many = entries.len() > 1;
    let mut cur: Vec<(Option<String>, usize, String)> = Vec::new();
    for (ei, e) in entries.iter().enumerate() {
        let label = many.then(|| format!("{} {}", e.headword, ei + 1));
        for (mi, m) in e.meanings.iter().enumerate() {
            cur.push((label.clone(), mi + 1, plain(&m.text)));
        }
    }
    // (reworded, entry, year) → the numbers; added sorts before reworded.
    let mut groups: BTreeMap<(bool, Option<String>, String), Vec<usize>> = BTreeMap::new();
    for (label, n, p) in &cur {
        if p.is_empty() {
            continue;
        }
        let m: Vec<Match> = hist.iter().map(|v| find_in(p, &v.plain)).collect();
        // Added after the newest form that lacks it.
        let missing = if m[last] == Match::None {
            Some(last)
        } else {
            (0..last).rev().find(|&v| m[v] == Match::None)
        };
        let (reworded, at) = match missing {
            Some(v) => (false, v),
            None => {
                // Reworded after the newest form that only has something like it, unless a newer
                // form has it as it is.
                let exact = (0..=last).rev().find(|&v| m[v] == Match::Exact);
                let Some(similar) = (0..=last).rev().find(|&v| m[v] == Match::Similar) else {
                    continue;
                };
                if exact > Some(similar) {
                    continue;
                }
                (true, similar)
            }
        };
        groups
            .entry((reworded, label.clone(), year(&hist[at].last)))
            .or_default()
            .push(*n);
    }
    let mut out: Vec<Note> = groups
        .into_iter()
        .map(|((reworded, label, year), nums)| {
            if reworded {
                Note::Reworded { label, nums, year }
            } else {
                Note::Added { label, nums, year }
            }
        })
        .collect();

    // Removed: a meaning of some form that neither today's entry nor any newer form has, not even
    // reworded. A copy of the site's page only says nothing of what it lacks.
    let cur_plains: Vec<String> = cur.into_iter().map(|c| c.2).collect();
    let mut removed = 0;
    for (v, form) in hist.iter().enumerate().rev() {
        if form.wayback_only() {
            continue;
        }
        for (text, p) in form.texts.iter().zip(&form.plain) {
            let gone = !p.is_empty()
                && find_in(p, &cur_plains) == Match::None
                && hist[v + 1..]
                    .iter()
                    .all(|w| find_in(p, &w.plain) == Match::None);
            if gone && removed < MAX_REMOVED {
                removed += 1;
                out.push(Note::Removed {
                    text: quote(text),
                    year: year(&form.last),
                });
            }
        }
    }
    out
}

/// A meaning's first characters, cut with an ellipsis when it is longer.
fn quote(s: &str) -> String {
    let s = s.replace('►', "");
    let s = s.trim();
    if s.chars().count() > QUOTE {
        let cut: String = s.chars().take(QUOTE).collect();
        format!("{}…", cut.trim_end())
    } else {
        s.to_string()
    }
}

// ---- the database ---------------------------------------------------------------------------

/// `history.sqlite`, open read-only.
pub struct History {
    conn: Connection,
}

impl History {
    pub fn open(data_dir: Option<&Path>) -> Option<History> {
        let path = find_history_db(data_dir)?;
        let conn = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .ok()?;
        History::check(conn)
    }

    /// A file that is not a history database is no history: no notes rather than an error at
    /// every word.
    fn check(conn: Connection) -> Option<History> {
        conn.prepare("SELECT last, sources, meanings FROM versions WHERE word = ''")
            .ok()?;
        conn.prepare("SELECT old, new, first_new FROM spellings WHERE new = ''")
            .ok()?;
        Some(History { conn })
    }

    fn versions(&self, word: &str) -> Vec<Version> {
        let Ok(mut st) = self
            .conn
            .prepare("SELECT last, sources, meanings FROM versions WHERE word = ?1")
        else {
            return Vec::new();
        };
        let Ok(rows) = st.query_map([word], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        }) else {
            return Vec::new();
        };
        rows.flatten()
            .map(|(last, sources, meanings)| {
                let texts: Vec<String> = serde_json::from_str(&meanings).unwrap_or_default();
                let plain = texts.iter().map(|t| plain(t)).collect();
                Version {
                    last,
                    sources,
                    texts,
                    plain,
                }
            })
            .collect()
    }

    /// (old, first_new) of each spelling row whose `new` is `word`.
    fn spelled_before(&self, word: &str) -> Vec<(String, String)> {
        let Ok(mut st) = self
            .conn
            .prepare("SELECT old, first_new FROM spellings WHERE new = ?1 ORDER BY first_new")
        else {
            return Vec::new();
        };
        st.query_map([word], |r| Ok((r.get(0)?, r.get(1)?)))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    /// The notes for the entries open under `word`: at most four lines, the last one counting what
    /// did not fit.
    pub fn notes(&self, word: &str, entries: &[Entry]) -> Vec<Note> {
        let mut names = vec![word.to_string()];
        for e in entries {
            if !names.contains(&e.headword) {
                names.push(e.headword.clone());
            }
        }
        let mut out = Vec::new();
        let mut olds: Vec<String> = Vec::new();
        for n in &names {
            for (old, first_new) in self.spelled_before(n) {
                if olds.contains(&old) {
                    continue;
                }
                let how = match (old.contains(' '), n.contains(' ')) {
                    (true, false) => Respelt::Apart,
                    (false, true) => Respelt::Together,
                    _ => Respelt::Other,
                };
                out.push(Note::Spelling {
                    how,
                    old: old.clone(),
                    year: year(&first_new),
                });
                olds.push(old);
            }
        }
        // The old spellings' forms count as the word's own.
        let mut hist: Vec<Version> = names
            .iter()
            .chain(&olds)
            .flat_map(|n| self.versions(n))
            .collect();
        hist.sort_by(|a, b| a.last.cmp(&b.last));
        out.extend(meaning_notes(entries, &hist));
        if out.len() > MAX_NOTES {
            let more = out.len() - (MAX_NOTES - 1);
            out.truncate(MAX_NOTES - 1);
            out.push(Note::More(more));
        }
        out
    }

    /// For a word TDK does not have: the spelling that replaced it, if that is a headword.
    pub fn old_spelling(
        &self,
        typed: &str,
        is_headword: impl Fn(&str) -> bool,
    ) -> Option<OldSpelling> {
        let l = lower(typed);
        let mut st = self
            .conn
            .prepare("SELECT old, new, first_new FROM spellings WHERE old = ?1 OR old = ?2")
            .ok()?;
        let rows: Vec<(String, String, String)> = st
            .query_map([typed, l.as_str()], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .ok()?
            .flatten()
            .collect();
        let mut seen = HashSet::new();
        rows.into_iter()
            .filter(|(old, new, _)| lower(old) == l && seen.insert(new.clone()))
            .find(|(_, new, _)| is_headword(new))
            .map(|(_, new, first_new)| OldSpelling {
                typed: typed.to_string(),
                new,
                year: year(&first_new),
            })
    }
}

/// What the Database tab says when there is no history database.
pub fn missing_text() -> &'static str {
    tl(
        "history.sqlite bulunamadı; geçmiş notları gösterilmez. Dosyayı sozluk.exe'nin yanına ya da %LOCALAPPDATA%\\sozluk klasörüne koyun ya da SOZLUK_HISTORY'yi ayarlayın.",
        "No history.sqlite found, so there are no history notes. Put the file next to sozluk.exe or in %LOCALAPPDATA%\\sozluk, or set SOZLUK_HISTORY.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dict::Meaning;
    use crate::lang::{self, Lang};
    use std::sync::{Mutex, MutexGuard};

    /// The interface language is global: a test that names one holds this, and leaves Turkish.
    static LANG: Mutex<()> = Mutex::new(());

    struct InLang(#[allow(dead_code)] MutexGuard<'static, ()>);

    impl Drop for InLang {
        fn drop(&mut self) {
            lang::set(Lang::Tr);
        }
    }

    fn english() -> InLang {
        let g = InLang(LANG.lock().unwrap_or_else(|e| e.into_inner()));
        lang::set(Lang::En);
        g
    }

    fn db(versions: &[(&str, &str, &str, &str, &[&str])], spellings: &[[&str; 4]]) -> History {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE versions(word TEXT, first TEXT, last TEXT, sources TEXT, meanings TEXT);
             CREATE TABLE spellings(old TEXT, new TEXT, last_old TEXT, first_new TEXT);",
        )
        .unwrap();
        for (w, first, last, src, ms) in versions {
            c.execute(
                "INSERT INTO versions VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![w, first, last, src, serde_json::to_string(ms).unwrap()],
            )
            .unwrap();
        }
        for s in spellings {
            c.execute("INSERT INTO spellings VALUES (?1, ?2, ?3, ?4)", s)
                .unwrap();
        }
        History::check(c).unwrap()
    }

    fn entry(word: &str, ms: &[&str]) -> Entry {
        Entry {
            headword: word.to_string(),
            meanings: ms
                .iter()
                .map(|t| Meaning {
                    text: t.to_string(),
                    ..Meaning::default()
                })
                .collect(),
            ..Entry::default()
        }
    }

    fn lines(h: &History, word: &str, es: &[Entry]) -> Vec<String> {
        h.notes(word, es).iter().map(Note::text).collect()
    }

    const A: &str = "Yunan alfabesinin birinci harfi";
    const B: &str = "Kuzey Afrika'da yetişen bir bitki";
    const C: &str = "Herhangi bir toplulukta çeşitli özellikleri olan";
    const D: &str = "Helyum atomunun çekirdeği";

    #[test]
    fn meanings_added_after_a_year() {
        let _lang = english();
        let h = db(
            &[
                ("alfa", "2019-06-06", "2019-06-06", "copy 2019", &[A, B]),
                (
                    "alfa",
                    "2023-07-23",
                    "2024-03-29",
                    "copy 2023,copy 2024",
                    &[A, B, C, D],
                ),
            ],
            &[],
        );
        let es = [entry("alfa", &[A, B, C, D])];
        assert_eq!(
            lines(&h, "alfa", &es),
            ["Meanings 3 and 4 were added after 2019."]
        );
        lang::set(Lang::Tr);
        let tr: Vec<String> = h.notes("alfa", &es).iter().map(Note::text).collect();
        assert_eq!(tr, ["3. ve 4. anlamlar 2019'dan sonra eklendi."]);
        lang::set(Lang::En);
        let es = [entry("alfa", &[A, B, C])];
        assert_eq!(
            lines(&h, "alfa", &es),
            [
                "Meaning 3 was added after 2019.",
                "A meaning was removed after 2024: “Helyum atomunun çekirdeği”."
            ]
        );
    }

    #[test]
    fn a_meaning_that_went_is_told_once() {
        let _lang = english();
        let two = "Dirsekten orta parmak ucuna kadar olan uzunluk ölçüsü, arşın ve başka bir şey";
        let h = db(
            &[
                (
                    "zira",
                    "2007",
                    "2007",
                    "copy 2007",
                    &[two, "Çünkü, şundan dolayı"],
                ),
                (
                    "zira",
                    "2010-04-15",
                    "2024-03-29",
                    "Wayback,copy 2019",
                    &["Çünkü"],
                ),
            ],
            &[],
        );
        let es = [entry("zira", &["Çünkü"])];
        assert_eq!(
            lines(&h, "zira", &es),
            [
                "A meaning was removed after 2007: “Dirsekten orta parmak ucuna kadar olan uzunluk ölçüsü, arşın…”."
            ]
        );
        lang::set(Lang::Tr);
        assert!(
            h.notes("zira", &es)[0]
                .text()
                .starts_with("2007'den sonra bir anlam")
        );
    }

    #[test]
    fn a_copy_of_the_site_cannot_show_a_removal() {
        let _lang = english();
        let h = db(
            &[(
                "x",
                "2016",
                "2016",
                "Wayback",
                &["Bir şey", "Başka bir şey"],
            )],
            &[],
        );
        assert!(lines(&h, "x", &[entry("x", &["Bir şey"])]).is_empty());
    }

    #[test]
    fn a_spelling_brings_its_old_forms() {
        let _lang = english();
        let old = "İçerisindeki bina ve donatımlar";
        let h = db(
            &[
                ("hava alanı", "2007", "2007", "copy 2007", &[old]),
                ("havaalanı", "2019-06-06", "2024-03-29", "copy 2019", &[old]),
            ],
            &[["hava alanı", "havaalanı", "2007", "2019-06-01"]],
        );
        let es = [entry("havaalanı", &[old])];
        assert_eq!(
            lines(&h, "havaalanı", &es),
            ["Written as two words until 2019: “hava alanı”."]
        );
        lang::set(Lang::Tr);
        assert_eq!(
            h.notes("havaalanı", &es)[0].text(),
            "2019'a kadar ayrı yazılıyordu: “hava alanı”."
        );
        let h = db(
            &[],
            &[
                ["yeşilbiber", "yeşil biber", "2007", "2019-06-01"],
                ["unvan", "ünvan", "2007", "2010"],
            ],
        );
        lang::set(Lang::En);
        assert_eq!(
            lines(&h, "yeşil biber", &[]),
            ["Written as one word until 2019: “yeşilbiber”."]
        );
        assert_eq!(lines(&h, "ünvan", &[]), ["Written “unvan” until 2010."]);
    }

    #[test]
    fn a_reworded_meaning() {
        let _lang = english();
        let before = "Yakmayacak derecede ısısı olan, yakmayacak kadar sıcak";
        let after = "Yakmayacak derecede ısısı olan, yakmayacak biçimde sıcak";
        let h = db(
            &[(
                "sıcak",
                "2007",
                "2024-03-29",
                "copy 2007,copy 2024",
                &[before, A],
            )],
            &[],
        );
        let es = [entry("sıcak", &[after, A])];
        assert_eq!(
            lines(&h, "sıcak", &es),
            ["Meaning 1 was reworded after 2024."]
        );
    }

    #[test]
    fn a_word_without_history_has_no_notes() {
        let _lang = english();
        let h = db(&[("alfa", "2019", "2024", "copy 2019", &[A])], &[]);
        assert!(lines(&h, "beta", &[entry("beta", &[A])]).is_empty());
        // Nor does an entry that has not changed.
        assert!(lines(&h, "alfa", &[entry("alfa", &[A])]).is_empty());
    }

    #[test]
    fn more_than_four_lines_are_counted() {
        let _lang = english();
        let h = db(
            &[
                ("w", "2007", "2007", "copy 2007", &[A]),
                ("w", "2019", "2019", "copy 2019", &[A, B]),
            ],
            &[
                ["w1", "w", "2000", "2008"],
                ["w2", "w", "2000", "2009"],
                ["w3", "w", "2000", "2010"],
            ],
        );
        let got = lines(&h, "w", &[entry("w", &[A, B, C])]);
        assert_eq!(got.len(), MAX_NOTES);
        assert_eq!(got[3], "…and 2 more changes.");
    }

    #[test]
    fn an_old_spelling_points_to_the_new_one() {
        let _lang = english();
        let h = db(&[], &[["yeşilbiber", "yeşil biber", "2007", "2019-06-01"]]);
        let got = h
            .old_spelling("Yeşilbiber", |w| w == "yeşil biber")
            .unwrap();
        lang::set(Lang::En);
        assert_eq!(
            got.text(),
            "“Yeşilbiber” was written this way until 2019; today: yeşil biber"
        );
        lang::set(Lang::Tr);
        assert_eq!(
            got.text(),
            "“Yeşilbiber” 2019'a kadar böyle yazılıyordu; bugün: yeşil biber"
        );
        assert!(h.old_spelling("yeşilbiber", |_| false).is_none());
    }

    #[test]
    fn meanings_are_compared_plainly() {
        assert_eq!(plain("► Hâlâ, (bk.) İyi-KÖTÜ!"), "hala iyi kötü");
        assert!(dice("yakmayacak sıcak", "yakmayacak sıcak") > 0.99);
        assert!(dice("kuzey afrika", "helyum atomu") < SIMILAR);
        assert_eq!(dice("a", "b"), 0.0);
    }
}
