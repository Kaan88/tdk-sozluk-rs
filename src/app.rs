//! The window: its state, what input does, and the live-lookup worker.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{Sender, channel};
use std::time::{Duration, Instant};

use mdshared_ui::host::{App, Button, Cx, Event, KeyPress};
use mdshared_ui::keys::KeyName;
use mdshared_ui::platform;
use mdshared_ui::textfield::TextField;
use winit::dpi::LogicalSize;
use winit::window::WindowLevel;

use crate::check::{self, Kind, Token};
use crate::dict::{Index, Lookup, Store, fetch_live};
use crate::prefs::Prefs;
use crate::theme::{Colors, Mode};
use mdshared_app::backdrop::Backdrop;

pub const COMPACT: (f32, f32) = (420.0, 660.0);
pub const FULL: (f32, f32) = (1120.0, 740.0);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tab {
    Lookup,
    Check,
    Pinned,
    Database,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Lookup, Tab::Check, Tab::Pinned, Tab::Database];
    pub fn label(self) -> &'static str {
        match self {
            Tab::Lookup => "Look up",
            Tab::Check => "Check",
            Tab::Pinned => "Pinned",
            Tab::Database => "Settings",
        }
    }
    pub fn index(self) -> usize {
        Tab::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }
}

/// Everything that can be clicked, as the last frame drew it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Hit {
    Title,
    Min,
    Max,
    Close,
    Theme(usize),
    Backdrop(usize),
    Size(bool),
    OnTop,
    Back,
    Forward,
    Search,
    ClearSearch,
    Tab(Tab),
    Result(usize),
    /// A headword to open (a compound, a variant, a pinned word, a synonym).
    Open(String),
    CopyHead,
    PinHead,
    Unpin(String),
    CheckField,
    CheckPaste,
    CheckClear,
    CheckWatch,
    CheckFixAll,
    /// Apply one finding: the token index and the text to put in.
    Apply(usize, String),
    /// Keep a finding as it is.
    Keep(String),
    /// Take a word as a name from now on.
    Ignore(String),
    DbLive,
    DbFolder,
    DbRefresh,
    Retry,
    Content,
}

pub enum Msg {
    Live(String, Result<Option<String>, String>),
}

pub struct Toast {
    pub text: String,
    pub until: Instant,
}

pub struct Sozluk {
    pub ix: Index,
    pub store: Store,
    pub data_dir: Option<PathBuf>,
    pub prefs: Prefs,
    pub system_dark: bool,

    pub tab: Tab,
    /// Bumped on every change of page or entry: the content fades and slides in under a new
    /// name.
    pub page_epoch: u64,
    pub page_dir: f32,

    pub search: TextField,
    pub search_focus: bool,
    pub search_stops: Vec<f32>,
    pub results: Vec<String>,
    pub sel: usize,
    /// The query the results are for; the list shows while it differs from the open word.
    pub results_for: String,

    pub current: Option<String>,
    pub lookup: Lookup,
    pub back: Vec<String>,
    pub forward: Vec<String>,
    pub live_tx: Option<Sender<String>>,
    pub live_pending: HashSet<String>,
    pub live_error: Option<String>,
    pub live_blocked_until: Option<Instant>,

    pub check_field: TextField,
    pub check_focus: bool,
    pub check_stops: Vec<f32>,
    pub check_text: String,
    pub check_tokens: Vec<Token>,
    pub last_clip: Option<String>,

    pub hits: Vec<(Rect, Hit)>,
    pub hover: Option<Hit>,
    pub pressed: Option<Hit>,
    pub pointer: (f32, f32),

    pub scroll: [f32; 4],
    pub content_h: f32,
    pub view_h: f32,

    pub toast: Option<Toast>,
    /// Where the fields' text starts, from the last frame.
    pub search_x: f32,
    pub check_x: f32,
    /// Findings the user chose to keep as they are, this session.
    pub kept: HashSet<String>,
    /// `--demo-switch`: flip the theme once, 1.5 s after start (to watch the transition).
    pub demo_switch: bool,
    pub blink: Option<Duration>,
    pub counts: Option<(u64, u64, u64)>,
    pub cache_n: u64,
}

