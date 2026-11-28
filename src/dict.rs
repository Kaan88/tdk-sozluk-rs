//! The headword index (built into the program) and the full entries (TDK's own answers, kept in
//! SQLite: the crawl's `gts.sqlite`, read-only, and this app's cache of live answers).

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::Value;

use crate::turkish::{fold, lower, search_key};

const HEADWORDS: &str = include_str!("../data/gts-headwords.txt");

/// Every headword of the Güncel Türkçe Sözlük and the ways to find one.
pub struct Index {
    pub words: Vec<String>,
    /// (search key, word index), sorted by key: prefix search by binary search.
    by_key: Vec<(String, u32)>,
    /// Lowercase form → indices (`Olimpiyat` finds `olimpiyat`).
    by_lower: HashMap<String, Vec<u32>>,
    /// Folded form → indices (`kağıt` finds `kâğıt`, `yeraltı` finds `yer altı` too).
    by_fold: HashMap<String, Vec<u32>>,
    /// Verb stems (`görün` for `görünmek`), for the line check's stem search.
    stems: HashSet<String>,
    pub fetched: String,
    pub multiword: usize,
}

impl Index {
    pub fn load() -> Index {
        let mut fetched = String::new();
        let mut words = Vec::new();
        for line in HEADWORDS.lines() {
            if let Some(h) = line.strip_prefix('#') {
                if let Some(at) = h.find("fetched ") {
                    fetched = h[at + 8..]
                        .split(',')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string();
                }
                continue;
            }
            if !line.trim().is_empty() {
                words.push(line.trim().to_string());
            }
        }
        let mut by_key = Vec::with_capacity(words.len());
        let mut by_lower: HashMap<String, Vec<u32>> = HashMap::with_capacity(words.len());
        let mut by_fold: HashMap<String, Vec<u32>> = HashMap::with_capacity(words.len());
        let mut stems = HashSet::new();
        let mut multiword = 0;
        for (i, w) in words.iter().enumerate() {
            let i = i as u32;
            by_key.push((search_key(w), i));
            by_lower.entry(lower(w)).or_default().push(i);
            by_fold.entry(fold(w)).or_default().push(i);
            if w.contains(' ') {
                multiword += 1;
            } else if let Some(s) = w.strip_suffix("mek").or_else(|| w.strip_suffix("mak"))
                && s.chars().count() >= 2
            {
                stems.insert(lower(s));
            }
        }
        by_key.sort();
        Index {
            words,
            by_key,
            by_lower,
            by_fold,
            stems,
            fetched,
            multiword,
        }
    }

    pub fn len(&self) -> usize {
        self.words.len()
    }

    /// The headwords spelled exactly so, ignoring case.
    pub fn exact(&self, w: &str) -> Vec<&str> {
        self.by_lower
            .get(&lower(w))
            .map(|v| v.iter().map(|&i| self.words[i as usize].as_str()).collect())
            .unwrap_or_default()
    }

    pub fn contains(&self, w: &str) -> bool {
        self.by_lower.contains_key(&lower(w))
    }

    pub fn is_stem(&self, w: &str) -> bool {
        self.stems.contains(&lower(w))
    }

