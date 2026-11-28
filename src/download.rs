//! The offline dictionary: the full entries as one SQLite file (`gts.sqlite`), published by the
//! crawl's server and kept current there. This module asks the server what its newest snapshot
//! is, downloads it, checks it and puts it in the app's data folder, keeping the file before it
//! as `gts.sqlite.prev`.
//!
//! The work is split so that nothing blocks the window and no open file is replaced: [`run`] is
//! the worker (it asks, downloads and checks, and leaves a checked file as `gts.sqlite.new`), and
//! [`install`] puts that file in place. The caller closes its own connection to `gts.sqlite`
//! first, because Windows does not rename a file SQLite holds open. A checked file that could not
//! be put in place waits as `gts.sqlite.new` and [`install_pending`] puts it in place at the next
//! start, before the database is opened.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

/// The crawl's server (through a Cloudflare tunnel): `status.json` and `gts.sqlite.gz`.
pub const BASE: &str = "https://sozluk.kweb.ovh";
/// The most an unpacked snapshot may be: a broken or hostile file stops here, not at a full disk.
const MAX_UNPACKED: u64 = 1 << 30;

/// Every request carries the program's own name; the server answers nothing else.
pub fn user_agent() -> String {
    format!("Sozluk/{}", env!("CARGO_PKG_VERSION"))
}

/// One published snapshot: when it was written and the SHA-256 of the unpacked database.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub written: String,
    pub sha256: String,
}

/// What the worker reports, in order: `Checking`, then `UpToDate`, or `Progress` (bytes of the
/// download so far and in all, when the server said) until `Ready`; `Failed` at any point.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Checking,
    UpToDate(Snapshot),
    Progress(u64, Option<u64>),
    /// Downloaded and checked: `gts.sqlite.new` waits for [`install`].
    Ready(Snapshot),
    Failed(String),
}

/// Reads a `status.json`. `None` unless it has a `written` text and a 64-digit hex `sha256`.
pub fn parse_status(json: &str) -> Option<Snapshot> {
    let v: Value = serde_json::from_str(json).ok()?;
    let sha256 = v.get("sha256")?.as_str()?.trim().to_ascii_lowercase();
    if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let written = v.get("written")?.as_str()?.to_string();
    Some(Snapshot { written, sha256 })
}

fn record(snap: &Snapshot) -> String {
    serde_json::json!({ "written": snap.written, "sha256": snap.sha256 }).to_string()
}

fn read_record(path: &Path) -> Option<Snapshot> {
    parse_status(&std::fs::read_to_string(path).ok()?)
}

fn paths(dir: &Path) -> (PathBuf, PathBuf, PathBuf) {
    (
        dir.join("gts.sqlite"),
        dir.join("gts.sqlite.new"),
        dir.join("gts.sqlite.prev"),
    )
}

/// The snapshot in place, as recorded when it was installed (`gts-snapshot.json`).
pub fn installed(dir: &Path) -> Option<Snapshot> {
    dir.join("gts.sqlite")
        .is_file()
        .then(|| read_record(&dir.join("gts-snapshot.json")))?
}

/// A checked snapshot waiting as `gts.sqlite.new`, if there is one.
pub fn pending(dir: &Path) -> Option<Snapshot> {
    let (_, new, _) = paths(dir);
    new.is_file()
        .then(|| read_record(&dir.join("gts.sqlite.new.json")))?
}

/// Puts the waiting `gts.sqlite.new` in place: `gts.sqlite` becomes `gts.sqlite.prev` (the one
/// before is deleted) and the new file `gts.sqlite`. If the second step fails, the first is
/// undone. Nothing may hold `gts.sqlite` open.
pub fn install(dir: &Path) -> Result<Snapshot, String> {
    let snap = pending(dir).ok_or("no downloaded dictionary is waiting")?;
    let (db, new, prev) = paths(dir);
    if db.exists() {
        let _ = std::fs::remove_file(&prev);
        std::fs::rename(&db, &prev).map_err(|e| e.to_string())?;
    }
    if let Err(e) = std::fs::rename(&new, &db) {
        if prev.exists() && !db.exists() {
            let _ = std::fs::rename(&prev, &db);
        }
        return Err(e.to_string());
    }
    let _ = std::fs::write(dir.join("gts-snapshot.json"), record(&snap));
    let _ = std::fs::remove_file(dir.join("gts.sqlite.new.json"));
    Ok(snap)
}