pub use mdshared_ui::gfx::Rect;

impl Sozluk {
    pub fn new() -> Sozluk {
        let data_dir = mdshared_app::settings::data_dir(crate::prefs::APP);
        let prefs = Prefs::load();
        let ix = Index::load();
        let store = Store::open(data_dir.as_deref());
        let mut s = Sozluk {
            ix,
            store,
            data_dir,
            prefs,
            system_dark: platform::system_is_dark(),
            tab: Tab::Lookup,
            page_epoch: 0,
            page_dir: 0.0,
            search: TextField::new(),
            search_focus: true,
            search_stops: Vec::new(),
            results: Vec::new(),
            sel: 0,
            results_for: String::new(),
            current: None,
            lookup: Lookup::Missing,
            back: Vec::new(),
            forward: Vec::new(),
            live_tx: None,
            live_pending: HashSet::new(),
            live_error: None,
            live_blocked_until: None,
            check_field: TextField::new(),
            check_focus: false,
            check_stops: Vec::new(),
            check_text: String::new(),
            check_tokens: Vec::new(),
            last_clip: None,
            hits: Vec::new(),
            hover: None,
            pressed: None,
            pointer: (0.0, 0.0),
            scroll: [0.0; 4],
            content_h: 0.0,
            view_h: 0.0,
            toast: None,
            search_x: 0.0,
            check_x: 0.0,
            kept: HashSet::new(),
            demo_switch: false,
            blink: None,
            counts: None,
            cache_n: 0,
        };
        if let Some(w) = s.prefs.last.clone() {
            s.open_word(&w, false);
        }
        s
    }

    pub fn dark(&self) -> bool {
        self.prefs.mode.dark(self.system_dark)
    }

    pub fn full(&self) -> bool {
        self.prefs.full
    }

    // ---- navigation ---------------------------------------------------------------------------

    pub fn set_tab(&mut self, t: Tab) {
        if t != self.tab {
            self.page_dir = if t.index() > self.tab.index() { 1.0 } else { -1.0 };
            self.tab = t;
            self.page_epoch += 1;
            if t == Tab::Database {
                self.refresh_counts();
            }
        }
    }

    pub fn refresh_counts(&mut self) {
        self.store = Store::open(self.data_dir.as_deref());
        self.counts = self.store.crawl_counts();
        self.cache_n = self.store.cache_count();
    }

    /// Opens a headword. `push` keeps the one open now for Back.
    pub fn open_word(&mut self, w: &str, push: bool) {
        if self.current.as_deref() == Some(w) {
            self.results_for = self.search.text().to_string();
            return;
        }
        if push && let Some(c) = self.current.take() {
            self.back.push(c);
            self.forward.clear();
        }
        self.current = Some(w.to_string());
        self.lookup = self.store.lookup(w);
        self.live_error = None;
        self.prefs.visited(w);
        self.search.set_text(w);
        self.results_for = w.to_string();
        self.scroll[0] = 0.0;
        self.page_dir = 0.0;
        self.page_epoch += 1;
        self.ask_live_if_missing();
    }

    pub fn ask_live_if_missing(&mut self) {
        let Some(w) = self.current.clone() else { return };
        if !matches!(self.lookup, Lookup::Missing) || !self.prefs.live {
            return;
        }
        if self.live_blocked_until.is_some_and(|t| t > Instant::now()) {
            self.live_error = Some("sozluk.gov.tr asked us to wait (rate limit)".into());
            return;
        }
        if let Some(tx) = &self.live_tx
            && self.live_pending.insert(w.clone())
        {
            let _ = tx.send(w);
        }
    }

    pub fn go_back(&mut self) {
        if let Some(w) = self.back.pop() {
            if let Some(c) = self.current.take() {
                self.forward.push(c);
            }
            self.open_word(&w, false);
        }
    }

    pub fn go_forward(&mut self) {
        if let Some(w) = self.forward.pop() {
            if let Some(c) = self.current.take() {
                self.back.push(c);
            }
            self.open_word(&w, false);
        }
    }