    /// Headwords that differ from `w` only by circumflex, case, spacing or hyphen (itself left
    /// out).
    pub fn variants(&self, w: &str) -> Vec<&str> {
        let l = lower(w);
        self.by_fold
            .get(&fold(w))
            .map(|v| {
                v.iter()
                    .map(|&i| self.words[i as usize].as_str())
                    .filter(|x| lower(x) != l)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Search: the words whose folded form is the query's first, then those that start with it,
    /// shortest first, up to `limit`.
    pub fn search(&self, query: &str, limit: usize) -> Vec<&str> {
        let q = search_key(query.trim());
        if q.is_empty() {
            return Vec::new();
        }
        let mut out: Vec<u32> = Vec::new();
        let mut seen = HashSet::new();
        if let Some(v) = self.by_fold.get(&fold(query.trim())) {
            // The spelling typed first, then the other spellings.
            let typed = lower(query.trim());
            let mut v = v.clone();
            v.sort_by_key(|&i| (lower(&self.words[i as usize]) != typed, i));
            for &i in &v {
                if seen.insert(i) {
                    out.push(i);
                }
            }
        }
        let start = self
            .by_key
            .partition_point(|(k, _)| k.as_str() < q.as_str());
        let mut prefixed: Vec<u32> = self.by_key[start..]
            .iter()
            .take_while(|(k, _)| k.starts_with(&q))
            .take(2000)
            .map(|&(_, i)| i)
            .filter(|i| !seen.contains(i))
            .collect();
        prefixed.sort_by_key(|&i| (self.words[i as usize].chars().count(), i));
        out.extend(prefixed);
        out.truncate(limit);
        out.into_iter()
            .map(|i| self.words[i as usize].as_str())
            .collect()
    }
}

// ---- entries ----------------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct Meaning {
    pub tags: Vec<String>,
    pub text: String,
    pub example: Option<(String, Option<String>)>,
}

#[derive(Clone, Debug, Default)]
pub struct Entry {
    pub headword: String,
    pub origin: String,
    pub pronunciation: String,
    pub meanings: Vec<Meaning>,
    pub compounds: Vec<String>,
    pub idioms: Vec<String>,
}

fn s(v: &Value, k: &str) -> String {
    v.get(k)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

/// TDK's `gts` answer (a list of entries, or `{"error": …}`) as entries.
pub fn parse(json: &str) -> Option<Vec<Entry>> {
    let v: Value = serde_json::from_str(json).ok()?;
    let list = v.as_array()?;
    Some(
        list.iter()
            .map(|e| Entry {
                headword: s(e, "madde"),
                origin: s(e, "lisan"),
                pronunciation: s(e, "telaffuz"),
                meanings: e
                    .get("anlamlarListe")
                    .and_then(Value::as_array)
                    .map(|ms| {
                        ms.iter()
                            .map(|m| Meaning {
                                tags: m
                                    .get("ozelliklerListe")
                                    .and_then(Value::as_array)
                                    .map(|os| {
                                        os.iter()
                                            .map(|o| s(o, "tam_adi"))
                                            .filter(|t| !t.is_empty())
                                            .collect()
                                    })
                                    .unwrap_or_default(),
                                text: s(m, "anlam"),
                                example: m
                                    .get("orneklerListe")
                                    .and_then(Value::as_array)
                                    .and_then(|xs| xs.first())
                                    .map(|x| {
                                        let author = x
                                            .get("yazar")
                                            .and_then(Value::as_array)
                                            .and_then(|a| a.first())
                                            .map(|a| s(a, "tam_adi"))
                                            .filter(|a| !a.is_empty());
                                        (s(x, "ornek"), author)
                                    }),
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                compounds: s(e, "birlesikler")
                    .split(',')
                    .map(|c| c.trim().to_string())
                    .filter(|c| !c.is_empty())
                    .collect(),
                idioms: e
                    .get("atasozu")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .map(|x| s(x, "madde"))
                            .filter(|x| !x.is_empty())
                            .collect()
                    })
                    .unwrap_or_default(),
            })
            .collect(),
    )
}

/// What the stores know of a word.
#[derive(Clone, Debug)]
pub enum Lookup {
    Found(Vec<Entry>),
    /// TDK answered and has no entry under this form.
    NotFound,
    /// Not downloaded yet.
    Missing,
}

/// The crawl's database (read-only) and this app's cache (read-write).
pub struct Store {
    crawl: Option<Connection>,
    pub crawl_path: Option<PathBuf>,
    cache: Option<Connection>,
    pub cache_path: Option<PathBuf>,
}

fn inflate(blob: &[u8]) -> Option<String> {
    let mut out = String::new();
    flate2::read::ZlibDecoder::new(blob)
        .read_to_string(&mut out)
        .ok()?;
    Some(out)
}

/// Where the crawl's `gts.sqlite` may be: `$SOZLUK_DB`, beside the program, in the app's data
/// folder.
pub fn find_crawl_db(data_dir: Option<&Path>) -> Option<PathBuf> {
    let mut c: Vec<PathBuf> = Vec::new();
    if let Ok(p) = std::env::var("SOZLUK_DB") {
        c.push(PathBuf::from(p));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(d) = exe.parent()
    {
        c.push(d.join("gts.sqlite"));
    }
    if let Some(d) = data_dir {
        c.push(d.join("gts.sqlite"));
    }
    c.into_iter().find(|p| p.is_file())
}

impl Store {
    pub fn open(data_dir: Option<&Path>) -> Store {
        let crawl_path = find_crawl_db(data_dir);
        let crawl = crawl_path.as_ref().and_then(|p| {
            Connection::open_with_flags(
                p,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )
            .ok()
        });
        let cache_path = data_dir.map(|d| d.join("live-cache.sqlite"));
        let cache = cache_path.as_ref().and_then(|p| {
            let _ = std::fs::create_dir_all(p.parent()?);
            let c = Connection::open(p).ok()?;
            c.execute_batch(
                "CREATE TABLE IF NOT EXISTS gts(word TEXT PRIMARY KEY, status TEXT, json TEXT, fetched TEXT)",
            )
            .ok()?;
            Some(c)
        });
        Store {
            crawl,
            crawl_path,
            cache,
            cache_path,
        }
    }

    /// A store with nothing open: what the app holds while it replaces `gts.sqlite`, which
    /// Windows does not let it rename while a connection has it open.
    pub fn closed() -> Store {
        Store {
            crawl: None,
            crawl_path: None,
            cache: None,
            cache_path: None,
        }
    }

    pub fn has_crawl(&self) -> bool {
        self.crawl.is_some()
    }

    fn in_crawl(&self, w: &str) -> Option<(String, Option<String>)> {
        let c = self.crawl.as_ref()?;
        let row: Option<(String, Vec<u8>)> = c
            .query_row("SELECT status, data FROM gts WHERE word = ?1", [w], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()
            .ok()?;
        let (status, data) = row?;
        Some((status, inflate(&data)))
    }

    fn in_cache(&self, w: &str) -> Option<(String, Option<String>)> {
        let c = self.cache.as_ref()?;
        c.query_row("SELECT status, json FROM gts WHERE word = ?1", [w], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()
        .ok()?
    }

    /// The entries for one headword. TDK's search ignores circumflexes, so `hala` brings `hâlâ`
    /// too: only the entries spelled as asked are kept, unless none are.
    pub fn lookup(&self, w: &str) -> Lookup {
        for got in [self.in_crawl(w), self.in_cache(w)] {
            match got {
                Some((status, Some(json))) if status == "ok" => {
                    if let Some(mut es) = parse(&json) {
                        let l = lower(w);
                        if es.iter().any(|e| lower(&e.headword) == l) {
                            es.retain(|e| lower(&e.headword) == l);
                        }
                        return Lookup::Found(es);
                    }
                }
                Some((status, _)) if status == "notfound" => return Lookup::NotFound,
                _ => {}
            }
        }
        Lookup::Missing
    }

    /// Keeps a live answer.
    pub fn remember(&self, w: &str, json: Option<&str>) {
        if let Some(c) = &self.cache {
            let status = match json.map(parse) {
                Some(Some(_)) => "ok",
                _ => "notfound",
            };
            let _ = c.execute(
                "INSERT OR REPLACE INTO gts VALUES (?1, ?2, ?3, datetime('now'))",
                rusqlite::params![w, status, json],
            );
        }
    }

    /// Counts in the crawl's database: (found, not found, to retry).
    pub fn crawl_counts(&self) -> Option<(u64, u64, u64)> {
        let c = self.crawl.as_ref()?;
        let mut st = c
            .prepare("SELECT status, COUNT(*) FROM gts GROUP BY status")
            .ok()?;
        let mut out = (0, 0, 0);
        let rows = st
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?.max(0) as u64))
            })
            .ok()?;
        for (s, n) in rows.flatten() {
            match s.as_str() {
                "ok" => out.0 = n,
                "notfound" => out.1 = n,
                _ => out.2 += n,
            }
        }
        Some(out)
    }

    pub fn cache_count(&self) -> u64 {
        self.cache
            .as_ref()
            .and_then(|c| {
                c.query_row("SELECT COUNT(*) FROM gts", [], |r| r.get::<_, i64>(0))
                    .ok()
                    .map(|n| n.max(0) as u64)
            })
            .unwrap_or(0)
    }
}

/// One live request to sozluk.gov.tr (blocking; run it on a worker).
pub fn fetch_live(w: &str) -> Result<Option<String>, String> {
    let url = format!("https://sozluk.gov.tr/gts?ara={}", encode(w));
    let resp = ureq::get(&url)
        .set("User-Agent", "Mozilla/5.0")
        .timeout(std::time::Duration::from_secs(20))
        .call();
    match resp {
        Ok(r) => {
            let body = r.into_string().map_err(|e| e.to_string())?;
            Ok(if body.trim_start().starts_with('[') {
                Some(body)
            } else {
                None
            })
        }
        Err(ureq::Error::Status(code, _)) => Err(format!("HTTP {code}")),
        Err(e) => Err(e.to_string()),
    }
}

fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_finds_variants() {
        let ix = Index::load();
        assert!(ix.len() > 80_000);
        assert!(ix.contains("akılalmaz"));
        assert_eq!(ix.variants("kağıt"), vec!["kâğıt"]);
        assert!(ix.variants("yeraltı").contains(&"yer altı"));
        assert_eq!(ix.search("yeraltı", 5)[0], "yeraltı");
        assert!(ix.search("kağıt", 3).contains(&"kâğıt"));
        assert_eq!(ix.search("kagit", 3)[0], "kâğıt");
        assert!(ix.is_stem("görün"));
    }

    #[test]
    fn parse_entry() {
        let json = r#"[{"madde":"yeraltı","lisan":"","telaffuz":"ye'raltı","birlesikler":"yeraltı dünyası",
            "anlamlarListe":[{"anlam":"Gizli ve yasa dışı","ozelliklerListe":[{"tam_adi":"sıfat"},{"tam_adi":"mecaz"}],
            "orneklerListe":[{"ornek":"Yeraltı faaliyet.","yazar":[]}]},{"anlam":"Alışılmışın dışında olan, aykırı olan"}]}]"#;
        let es = parse(json).unwrap();
        assert_eq!(es[0].meanings.len(), 2);
        assert_eq!(es[0].meanings[0].tags, vec!["sıfat", "mecaz"]);
        assert_eq!(es[0].compounds, vec!["yeraltı dünyası"]);
        assert!(parse(r#"{"error":"Sonuç bulunamadı"}"#).is_none());
        assert_eq!(encode("şu an"), "%C5%9Fu%20an");
    }
}