/// [`install`] for a file left waiting by an earlier run; call before opening the database.
pub fn install_pending(dir: &Path) -> Option<Snapshot> {
    pending(dir).and_then(|_| install(dir).ok())
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .user_agent(&user_agent())
        .timeout_connect(Duration::from_secs(20))
        .timeout_read(Duration::from_secs(60))
        .build()
}

fn status_of(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, _) => format!("HTTP {code}"),
        e => e.to_string(),
    }
}

/// Asks the server for its newest snapshot (blocking).
pub fn remote() -> Result<Snapshot, String> {
    let body = agent()
        .get(&format!("{BASE}/status.json"))
        .call()
        .map_err(status_of)?
        .into_string()
        .map_err(|e| e.to_string())?;
    parse_status(&body).ok_or_else(|| "the server's status is not readable".to_string())
}

/// Counts the bytes read through it.
struct Counting<R, F> {
    inner: R,
    read: u64,
    tell: F,
}

impl<R: Read, F: FnMut(u64)> Read for Counting<R, F> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read += n as u64;
        (self.tell)(self.read);
        Ok(n)
    }
}

/// Unpacks a gzip'd database from `gz` and checks that its SHA-256 is `sha256`. Gives the
/// database's bytes, or why not: not gzip, larger than `max`, or another checksum.
pub fn unpack(gz: impl Read, sha256: &str, max: u64) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(gz)
        .take(max + 1)
        .read_to_end(&mut out)
        .map_err(|e| format!("not a gzip'd file: {e}"))?;
    if out.len() as u64 > max {
        return Err("the file is larger than a dictionary can be".into());
    }
    if mdshared_app::update::sha256_hex(&out) != sha256 {
        return Err("the file's checksum is not the one published".into());
    }
    Ok(out)
}

/// Whether the file at `path` is a crawl database SQLite can read: `quick_check` passes and its
/// `gts` table has found entries.
pub fn check_db(path: &Path) -> Result<(), String> {
    use rusqlite::{Connection, OpenFlags};
    let c = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| e.to_string())?;
    let ok: String = c
        .query_row("PRAGMA quick_check", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if ok != "ok" {
        return Err(format!("the database is damaged ({ok})"));
    }
    let found: i64 = c
        .query_row("SELECT COUNT(*) FROM gts WHERE status = 'ok'", [], |r| {
            r.get(0)
        })
        .map_err(|e| e.to_string())?;
    if found == 0 {
        return Err("the database has no entries".into());
    }
    Ok(())
}

/// Keeps `bytes` (a checked database) as `gts.sqlite.new` beside a record of `snap`, written
/// under another name and renamed, so a half-written file is never taken for a finished one.
pub fn keep(dir: &Path, bytes: &[u8], snap: &Snapshot) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let (_, new, _) = paths(dir);
    let part = dir.join("gts.sqlite.download");
    std::fs::write(&part, bytes).map_err(|e| e.to_string())?;
    if let Err(e) = check_db(&part) {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    std::fs::rename(&part, &new).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("gts.sqlite.new.json"), record(snap)).map_err(|e| e.to_string())
}

/// The worker: asks for the newest snapshot and, unless it is the one installed (or already
/// waiting), downloads, checks and keeps it. Reports through `send`; stops early if `send` says
/// the window is gone (returns false).
pub fn run(dir: &Path, send: &dyn Fn(Event) -> bool) {
    if !send(Event::Checking) {
        return;
    }
    let done = match fetch(dir, send) {
        Ok(e) => e,
        Err(e) => Event::Failed(e),
    };
    send(done);
}