    pub fn update_results(&mut self) {
        let q = self.search.text().to_string();
        self.results = self.ix.search(&q, 60).into_iter().map(str::to_string).collect();
        self.sel = 0;
        self.results_for = String::new();
        if self.tab != Tab::Lookup {
            self.set_tab(Tab::Lookup);
        }
    }

    /// The results list is up while the query is not the open word.
    pub fn showing_results(&self) -> bool {
        !self.search.text().trim().is_empty() && self.results_for != self.search.text()
    }

    pub fn toast(&mut self, cx: &mut Cx<'_, Msg>, text: &str) {
        let until = cx.now() + Duration::from_millis(1600);
        self.toast = Some(Toast { text: text.to_string(), until });
        cx.wake_at(until);
    }

    pub fn copy(&mut self, cx: &mut Cx<'_, Msg>, text: &str) {
        if platform::set_clipboard_text(text) {
            let t = format!("Copied \u{201c}{text}\u{201d}");
            self.toast(cx, &t);
        }
    }

    // ---- check --------------------------------------------------------------------------------

    pub fn set_check_text(&mut self, text: &str) {
        self.check_text = text.replace("\r\n", "\n");
        self.check_tokens = check::check(&self.ix, &self.check_text, &self.prefs.ignore);
    }

    pub fn findings(&self) -> Vec<usize> {
        self.check_tokens
            .iter()
            .enumerate()
            .filter(|(_, t)| matches!(t.kind, Kind::Spelling { .. } | Kind::Pair { .. }))
            .filter(|(_, t)| !self.kept.contains(&crate::turkish::lower(&t.word)))
            .map(|(i, _)| i)
            .collect()
    }

    fn apply_fix(&mut self, i: usize, with: &str) {
        if let Some(t) = self.check_tokens.get(i).cloned() {
            let text = check::replace(&self.check_text, &t, with);
            self.set_check_text(&text);
            self.check_field.set_text(text.lines().next().unwrap_or(""));
        }
    }

    fn poll_clipboard(&mut self, cx: &mut Cx<'_, Msg>) {
        if !self.prefs.watch_clipboard {
            return;
        }
        if let Some(t) = platform::clipboard_text()
            && self.last_clip.as_deref() != Some(t.as_str())
            && t.chars().count() <= 4000
        {
            self.last_clip = Some(t.clone());
            if !t.trim().is_empty() {
                self.set_check_text(&t);
                cx.request_redraw_all();
            }
        }
        cx.timer_at("clipboard", cx.now() + Duration::from_millis(700));
    }

    // ---- window -------------------------------------------------------------------------------

    pub fn apply_size(&self, cx: &mut Cx<'_, Msg>) {
        let (w, h) = if self.prefs.full { FULL } else { COMPACT };
        let _ = cx.window().request_inner_size(LogicalSize::new(w as f64, h as f64));
    }

    pub fn apply_on_top(&self, cx: &mut Cx<'_, Msg>) {
        cx.window().set_window_level(if self.prefs.on_top { WindowLevel::AlwaysOnTop } else { WindowLevel::Normal });
    }

    fn set_mode(&mut self, cx: &mut Cx<'_, Msg>, m: Mode) {
        self.prefs.mode = m;
        cx.set_dark(self.dark());
        self.prefs.save();
        self.toast(cx, m.label());
    }

    fn set_backdrop(&mut self, cx: &mut Cx<'_, Msg>, b: Backdrop) {
        self.prefs.backdrop = b;
        self.prefs.save();
        let label = match b {
            Backdrop::Off => "Backdrop off",
            Backdrop::Mica => "Mica",
            Backdrop::Acrylic => "Acrylic",
        };
        self.toast(cx, label);
        if b != Backdrop::Off && !platform::backdrop_available() {
            self.toast(cx, "This Windows has no Mica or Acrylic");
        }
    }

