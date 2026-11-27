//! What the app keeps between runs, in `%LOCALAPPDATA%\sozluk\settings.ini`.

use std::path::PathBuf;

use mdshared_app::settings::{self, Writer, escape, unescape};

use mdshared_app::backdrop::Backdrop;

use crate::theme::Mode;

pub const APP: &str = "sozluk";

/// `--demo`: settings are neither read nor written (for screenshots and trials).
pub static DEMO: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Clone, Debug)]
pub struct Prefs {
    pub mode: Mode,
    /// Mica or Acrylic behind the chrome (Windows 11).
    pub backdrop: Backdrop,
    pub full: bool,
    pub on_top: bool,
    /// Ask sozluk.gov.tr for an entry that is not downloaded yet.
    pub live: bool,
    /// Check what is copied to the clipboard.
    pub watch_clipboard: bool,
    pub pinned: Vec<String>,
    pub history: Vec<String>,
    /// Words the line check takes as names.
    pub ignore: Vec<String>,
    pub last: Option<String>,
}

impl Default for Prefs {
    fn default() -> Prefs {
        Prefs {
            mode: Mode::System,
            backdrop: Backdrop::Mica,
            full: false,
            on_top: false,
            live: true,
            watch_clipboard: false,
            pinned: Vec::new(),
            history: Vec::new(),
            ignore: Vec::new(),
            last: None,
        }
    }
}

const SEP: char = '\u{1f}';

fn list(v: &str) -> Vec<String> {
    unescape(v).split(SEP).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

fn join(v: &[String]) -> String {
    escape(&v.join(&SEP.to_string()))
}

impl Prefs {
    pub fn path() -> Option<PathBuf> {
        if DEMO.load(std::sync::atomic::Ordering::Relaxed) {
            return None;
        }
        settings::default_settings_path(APP)
    }

    pub fn load() -> Prefs {
        let mut p = Prefs::default();
        let Some(text) = Prefs::path().and_then(|path| settings::read(&path).ok().flatten()) else {
            return p;
        };
        for (k, v) in settings::pairs(&text) {
            match k {
                "theme" => p.mode = Mode::from_key(v),
                "backdrop" => p.backdrop = Backdrop::parse(v).unwrap_or(Backdrop::Mica),
                "full" => p.full = v == "1",
                "on_top" => p.on_top = v == "1",
                "live" => p.live = v != "0",
                "watch_clipboard" => p.watch_clipboard = v == "1",
                "pinned" => p.pinned = list(v),
                "history" => p.history = list(v),
                "ignore" => p.ignore = list(v),
                "last" => p.last = Some(unescape(v)).filter(|s| !s.is_empty()),
                _ => {}
            }
        }
        p
    }

    pub fn save(&self) {
        let Some(path) = Prefs::path() else { return };
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let b = |x: bool| if x { "1" } else { "0" };
        let mut w = Writer::new("sozluk");
        w.line("theme", self.mode.key());
        w.line("backdrop", self.prefs_backdrop_key());
        w.line("full", b(self.full));
        w.line("on_top", b(self.on_top));
        w.line("live", b(self.live));
        w.line("watch_clipboard", b(self.watch_clipboard));
        w.line("pinned", &join(&self.pinned));
        w.line("history", &join(&self.history));
        w.line("ignore", &join(&self.ignore));
        w.line("last", &escape(self.last.as_deref().unwrap_or("")));
        let _ = settings::write_atomic(&path, &w.finish());
    }

    fn prefs_backdrop_key(&self) -> &'static str {
        self.backdrop.key()
    }

    /// Puts `w` at the front of the history (kept to 50).
    pub fn visited(&mut self, w: &str) {
        self.history.retain(|h| h != w);
        self.history.insert(0, w.to_string());
        self.history.truncate(50);
        self.last = Some(w.to_string());
    }

    pub fn is_pinned(&self, w: &str) -> bool {
        self.pinned.iter().any(|p| p == w)
    }

    pub fn toggle_pin(&mut self, w: &str) -> bool {
        if self.is_pinned(w) {
            self.pinned.retain(|p| p != w);
            false
        } else {
            self.pinned.insert(0, w.to_string());
            true
        }
    }
}