fn fetch(dir: &Path, send: &dyn Fn(Event) -> bool) -> Result<Event, String> {
    let snap = remote()?;
    if installed(dir).as_ref() == Some(&snap) {
        return Ok(Event::UpToDate(snap));
    }
    if pending(dir).as_ref() == Some(&snap) {
        return Ok(Event::Ready(snap));
    }
    let resp = agent()
        .get(&format!("{BASE}/gts.sqlite.gz"))
        .call()
        .map_err(status_of)?;
    let total = resp
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok());
    let mut told = 0u64;
    let counting = Counting {
        inner: resp.into_reader(),
        read: 0,
        tell: |n: u64| {
            // A report every half megabyte is plenty for a progress bar.
            if n >= told + (1 << 19) {
                told = n;
                send(Event::Progress(n, total));
            }
        },
    };
    let bytes = unpack(counting, &snap.sha256, MAX_UNPACKED)?;
    keep(dir, &bytes, &snap)?;
    Ok(Event::Ready(snap))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sozluk-download-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn gz(data: &[u8]) -> Vec<u8> {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    fn sha(data: &[u8]) -> String {
        mdshared_app::update::sha256_hex(data)
    }

    fn crawl_db(path: &Path, words: &[&str]) -> Vec<u8> {
        let c = rusqlite::Connection::open(path).unwrap();
        c.execute_batch(
            "CREATE TABLE gts(word TEXT PRIMARY KEY, status TEXT, data BLOB, fetched TEXT)",
        )
        .unwrap();
        for w in words {
            c.execute("INSERT INTO gts VALUES (?1, 'ok', x'00', 'dump')", [w])
                .unwrap();
        }
        drop(c);
        std::fs::read(path).unwrap()
    }

    #[test]
    fn a_status_needs_a_written_time_and_a_full_hex_checksum() {
        let good = format!(
            r#"{{"written":"2026-11-28 04:35:54","sha256":"{}","counts":{{}}}}"#,
            "AB".repeat(32)
        );
        assert_eq!(
            parse_status(&good),
            Some(Snapshot {
                written: "2026-11-28 04:35:54".into(),
                sha256: "ab".repeat(32)
            })
        );
        for bad in [
            "",
            "[]",
            r#"{"written":"x"}"#,
            r#"{"written":"x","sha256":"abc"}"#,
            &format!(r#"{{"written":"x","sha256":"{}"}}"#, "zz".repeat(32)),
            &format!(r#"{{"sha256":"{}"}}"#, "ab".repeat(32)),
        ] {
            assert_eq!(parse_status(bad), None, "{bad}");
        }
    }

    #[test]
    fn unpacking_gives_the_bytes_only_when_the_checksum_and_size_are_right() {
        let data = b"SQLite format 3\0 and the rest".to_vec();
        assert_eq!(
            unpack(&gz(&data)[..], &sha(&data), 1 << 20),
            Ok(data.clone())
        );
        assert!(unpack(&gz(&data)[..], &sha(b"other"), 1 << 20).is_err());
        assert!(unpack(&gz(&data)[..], &sha(&data), 4).is_err());
        assert!(unpack(&data[..], &sha(&data), 1 << 20).is_err());
        assert!(unpack(&[][..], &sha(&data), 1 << 20).is_err());
    }

    #[test]
    fn a_kept_download_is_checked_installed_and_the_old_file_kept_once() {
        let d = temp("install");
        let first = crawl_db(&d.join("first.sqlite"), &["ev"]);
        let second = crawl_db(&d.join("second.sqlite"), &["ev", "kitap"]);
        let s1 = Snapshot {
            written: "1".into(),
            sha256: sha(&first),
        };
        let s2 = Snapshot {
            written: "2".into(),
            sha256: sha(&second),
        };

        assert_eq!(installed(&d), None);
        keep(&d, &first, &s1).unwrap();
        assert_eq!(pending(&d), Some(s1.clone()));
        assert_eq!(install(&d), Ok(s1.clone()));
        assert_eq!(installed(&d), Some(s1.clone()));
        assert_eq!(pending(&d), None);
        assert!(!d.join("gts.sqlite.prev").exists());

        keep(&d, &second, &s2).unwrap();
        assert_eq!(install_pending(&d), Some(s2.clone()));
        assert_eq!(std::fs::read(d.join("gts.sqlite")).unwrap(), second);
        assert_eq!(std::fs::read(d.join("gts.sqlite.prev")).unwrap(), first);
        assert_eq!(installed(&d), Some(s2));
        assert_eq!(
            install(&d),
            Err("no downloaded dictionary is waiting".into())
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_file_that_is_not_a_crawl_database_is_not_kept() {
        let d = temp("reject");
        let junk = b"not a database at all".to_vec();
        let s = Snapshot {
            written: "1".into(),
            sha256: sha(&junk),
        };
        assert!(keep(&d, &junk, &s).is_err());
        let empty = crawl_db(&d.join("empty.sqlite"), &[]);
        assert!(
            keep(
                &d,
                &empty,
                &Snapshot {
                    written: "1".into(),
                    sha256: sha(&empty)
                }
            )
            .is_err()
        );
        assert_eq!(pending(&d), None);
        assert!(!d.join("gts.sqlite.new").exists() && !d.join("gts.sqlite.download").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_user_agent_names_the_program_and_its_version() {
        assert_eq!(
            user_agent(),
            format!("Sozluk/{}", env!("CARGO_PKG_VERSION"))
        );
    }
}