    fn hit_at(&self, x: f32, y: f32) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(x, y)).map(|(_, h)| h.clone())
    }

    fn focus_search(&mut self) {
        self.search_focus = true;
        self.check_focus = false;
        self.search.select_all();
    }

    fn activate(&mut self, cx: &mut Cx<'_, Msg>, h: Hit) {
        match h {
            Hit::Close => {
                self.prefs.save();
                cx.exit();
            }
            Hit::Min => cx.minimize(),
            Hit::Max => cx.toggle_maximize(),
            Hit::Theme(i) => self.set_mode(cx, Mode::ALL[i]),
            Hit::Backdrop(i) => self.set_backdrop(cx, Backdrop::from_index(i)),
            Hit::Size(full) => {
                if self.prefs.full != full {
                    self.prefs.full = full;
                    self.apply_size(cx);
                    self.page_epoch += 1;
                    self.prefs.save();
                }
            }
            Hit::OnTop => {
                self.prefs.on_top = !self.prefs.on_top;
                self.apply_on_top(cx);
                self.prefs.save();
                let t = if self.prefs.on_top { "Stays on top" } else { "No longer on top" };
                self.toast(cx, t);
            }
            Hit::Back => self.go_back(),
            Hit::Forward => self.go_forward(),
            Hit::ClearSearch => {
                self.search.clear();
                self.results.clear();
                self.search_focus = true;
            }
            Hit::Tab(t) => self.set_tab(t),
            Hit::Result(i) => {
                if let Some(w) = self.results.get(i).cloned() {
                    self.open_word(&w, true);
                }
            }
            Hit::Open(w) => {
                self.set_tab(Tab::Lookup);
                self.open_word(&w, true);
            }
            Hit::CopyHead => {
                if let Some(w) = self.current.clone() {
                    self.copy(cx, &w);
                }
            }
            Hit::PinHead => {
                if let Some(w) = self.current.clone() {
                    let on = self.prefs.toggle_pin(&w);
                    self.prefs.save();
                    self.toast(cx, if on { "Pinned" } else { "Unpinned" });
                }
            }
            Hit::Unpin(w) => {
                self.prefs.pinned.retain(|p| *p != w);
                self.prefs.save();
            }
            Hit::CheckPaste => {
                if let Some(t) = platform::clipboard_text() {
                    self.check_field.set_text(t.lines().next().unwrap_or(""));
                    self.set_check_text(&t);
                }
            }
            Hit::CheckClear => {
                self.check_field.clear();
                self.set_check_text("");
                self.check_focus = true;
                self.search_focus = false;
            }
            Hit::CheckWatch => {
                self.prefs.watch_clipboard = !self.prefs.watch_clipboard;
                self.prefs.save();
                if self.prefs.watch_clipboard {
                    self.last_clip = platform::clipboard_text();
                    cx.timer_at("clipboard", cx.now() + Duration::from_millis(700));
                }
            }
            Hit::CheckFixAll => {
                let fixed = check::fixed(&self.check_text, &self.check_tokens);
                self.set_check_text(&fixed);
                self.check_field.set_text(fixed.lines().next().unwrap_or(""));
                if platform::set_clipboard_text(&fixed) {
                    self.last_clip = Some(fixed);
                    self.toast(cx, "Fixed text copied");
                }
            }
            Hit::Apply(i, with) => self.apply_fix(i, &with),
            Hit::Keep(w) => {
                self.kept.insert(crate::turkish::lower(&w));
            }
            Hit::Ignore(w) => {
                if !self.prefs.ignore.contains(&w) {
                    self.prefs.ignore.push(w);
                    self.prefs.save();
                }
                let t = self.check_text.clone();
                self.set_check_text(&t);
            }
            Hit::DbLive => {
                self.prefs.live = !self.prefs.live;
                self.prefs.save();
            }
            Hit::DbFolder => {
                let target = self.store.crawl_path.clone().or_else(|| self.store.cache_path.clone());
                if let Some(p) = target {
                    platform::reveal_in_file_manager(&p);
                }
            }
            Hit::DbRefresh => {
                self.refresh_counts();
                if let Some(w) = self.current.clone() {
                    self.lookup = self.store.lookup(&w);
                }
                self.toast(cx, "Database read again");
            }
            Hit::Retry => {
                self.live_blocked_until = None;
                self.live_error = None;
                if let Some(w) = &self.current {
                    self.live_pending.remove(w);
                }
                self.ask_live_if_missing();
            }
            Hit::Search => self.focus_search(),
            Hit::CheckField => {
                self.check_focus = true;
                self.search_focus = false;
            }
            Hit::Title | Hit::Content => {}
        }
        cx.request_redraw_all();
    }

    fn key(&mut self, cx: &mut Cx<'_, Msg>, k: KeyPress) {
        let Some(name) = k.name.clone() else { return };
        let m = k.mods;
        // Shortcuts first.
        match (&name, m.ctrl, m.shift, m.alt) {
            (KeyName::Char('k' | 'l'), true, false, false) | (KeyName::F(3), false, false, false) => {
                self.set_tab(Tab::Lookup);
                self.focus_search();
                return;
            }
            (KeyName::Char(c @ '1'..='4'), true, false, false) => {
                let i = (*c as usize) - ('1' as usize);
                self.set_tab(Tab::ALL[i]);
                if self.tab == Tab::Check {
                    self.check_focus = true;
                    self.search_focus = false;
                }
                return;
            }
            (KeyName::Char('m'), true, false, false) => {
                let f = !self.prefs.full;
                self.activate(cx, Hit::Size(f));
                return;
            }
            (KeyName::Char('t'), true, false, false) => {
                let next = self.prefs.mode.next();
                self.set_mode(cx, next);
                return;
            }
            (KeyName::Char('b'), true, false, false) => {
                let next = Backdrop::from_index((self.prefs.backdrop.index() + 1) % 3);
                self.set_backdrop(cx, next);
                return;
            }
            (KeyName::Char('d'), true, false, false) => {
                self.activate(cx, Hit::PinHead);
                return;
            }
            (KeyName::Char('c'), true, true, false) => {
                self.activate(cx, Hit::CopyHead);
                return;
            }
            (KeyName::Char('v'), true, true, false) => {
                self.set_tab(Tab::Check);
                self.activate(cx, Hit::CheckPaste);
                return;
            }
            (KeyName::Left, false, false, true) => {
                self.go_back();
                return;
            }
            (KeyName::Right, false, false, true) => {
                self.go_forward();
                return;
            }
            (KeyName::Enter, true, false, false) if self.tab == Tab::Check => {
                self.activate(cx, Hit::CheckFixAll);
                return;
            }
            _ => {}
        }
        if self.search_focus {
            if let Some(changed) = self.search.handle_key(&name, m, k.text.as_deref(), true) {
                if changed {
                    self.update_results();
                }
                return;
            }
            match name {
                KeyName::Down if self.showing_results() => {
                    self.sel = (self.sel + 1).min(self.results.len().saturating_sub(1));
                }
                KeyName::Up if self.showing_results() => self.sel = self.sel.saturating_sub(1),
                KeyName::Enter => {
                    if self.showing_results() {
                        if let Some(w) = self.results.get(self.sel).cloned() {
                            self.open_word(&w, true);
                        }
                    } else if let Some(w) = self.ix.search(self.search.text(), 1).first() {
                        let w = w.to_string();
                        self.open_word(&w, true);
                    }
                }
                KeyName::Escape => {
                    if self.showing_results() {
                        let w = self.current.clone().unwrap_or_default();
                        self.search.set_text(&w);
                        self.results_for = w;
                    } else if !self.search.is_empty() {
                        self.search.clear();
                        self.results.clear();
                    } else {
                        self.search_focus = false;
                    }
                }
                KeyName::PageDown => self.scroll_by(self.view_h * 0.85),
                KeyName::PageUp => self.scroll_by(-self.view_h * 0.85),
                _ => {}
            }
            return;
        }
        if self.check_focus {
            if let Some(changed) = self.check_field.handle_key(&name, m, k.text.as_deref(), true) {
                if changed {
                    let t = self.check_field.text().to_string();
                    self.set_check_text(&t);
                }
                return;
            }
            if name == KeyName::Escape {
                self.check_focus = false;
            }
            return;
        }
        match name {
            KeyName::PageDown | KeyName::Down => self.scroll_by(if name == KeyName::Down { 60.0 } else { self.view_h * 0.85 }),
            KeyName::PageUp | KeyName::Up => self.scroll_by(if name == KeyName::Up { -60.0 } else { -self.view_h * 0.85 }),
            KeyName::Home => self.scroll[self.tab.index()] = 0.0,
            KeyName::Backspace if self.tab == Tab::Lookup => self.go_back(),
            _ => {
                // Type to search: a letter typed with nothing focused starts a search.
                if let Some(t) = k.text.as_deref().filter(|t| t.chars().all(char::is_alphabetic) && !t.is_empty())
                    && !m.ctrl
                    && !m.alt
                {
                    self.set_tab(Tab::Lookup);
                    self.search_focus = true;
                    self.search.clear();
                    self.search.insert(t);
                    self.update_results();
                }
            }
        }
    }

    pub fn scroll_by(&mut self, dy: f32) {
        let i = self.tab.index();
        let max = (self.content_h - self.view_h).max(0.0);
        self.scroll[i] = (self.scroll[i] + dy).clamp(0.0, max);
    }
}

