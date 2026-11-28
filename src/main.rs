//! Sözlük: the TDK Güncel Türkçe Sözlük on the desktop, offline.
#![cfg_attr(not(test), windows_subsystem = "windows")]
// Drawing calls take a rectangle plus its style; grouping them would only hide that.
#![allow(clippy::too_many_arguments)]

mod app;
mod check;
mod dict;
mod download;
mod draw;
mod history;
mod lang;
mod prefs;
mod theme;
mod turkish;
mod widgets;

use mdshared_ui::host::{self, Config};

/// Launch options: `--demo` (settings untouched), `--word W`, `--tab lookup|check|pinned|database`,
/// `--check TEXT`, `--theme system|light|dark`, `--full`, `--compact`.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |k: &str| {
        args.iter()
            .position(|a| a == k)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    if args.iter().any(|a| a == "--demo") {
        prefs::DEMO.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    let mut app = app::Sozluk::new();
    if let Some(l) = arg("--lang") {
        app.prefs.lang = lang::Lang::from_key(&l);
        lang::set(app.prefs.lang);
    }
    if let Some(t) = arg("--theme") {
        app.prefs.mode = theme::Mode::from_key(&t);
    }
    if args.iter().any(|a| a == "--full") {
        app.prefs.full = true;
    }
    if args.iter().any(|a| a == "--compact") {
        app.prefs.full = false;
    }
    if let Some(w) = arg("--word") {
        app.open_word(&w, true);
        app.search_focus = false;
    }
    if let Some(t) = arg("--check") {
        // A literal `\n` on the command line is a line break.
        let t = t.replace("\\n", "\n");
        app.check_field.set_text(t.lines().next().unwrap_or(""));
        app.set_check_text(&t);
    }
    if let Some(t) = arg("--tab") {
        let tab = match t.as_str() {
            "check" => app::Tab::Check,
            "pinned" => app::Tab::Pinned,
            "database" | "settings" => app::Tab::Database,
            _ => app::Tab::Lookup,
        };
        app.set_tab(tab);
        app.search_focus = false;
    }
    app.demo_switch = args.iter().any(|a| a == "--demo-switch");
    if let Some(q) = arg("--search") {
        app.search.set_text(&q);
        app.update_results();
    }
    let (w, h) = app.prefs.window_size();
    let config = Config::new("Sözlük")
        .size(w, h)
        .min_size(380.0, 520.0)
        .app_id("sozluk")
        .dark(app.dark());
    if let Err(e) = host::run(config, app) {
        eprintln!("sozluk: {e}");
        std::process::exit(1);
    }
}