/// The window's and the taskbar's icon, from the pixels `tools/make_icon.py` writes.
fn set_icons(cx: &mut Cx<'_, Msg>) {
    use winit::window::Icon;
    const SMALL: &[u8] = include_bytes!("../assets/icon-32.rgba");
    const BIG: &[u8] = include_bytes!("../assets/icon-256.rgba");
    if let Ok(i) = Icon::from_rgba(SMALL.to_vec(), 32, 32) {
        cx.window().set_window_icon(Some(i));
    }
    #[cfg(windows)]
    {
        use winit::platform::windows::WindowExtWindows;
        if let Ok(i) = Icon::from_rgba(BIG.to_vec(), 256, 256) {
            cx.window().set_taskbar_icon(Some(i));
        }
    }
    #[cfg(not(windows))]
    let _ = BIG;
}

fn spawn_live(proxy: mdshared_ui::host::Proxy<Msg>) -> Sender<String> {
    let (tx, rx) = channel::<String>();
    std::thread::spawn(move || {
        while let Ok(w) = rx.recv() {
            let r = fetch_live(&w);
            if !proxy.send(Msg::Live(w, r)) {
                break;
            }
            // One request at a time, with a pause: never more than about two a second.
            std::thread::sleep(Duration::from_millis(450));
        }
    });
    tx
}

impl App for Sozluk {
    type Message = Msg;

    fn start(&mut self, cx: &mut Cx<'_, Msg>) {
        self.live_tx = Some(spawn_live(cx.proxy()));
        self.blink = platform::caret_blink_time();
        cx.set_dark(self.dark());
        cx.set_backdrop(self.prefs.backdrop, &Colors::of(self.dark()).pal);
        set_icons(cx);
        self.apply_on_top(cx);
        self.ask_live_if_missing();
        if self.prefs.watch_clipboard {
            self.last_clip = platform::clipboard_text();
            cx.timer_at("clipboard", cx.now() + Duration::from_millis(700));
        }
        self.refresh_counts();
        if self.demo_switch {
            cx.timer_at("demo-switch", cx.now() + Duration::from_millis(1500));
        }
    }

    fn timer_fired(&mut self, cx: &mut Cx<'_, Msg>, name: &str) {
        if name == "clipboard" {
            self.poll_clipboard(cx);
        }
        if name == "demo-switch" {
            let next = if self.dark() { Mode::Light } else { Mode::Dark };
            self.set_mode(cx, next);
            cx.request_redraw_all();
        }
    }

    fn message(&mut self, cx: &mut Cx<'_, Msg>, m: Msg) {
        let Msg::Live(w, r) = m;
        self.live_pending.remove(&w);
        match r {
            Ok(json) => {
                self.store.remember(&w, json.as_deref());
                if self.current.as_deref() == Some(w.as_str()) {
                    self.lookup = self.store.lookup(&w);
                    self.page_epoch += 1;
                }
            }
            Err(e) => {
                if e.contains("429") {
                    self.live_blocked_until = Some(Instant::now() + Duration::from_secs(90));
                }
                if self.current.as_deref() == Some(w.as_str()) {
                    self.live_error = Some(e);
                }
            }
        }
        cx.request_redraw_all();
    }

    fn event(&mut self, cx: &mut Cx<'_, Msg>, event: Event) {
        match event {
            Event::PointerMove { x, y } => {
                self.pointer = (x, y);
                let h = self.hit_at(x, y);
                if h != self.hover {
                    self.hover = h;
                    cx.request_redraw_all();
                }
                let text = matches!(self.hover, Some(Hit::Search | Hit::CheckField));
                cx.set_cursor(if text {
                    mdshared_ui::cursor::Cursor::Text
                } else if matches!(self.hover, None | Some(Hit::Title | Hit::Content)) {
                    mdshared_ui::cursor::Cursor::Default
                } else {
                    mdshared_ui::cursor::Cursor::Hand
                });
            }
            Event::PointerLeave => {
                self.hover = None;
                cx.request_redraw_all();
            }
            Event::Press { x, y, button: Button::Left, clicks } => {
                let h = self.hit_at(x, y);
                match h {
                    Some(Hit::Title) => {
                        if clicks == 2 && self.prefs.full {
                            cx.title_double_click();
                        } else {
                            cx.drag_window();
                        }
                    }
                    Some(Hit::Search) => {
                        self.search_focus = true;
                        self.check_focus = false;
                        let fx = self.search_x;
                        if clicks >= 2 {
                            self.search.select_all();
                        } else {
                            self.search.press(x - fx, &self.search_stops, cx.mods().shift);
                        }
                    }
                    Some(Hit::CheckField) => {
                        self.check_focus = true;
                        self.search_focus = false;
                        let fx = self.check_x;
                        self.check_field.press(x - fx, &self.check_stops, cx.mods().shift);
                    }
                    Some(h) => {
                        if !matches!(h, Hit::Content) {
                            self.pressed = Some(h);
                        } else {
                            self.search_focus = false;
                            self.check_focus = false;
                        }
                    }
                    None => {}
                }
                cx.request_redraw_all();
            }
            Event::Press { button: Button::Back, .. } => {
                self.go_back();
                cx.request_redraw_all();
            }
            Event::Press { button: Button::Forward, .. } => {
                self.go_forward();
                cx.request_redraw_all();
            }
            Event::Release { x, y, button: Button::Left } => {
                if let Some(p) = self.pressed.take() {
                    if self.hit_at(x, y).as_ref() == Some(&p) {
                        self.activate(cx, p);
                    }
                    cx.request_redraw_all();
                }
            }
            Event::Wheel { dy, precise, .. } => {
                let step = if precise { dy } else { dy * 1.0 };
                self.scroll_by(-step);
                cx.request_redraw_all();
            }
            Event::Key(k) => {
                self.key(cx, k);
                cx.request_redraw_all();
            }
            Event::Text(t) => {
                if self.search_focus && self.search.insert(&t) {
                    self.update_results();
                } else if self.check_focus && self.check_field.insert(&t) {
                    let tx = self.check_field.text().to_string();
                    self.set_check_text(&tx);
                }
                cx.request_redraw_all();
            }
            Event::Wake(now) => {
                let _ = now;
                cx.request_redraw_all();
            }
            Event::Theme { dark } => {
                self.system_dark = dark;
                cx.set_dark(self.dark());
                cx.request_redraw_all();
            }
            Event::Focus(_) | Event::Resize { .. } | Event::Scale(_) => cx.request_redraw_all(),
            Event::CloseRequested => {
                self.prefs.save();
                cx.exit();
            }
            _ => {}
        }
    }

    fn raw(&mut self, cx: &mut Cx<'_, Msg>, event: &winit::event::WindowEvent) {
        // A subtitle or text file dropped on the window is checked.
        if let winit::event::WindowEvent::DroppedFile(path) = event
            && let Ok(bytes) = std::fs::read(path)
        {
            let text = String::from_utf8_lossy(&bytes);
            let text = crate::draw::subtitle_text(&text);
            self.set_tab(Tab::Check);
            self.check_field.set_text(text.lines().next().unwrap_or(""));
            self.set_check_text(&text);
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            self.toast(cx, &format!("Checking {name}"));
            cx.request_redraw_all();
        }
    }

    fn draw(&mut self, cx: &mut Cx<'_, Msg>, p: &mut mdshared_ui::gfx::Painter, _damage: Rect) {
        crate::draw::frame(self, cx, p);
    }
}
