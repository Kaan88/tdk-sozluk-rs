//! Every frame, drawn whole: the title row with the theme and size switches, the search bar,
//! the tabs with their sliding underline, the page (fading and sliding in when it changes), the
//! status strip and the toast.

use crate::lang::{en, tl};
use crate::tf;
use std::time::Instant;

use mdshared_ui::anim::{Animator, fluent};
use mdshared_ui::chrome::{self, WinButton};
use mdshared_ui::gfx::{Face, Painter, Rect, mix};
use mdshared_ui::glyph;
use mdshared_ui::host::Cx;
use mdshared_ui::overlay::Bubble;
use mdshared_ui::textfield::FieldColors;

use crate::app::{Hit, Msg, Sozluk, Tab};
use crate::check::Kind;
use crate::dict::{Entry, Lookup};
use crate::download::Event;
use crate::theme::{self, Colors, Mode};
use crate::turkish::{letters, lower};
use crate::widgets::{self as w, RADIUS};

const TITLE_H: f32 = chrome::TITLE_H;
const BAR_H: f32 = 50.0;
const TABS_H: f32 = 36.0;
const STATUS_H: f32 = 28.0;
const BOOK: &str = "M2.5 3h4a1.5 1.5 0 0 1 1.5 1.5v9a1.5 1.5 0 0 0-1.5-1.5h-4zM13.5 3h-4A1.5 1.5 0 0 0 8 4.5v9a1.5 1.5 0 0 1 1.5-1.5h4z";
const LEFT: &str = "M10.2 3.5L5.7 8l4.5 4.5";
const STAR: &str = "M8 1.8l1.9 3.9 4.3.6-3.1 3 .7 4.3L8 11.6l-3.8 2 .7-4.3-3.1-3 4.3-.6z";

/// Intersection of two rectangles (empty when apart).
fn meet(a: Rect, b: Rect) -> Rect {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = a.right().min(b.right());
    let y1 = a.bottom().min(b.bottom());
    Rect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
}

/// What a page needs while it draws: the colours, the animator, and where its hits go (moved by
/// the scroll, cut to the page).
struct Pg<'a> {
    c: Colors,
    anim: &'a mut Animator,
    hits: Vec<(Rect, Hit)>,
    dy: f32,
    clip: Rect,
    hover: Option<Hit>,
    pressed: Option<Hit>,
}

impl Pg<'_> {
    fn hit(&mut self, r: Rect, h: Hit) {
        let r = meet(Rect::new(r.x, r.y + self.dy, r.w, r.h), self.clip);
        if r.w > 0.0 && r.h > 0.0 {
            self.hits.push((r, h));
        }
    }
    fn hovered(&self, h: &Hit) -> bool {
        self.hover.as_ref() == Some(h)
    }
    fn pressed(&self, h: &Hit) -> bool {
        self.pressed.as_ref() == Some(h)
    }
    fn icon_btn(&mut self, p: &mut Painter, r: Rect, g: &str, h: Hit, on: bool) {
        let (hv, pr) = (self.hovered(&h), self.pressed(&h));
        w::icon_button(p, self.anim, &self.c, r, g, h.clone(), hv, pr, on);
        self.hit(r, h);
    }
    fn btn(&mut self, p: &mut Painter, r: Rect, label: &str, h: Hit, primary: bool) {
        let (hv, pr) = (self.hovered(&h), self.pressed(&h));
        w::text_button(p, self.anim, &self.c, r, label, h.clone(), hv, pr, primary);
        self.hit(r, h);
    }
    /// A row that lights under the pointer.
    fn row(&mut self, p: &mut Painter, r: Rect, h: Hit) {
        let t = w::hover_t(self.anim, h.clone(), self.hovered(&h));
        if t > 0.0 {
            p.rbox_a(r, RADIUS, self.c.pal.hover, t);
        }
        self.hit(r, h);
    }
    fn chips(
        &mut self,
        p: &mut Painter,
        x: f32,
        y: f32,
        width: f32,
        labels: &[String],
        id: &str,
        open: bool,
    ) -> f32 {
        let hov = labels
            .iter()
            .position(|l| self.hovered(&Hit::Open(l.clone())));
        let (rects, h) = w::chip_rows(p, self.anim, &self.c, x, y, width, labels, hov, id);
        if open {
            for (r, l) in rects.into_iter().zip(labels) {
                self.hit(r, Hit::Open(l.clone()));
            }
        }
        h
    }
}

pub fn subtitle_text(text: &str) -> String {
    // An .srt: cue numbers and timecodes go, tags go; the lines stay.
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.contains("-->") || (!t.is_empty() && t.chars().all(|c| c.is_ascii_digit())) {
            continue;
        }
        let mut clean = String::new();
        let mut skip = 0u8;
        for ch in t.chars() {
            match ch {
                '<' | '{' => skip += 1,
                '>' | '}' if skip > 0 => skip -= 1,
                _ if skip == 0 => clean.push(ch),
                _ => {}
            }
        }
        out.push(clean);
    }
    let joined = out.join("\n");
    joined.trim().to_string()
}

/// The synonyms TDK lists in its meanings: the words after the last `;`, and `► x` redirects,
/// kept when they are headwords.
pub fn synonyms(s: &Sozluk, entries: &[Entry]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in entries {
        for m in &e.meanings {
            let t = m.text.trim();
            let tail = if let Some(r) = t.strip_prefix('►') {
                r.to_string()
            } else if let Some((_, r)) = t.rsplit_once(';') {
                r.to_string()
            } else {
                continue;
            };
            for part in tail.split(',') {
                let word = part
                    .trim()
                    .trim_end_matches(|c: char| {
                        c == ')'
                            || c.is_ascii_digit()
                            || c == '('
                            || c == 'I'
                            || c == 'V'
                            || c == ' '
                    })
                    .trim();
                let word = word.split(" (").next().unwrap_or(word).trim();
                if !word.is_empty()
                    && s.ix.contains(word)
                    && lower(word) != lower(&e.headword)
                    && !out.iter().any(|o| lower(o) == lower(word))
                {
                    out.push(
                        s.ix.exact(word)
                            .first()
                            .map(|x| x.to_string())
                            .unwrap_or(word.to_string()),
                    );
                }
            }
        }
    }
    out
}

pub fn frame(s: &mut Sozluk, cx: &mut Cx<'_, Msg>, p: &mut Painter) {
    let (ww, wh) = cx.size();
    let maximized = cx.maximized();
    let focused = cx.focused();
    let now = cx.now();
    let full = s.full();
    let target = Colors::of(s.dark());
    let frosted = cx.frosted();
    let anim = cx.anim();
    let c = theme::animated(anim, &target);
    let pal = c.pal;
    let win = Rect::new(0.0, 0.0, ww, wh);
    p.fill(win, pal.page);

    let mut g = Pg {
        c,
        anim,
        hits: Vec::new(),
        dy: 0.0,
        clip: win,
        hover: s.hover.clone(),
        pressed: s.pressed.clone(),
    };
    // Lets Mica or Acrylic through a chrome region once it is drawn; whatever is drawn over it
    // afterwards with an opaque fill (the search field, the toast) stays opaque.
    let frost = |p: &mut Painter, r: Rect| {
        if frosted {
            let d = p.dev_rect(r);
            mdshared_ui::frost::mark(p.bmp, d);
        }
    };

    // ---- title row ----
    let title = Rect::new(0.0, 0.0, ww, TITLE_H);
    p.fill(title, pal.title);
    g.hit(title, Hit::Title);
    // The logo in small: the accent tile and the white book.
    let tile = Rect::new(13.0, title.cy() - 9.0, 18.0, 18.0);
    p.rbox(tile, 4.5, pal.accent);
    p.icon(BOOK, 13.0, tile.cx(), tile.cy() + 0.5, pal.on_accent);
    let name_w = p.text(
        Face::Semibold,
        13.0,
        38.0,
        title.cy(),
        "Sözlük",
        pal.text,
        80.0,
    );
    if full && let Some(cur) = &s.current {
        p.text(
            Face::Ui,
            13.0,
            38.0 + name_w + 8.0,
            title.cy(),
            &format!("· {cur}"),
            pal.muted,
            260.0,
        );
    }
    let buttons: Vec<WinButton> = if full {
        vec![WinButton::Close, WinButton::Max, WinButton::Min]
    } else {
        vec![WinButton::Close, WinButton::Min]
    };
    let mut right = ww;
    for (n, b) in buttons.iter().enumerate() {
        let r = chrome::win_button_rect(ww, n);
        right = r.x;
        let h = match b {
            WinButton::Close => Hit::Close,
            WinButton::Max => Hit::Max,
            WinButton::Min => Hit::Min,
        };
        chrome::draw_win_button(p, &pal, *b, r, maximized, g.hovered(&h), g.pressed(&h));
        g.hit(r, h);
    }
    // Always on top.
    let pin = Rect::new(right - 38.0, 6.0, 30.0, 28.0);
    g.icon_btn(p, pin, glyph::PIN, Hit::OnTop, s.prefs.on_top);
    // Compact | Full.
    let size_r = Rect::new(pin.x - 66.0, 6.0, 60.0, 28.0);
    let hov = match &g.hover {
        Some(Hit::Size(f)) => Some(*f as usize),
        _ => None,
    };
    let segs = w::segmented_icons(
        p,
        g.anim,
        &g.c,
        size_r,
        &[glyph::PAGE_NARROW, glyph::PAGE_WIDE],
        full as usize,
        hov,
        "size",
    );
    g.hit(segs[0], Hit::Size(false));
    g.hit(segs[1], Hit::Size(true));
    // System | Light | Dark.
    let theme_r = Rect::new(size_r.x - 98.0, 6.0, 90.0, 28.0);
    let hov = match &g.hover {
        Some(Hit::Theme(i)) => Some(*i),
        _ => None,
    };
    let segs = w::segmented_icons(
        p,
        g.anim,
        &g.c,
        theme_r,
        &[glyph::SYSTEM, glyph::SUN, glyph::MOON],
        s.prefs.mode.index(),
        hov,
        "theme",
    );
    for (i, r) in segs.into_iter().enumerate() {
        g.hit(r, Hit::Theme(i));
    }
    let mut max = chrome::win_button_rect(ww, 1);
    if full && !maximized {
        max.y += mdshared_ui::window::EDGE;
        max.h -= mdshared_ui::window::EDGE;
    }

    frost(p, title);

    // ---- search bar ----
    let bar = Rect::new(0.0, TITLE_H, ww, BAR_H);
    p.fill(bar, pal.bar);
    let back = Rect::new(8.0, bar.y + 10.0, 30.0, 30.0);
    let fwd = Rect::new(40.0, bar.y + 10.0, 30.0, 30.0);
    let can_back = !s.back.is_empty();
    let can_fwd = !s.forward.is_empty();
    g.icon_btn(p, back, LEFT, Hit::Back, false);
    g.icon_btn(p, fwd, glyph::RIGHT, Hit::Forward, false);
    if !can_back {
        p.rbox_a(back, RADIUS, pal.bar, 0.6);
    }
    if !can_fwd {
        p.rbox_a(fwd, RADIUS, pal.bar, 0.6);
    }
    frost(p, bar);
    let field_w = if full {
        (ww - 90.0).min(560.0)
    } else {
        ww - 86.0
    };
    let field = Rect::new(76.0, bar.y + 8.0, field_w, 34.0);
    let ft = w::hover_t(g.anim, "search-focus", s.search_focus && focused);
    p.frame(field, RADIUS, pal.field, pal.line);
    let under = Rect::new(field.x + 1.0, field.bottom() - 2.0, field.w - 2.0, 2.0);
    // The accent line grows from the middle as the field takes the keyboard.
    let grow = under.w * ft;
    p.fill(
        Rect::new(under.cx() - grow / 2.0, under.y, grow, 2.0),
        pal.accent,
    );
    p.icon(glyph::SEARCH, 14.0, field.x + 16.0, field.cy(), pal.muted);
    let inner = Rect::new(
        field.x + 32.0,
        field.y + 4.0,
        field.w - 32.0 - 34.0,
        field.h - 8.0,
    );
    s.search_x = inner.x;
    s.search_area = Rect::new(inner.x, inner.y + g.dy, inner.w, inner.h);
    let colors = FieldColors {
        text: pal.text,
        placeholder: pal.muted,
        selection: pal.selection,
        caret: pal.text,
    };
    s.search.draw_blinking(
        p,
        g.anim,
        "search",
        s.blink,
        inner,
        Face::Ui,
        14.0,
        &colors,
        tl("TDK'de ara  (Ctrl+K)", "Search TDK  (Ctrl+K)"),
        s.search_focus && focused,
    );
    s.search_stops = p.caret_stops(Face::Ui, 14.0, &s.search.display());
    g.hit(
        Rect::new(field.x, field.y, field.w - 32.0, field.h),
        Hit::Search,
    );
    if !s.search.is_empty() {
        let x = Rect::new(field.right() - 32.0, field.y + 3.0, 28.0, 28.0);
        g.icon_btn(p, x, glyph::CLOSE, Hit::ClearSearch, false);
    }

    // ---- tabs ----
    let tabs = Rect::new(0.0, bar.bottom(), ww, TABS_H);
    p.fill(tabs, pal.bar);
    p.fill(Rect::new(0.0, tabs.bottom() - 1.0, ww, 1.0), pal.line);
    let tab_w = if full { 120.0 } else { (ww - 16.0) / 4.0 };
    let mut ul = Rect::new(0.0, 0.0, 0.0, 0.0);
    for (i, t) in Tab::ALL.iter().enumerate() {
        let r = Rect::new(8.0 + i as f32 * tab_w, tabs.y, tab_w, TABS_H - 1.0);
        let h = Hit::Tab(*t);
        let hv = w::hover_t(g.anim, h.clone(), g.hovered(&h));
        let on = *t == s.tab;
        let label = match t {
            Tab::Pinned if !s.prefs.pinned.is_empty() => {
                tf!("Sabitlenen {}", "Pinned {}", s.prefs.pinned.len())
            }
            Tab::Check if !s.findings().is_empty() => {
                tf!("Denetle {}", "Check {}", s.findings().len())
            }
            _ => t.label().to_string(),
        };
        let face = if on { Face::Semibold } else { Face::Ui };
        let tw = p.measure(face, 13.0, &label);
        let ink = if on {
            pal.text
        } else {
            mix(pal.muted, pal.text, hv)
        };
        p.text(face, 13.0, r.cx() - tw / 2.0, r.cy(), &label, ink, r.w);
        if on {
            ul = Rect::new(r.cx() - tw / 2.0 - 4.0, tabs.bottom() - 3.0, tw + 8.0, 3.0);
        }
        g.hit(r, h);
    }
    let ul: Rect = g.anim.value("tab-underline", ul, w::slide());
    p.rbox(ul, 1.5, pal.accent);
    frost(p, tabs);

    // ---- page ----
    let page = Rect::new(0.0, tabs.bottom(), ww, wh - tabs.bottom() - STATUS_H);
    let t: f32 = g
        .anim
        .value_from(("page", s.page_epoch), 0.0_f32, 1.0, fluent::PAGE);
    let clip = p.clip(page);
    let snap = p.snapshot(page);
    let i = s.tab.index();
    let scroll: f32 = g.anim.value(("scroll", i), s.scroll[i], w::slide());
    let (dx, rise) = if s.page_dir == 0.0 {
        (0.0, 10.0 * (1.0 - t))
    } else {
        (22.0 * (1.0 - t) * s.page_dir, 0.0)
    };
    let origin = p.offset(dx, rise - scroll);
    g.dy = -scroll;
    g.clip = page;
    let content_h = match s.tab {
        Tab::Lookup => draw_lookup(s, &mut g, p, page, full),
        Tab::Check => draw_check(s, &mut g, p, page),
        Tab::Pinned => draw_pinned(s, &mut g, p, page),
        Tab::Database => draw_database(s, &mut g, p, page),
    };
    p.restore_offset(origin);
    if t < 1.0 {
        p.fade(&snap, t);
    }
    // The side panes of the full window are chrome too; marked after the page's fade, which
    // would write over the marks.
    if full && s.tab == Tab::Lookup {
        frost(p, Rect::new(0.0, page.y, 250.0, page.h));
        frost(p, Rect::new(page.right() - 290.0, page.y, 290.0, page.h));
    }
    s.content_h = content_h;
    s.view_h = page.h;
    let max_scroll = (content_h - page.h).max(0.0);
    if s.scroll[i] > max_scroll {
        s.scroll[i] = max_scroll;
    }
    // The overlay scrollbar while there is more than a page.
    if content_h > page.h + 1.0 {
        let frac = page.h / content_h;
        let th = (page.h * frac).max(28.0);
        let ty = page.y + (page.h - th) * (scroll / max_scroll.max(1.0)).clamp(0.0, 1.0);
        p.rbox_a(Rect::new(ww - 6.0, ty, 3.0, th), 1.5, pal.thumb, 0.7);
    }
    p.restore(clip);
    g.dy = 0.0;
    g.clip = win;
    // Clicks on the empty page take the keyboard from the fields.
    g.hits.insert(0, (page, Hit::Content));

    // ---- status strip ----
    let st = Rect::new(0.0, wh - STATUS_H, ww, STATUS_H);
    p.fill(st, pal.pane);
    p.fill(Rect::new(0.0, st.y, ww, 1.0), pal.line);
    let (dot, text) = status_text(s);
    p.rbox(
        Rect::new(12.0, st.cy() - 3.5, 7.0, 7.0),
        3.5,
        if dot { c.success } else { c.tag },
    );
    p.text(Face::Ui, 12.0, 26.0, st.cy(), &text, pal.muted, ww * 0.6);
    let hint = match s.tab {
        Tab::Lookup => tl(
            "Ctrl+D sabitle · Ctrl+Shift+C kopyala",
            "Ctrl+D pin · Ctrl+Shift+C copy",
        ),
        Tab::Check => tl(
            "Ctrl+Enter tümünü düzelt · .srt bırakın",
            "Ctrl+Enter fix all · drop an .srt",
        ),
        Tab::Pinned => tl("Ctrl+1–4 sekme değiştir", "Ctrl+1–4 switch tabs"),
        Tab::Database => tl(
            "Ctrl+T tema · Ctrl+B arka plan",
            "Ctrl+T theme · Ctrl+B backdrop",
        ),
    };
    let hw = p.measure(Face::Ui, 12.0, hint);
    if hw < ww * 0.45 {
        p.text(
            Face::Ui,
            12.0,
            ww - hw - 12.0,
            st.cy(),
            hint,
            pal.muted,
            f32::INFINITY,
        );
    }
    frost(p, st);

    // ---- toast ----
    if let Some(toast) = &s.toast {
        let shown = toast.until > now;
        let tt = g.anim.presence("toast", shown, fluent::ENTER, fluent::EXIT);
        let b = Bubble::toast(p, &toast.text, Rect::new(0.0, 0.0, ww, wh - STATUS_H));
        b.draw_presence(p, &pal, tt);
        if !shown && tt <= 0.0 {
            s.toast = None;
        }
    }
    // ---- a field's right-click menu, over everything ----
    if let Some((menu, _)) = &mut s.popup {
        menu.draw_in(p, &pal, win);
    }
    s.hits = g.hits;
    cx.set_maximize_button(if full { Some(max) } else { None });
    // The colours of this frame are what the chrome was drawn in: the backdrop takes out that
    // flat colour, so it follows the theme while it glides.
    cx.set_backdrop(s.prefs.backdrop, &pal);
}

fn status_text(s: &Sozluk) -> (bool, String) {
    let n = s.ix.len();
    match s.counts {
        Some((ok, nf, _)) if ok + nf >= n as u64 => (
            true,
            tf!(
                "Çevrim dışı · {} sözcük, tüm maddeler",
                "Offline · {} words, all entries",
                fmt_n(n as u64)
            ),
        ),
        Some((ok, _, _)) => (
            true,
            tf!(
                "Çevrim dışı · {} sözcük · {} madde",
                "Offline · {} words · {} entries",
                fmt_n(n as u64),
                fmt_n(ok)
            ),
        ),
        None => (
            false,
            tf!(
                "{} sözcük · maddeler sozluk.gov.tr'den",
                "{} words · entries from sozluk.gov.tr",
                fmt_n(n as u64)
            ),
        ),
    }
}

pub fn fmt_n(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(if en() { ',' } else { '.' });
        }
        out.push(ch);
    }
    out
}

// ---- Look up ----------------------------------------------------------------------------------

fn draw_lookup(s: &mut Sozluk, g: &mut Pg<'_>, p: &mut Painter, page: Rect, full: bool) -> f32 {
    if full {
        let left = Rect::new(0.0, page.y, 250.0, page.h);
        let right_w = 290.0;
        let right = Rect::new(page.right() - right_w, page.y, right_w, page.h);
        // The side panes stay put while the middle scrolls: draw them outside the scroll.
        let dy = g.dy;
        let o = p.offset(0.0, -dy);
        g.dy = 0.0;
        p.fill(left, g.c.pal.pane);
        p.fill(
            Rect::new(left.right() - 1.0, left.y, 1.0, left.h),
            g.c.pal.line,
        );
        draw_side_list(s, g, p, left.inset(6.0));
        p.fill(right, g.c.pal.pane);
        p.fill(Rect::new(right.x, right.y, 1.0, right.h), g.c.pal.line);
        draw_side_tools(
            s,
            g,
            p,
            Rect::new(
                right.x + 14.0,
                right.y + 14.0,
                right.w - 28.0,
                right.h - 28.0,
            ),
        );
        p.restore_offset(o);
        g.dy = dy;
        let mid = Rect::new(
            left.right() + 36.0,
            page.y + 24.0,
            right.x - left.right() - 72.0,
            page.h,
        );
        let h = draw_entry(s, g, p, mid, true);
        return h + 48.0;
    }
    if s.showing_results() {
        return draw_results(s, g, p, page.inset(8.0)) + 16.0;
    }
    let area = Rect::new(page.x + 16.0, page.y + 16.0, page.w - 32.0, page.h);
    let mut h = draw_entry(s, g, p, area, false);
    if let Some(cur) = s.current.clone()
        && let Lookup::Found(es) = &s.lookup
    {
        let syn = synonyms(s, es);
        if !syn.is_empty() {
            let y = area.y + h + 6.0;
            w::label(
                p,
                &g.c,
                area.x,
                y,
                tl("Anlamlarındaki eş anlamlılar", "Synonyms in its meanings"),
            );
            let labels: Vec<String> = syn.iter().map(|x| diff_label(x, &cur)).collect();
            let ch = chips_open_as(g, p, area.x, y + 22.0, area.w, &labels, &syn, "syn");
            h += 28.0 + ch + 6.0;
        }
    }
    h + 24.0
}

fn diff_label(x: &str, cur: &str) -> String {
    let d = letters(x) as i64 - letters(cur) as i64;
    if d == 0 {
        x.to_string()
    } else {
        format!("{x}  {d:+}")
    }
}

/// Chips whose labels differ from the word they open.
fn chips_open_as(
    g: &mut Pg<'_>,
    p: &mut Painter,
    x: f32,
    y: f32,
    width: f32,
    labels: &[String],
    words: &[String],
    id: &str,
) -> f32 {
    let hov = words.iter().position(|l| g.hovered(&Hit::Open(l.clone())));
    let (rects, h) = w::chip_rows(p, g.anim, &g.c, x, y, width, labels, hov, id);
    for (r, wd) in rects.into_iter().zip(words) {
        g.hit(r, Hit::Open(wd.clone()));
    }
    h
}

fn draw_results(s: &mut Sozluk, g: &mut Pg<'_>, p: &mut Painter, area: Rect) -> f32 {
    let pal = g.c.pal;
    if s.results.is_empty() {
        p.text(
            Face::Ui,
            13.0,
            area.x + 10.0,
            area.y + 20.0,
            tl("Böyle başlayan madde yok.", "No headword starts so."),
            pal.muted,
            area.w,
        );
        return 40.0;
    }
    let row_h = 32.0;
    let sel_y: f32 = g
        .anim
        .value("result-sel", area.y + s.sel as f32 * row_h, w::slide());
    p.rbox(
        Rect::new(area.x, sel_y, area.w, row_h - 2.0),
        RADIUS,
        pal.on,
    );
    for (i, r) in s.results.clone().iter().enumerate() {
        let row = Rect::new(area.x, area.y + i as f32 * row_h, area.w, row_h - 2.0);
        if i != s.sel {
            g.row(p, row, Hit::Result(i));
        } else {
            g.hit(row, Hit::Result(i));
        }
        let face = if i == s.sel { Face::Semibold } else { Face::Ui };
        p.text(
            face,
            13.5,
            row.x + 10.0,
            row.cy(),
            r,
            pal.text,
            row.w - 120.0,
        );
        if s.prefs.is_pinned(r) {
            p.icon(STAR, 12.0, row.right() - 16.0, row.cy(), pal.accent);
        }
    }
    s.results.len() as f32 * row_h
}

fn draw_side_list(s: &mut Sozluk, g: &mut Pg<'_>, p: &mut Painter, area: Rect) {
    let pal = g.c.pal;
    let mut y = area.y + 4.0;
    let row_h = 30.0;
    let section = |p: &mut Painter,
                   g: &mut Pg<'_>,
                   y: &mut f32,
                   title: &str,
                   items: &[String],
                   cur: Option<&str>| {
        w::label(p, &g.c, area.x + 8.0, *y + 4.0, title);
        *y += 26.0;
        for it in items {
            if *y + row_h > area.bottom() {
                break;
            }
            let r = Rect::new(area.x, *y, area.w, row_h - 2.0);
            if cur == Some(it.as_str()) {
                p.rbox(r, RADIUS, g.c.pal.on);
            }
            g.row(p, r, Hit::Open(it.clone()));
            let face = if cur == Some(it.as_str()) {
                Face::Semibold
            } else {
                Face::Ui
            };
            p.text(face, 13.0, r.x + 10.0, r.cy(), it, g.c.pal.text, r.w - 20.0);
            *y += row_h;
        }
        *y += 8.0;
    };
    let cur = s.current.clone();
    if !s.search.text().trim().is_empty() && !s.results.is_empty() && s.showing_results() {
        let items: Vec<String> = s.results.clone();
        let title = tf!("Eşleşmeler ({})", "Matches ({})", items.len());
        // Results open by index so the keyboard selection lines up.
        w::label(p, &g.c, area.x + 8.0, y + 4.0, &title);
        y += 26.0;
        let sel_y: f32 = g
            .anim
            .value("side-sel", y + s.sel as f32 * row_h, w::slide());
        p.rbox(
            Rect::new(area.x, sel_y, area.w, row_h - 2.0),
            RADIUS,
            pal.on,
        );
        for (i, it) in items.iter().enumerate() {
            if y + row_h > area.bottom() {
                break;
            }
            let r = Rect::new(area.x, y, area.w, row_h - 2.0);
            if i != s.sel {
                g.row(p, r, Hit::Result(i));
            } else {
                g.hit(r, Hit::Result(i));
            }
            p.text(
                if i == s.sel { Face::Semibold } else { Face::Ui },
                13.0,
                r.x + 10.0,
                r.cy(),
                it,
                pal.text,
                r.w - 20.0,
            );
            y += row_h;
        }
        return;
    }
    let pinned = s.prefs.pinned.clone();
    if !pinned.is_empty() {
        section(
            p,
            g,
            &mut y,
            tl("Sabitlenen", "Pinned"),
            &pinned,
            cur.as_deref(),
        );
    }
    let hist: Vec<String> = s.prefs.history.iter().take(30).cloned().collect();
    if !hist.is_empty() {
        section(
            p,
            g,
            &mut y,
            tl("Son bakılanlar", "Recent"),
            &hist,
            cur.as_deref(),
        );
    }
    if pinned.is_empty() && hist.is_empty() {
        p.paragraph(
            Face::Ui,
            12.5,
            area.x + 8.0,
            y + 8.0,
            area.w - 16.0,
            tl(
                "Açtığınız ve sabitlediğiniz sözcükler burada görünür.",
                "Words you open and pin show here.",
            ),
            pal.muted,
        );
    }
}

fn draw_side_tools(s: &mut Sozluk, g: &mut Pg<'_>, p: &mut Painter, area: Rect) {
    let pal = g.c.pal;
    p.text(
        Face::Semibold,
        14.0,
        area.x,
        area.y + 8.0,
        tl("Altyazı için", "For subtitles"),
        pal.text,
        area.w,
    );
    let mut y = area.y + 30.0;
    let Some(cur) = s.current.clone() else { return };
    let found = matches!(s.lookup, Lookup::Found(_));
    let syn = match &s.lookup {
        Lookup::Found(es) => synonyms(s, es),
        _ => Vec::new(),
    };
    // Synonyms with their letter difference, for a CPS cut.
    let rows = syn.len().max(1) as f32;
    let card_h = 40.0 + rows * 30.0 + 34.0;
    let card = Rect::new(area.x, y, area.w, card_h);
    w::card(p, &g.c, card);
    w::label(
        p,
        &g.c,
        card.x + 12.0,
        card.y + 8.0,
        tl("Anlamlarındaki eş anlamlılar", "Synonyms in its meanings"),
    );
    let mut ry = card.y + 34.0;
    if syn.is_empty() {
        let none = if found {
            tl(
                "TDK bu sözcük için eş anlamlı vermiyor.",
                "TDK lists none for this word.",
            )
        } else {
            tl(
                "Anlamlar henüz indirilmedi.",
                "The meanings are not downloaded yet.",
            )
        };
        p.text(
            Face::Ui,
            12.5,
            card.x + 12.0,
            ry + 12.0,
            none,
            pal.muted,
            card.w - 24.0,
        );
        ry += 30.0;
    }
    for x in &syn {
        let r = Rect::new(card.x + 4.0, ry, card.w - 8.0, 28.0);
        g.row(p, r, Hit::Open(x.clone()));
        p.text(Face::Ui, 13.0, r.x + 8.0, r.cy(), x, pal.text, r.w - 80.0);
        let d = letters(x) as i64 - letters(&cur) as i64;
        let (txt, ink) = (
            format!("{} · {d:+}", letters(x)),
            if d < 0 { g.c.success } else { pal.muted },
        );
        p.text_right(Face::Semibold, 12.0, r.right() - 8.0, r.cy(), &txt, ink);
        ry += 30.0;
    }
    p.paragraph(
        Face::Ui,
        11.5,
        card.x + 12.0,
        ry + 4.0,
        card.w - 24.0,
        tl(
            "Harf sayısı ve açık sözcüğe göre farkı.",
            "Letters, and the change against the open word.",
        ),
        pal.muted,
    );
    y += card_h + 12.0;

    // Other spellings.
    let vars: Vec<String> =
        s.ix.variants(&cur)
            .into_iter()
            .map(str::to_string)
            .collect();
    if !vars.is_empty() {
        let card = Rect::new(area.x, y, area.w, 40.0 + 30.0 * vars.len() as f32);
        w::card(p, &g.c, card);
        w::label(
            p,
            &g.c,
            card.x + 12.0,
            card.y + 8.0,
            tl("TDK'deki öteki yazımlar", "Other spellings in TDK"),
        );
        let mut ry = card.y + 34.0;
        for v in &vars {
            let r = Rect::new(card.x + 4.0, ry, card.w - 8.0, 28.0);
            g.row(p, r, Hit::Open(v.clone()));
            p.text(Face::Ui, 13.0, r.x + 8.0, r.cy(), v, pal.accent, r.w - 16.0);
            ry += 30.0;
        }
        y += card.h + 12.0;
    }
    // Copy buttons.
    let b = Rect::new(area.x, y, area.w, 32.0);
    g.btn(
        p,
        b,
        &tf!(
            "Kopyala: \u{201c}{cur}\u{201d}",
            "Copy \u{201c}{cur}\u{201d}"
        ),
        Hit::CopyHead,
        false,
    );
}

/// The open entry. Returns the height used.
fn draw_entry(s: &mut Sozluk, g: &mut Pg<'_>, p: &mut Painter, area: Rect, big: bool) -> f32 {
    let pal = g.c.pal;
    let x = area.x;
    let wid = area.w;
    let Some(cur) = s.current.clone() else {
        let y = area.y + 30.0;
        p.text(
            Face::Semibold,
            18.0,
            x,
            y,
            tl("Sözcük arayın", "Look up a word"),
            pal.text,
            wid,
        );
        p.paragraph(
            Face::Ui,
            13.0,
            x,
            y + 22.0,
            wid,
            tl("Güncel Türkçe Sözlük'ün 81.017 maddesinin tümünde aramak için yazın. Yazım farkları da bulunur: kağıt yazınca kâğıt çıkar.", "Type to search all 81,017 headwords of the Güncel Türkçe Sözlük. Spelling variants are found too: kağıt finds kâğıt."),
            pal.muted,
        );
        return 120.0;
    };
    let mut y = area.y;
    let size = if big { 30.0 } else { 24.0 };
    let hw = p.text(
        Face::Semibold,
        size,
        x,
        y + size * 0.6,
        &cur,
        pal.text,
        wid - 90.0,
    );
    let pron = match &s.lookup {
        Lookup::Found(es) => es
            .first()
            .map(|e| e.pronunciation.clone())
            .unwrap_or_default(),
        _ => String::new(),
    };
    if !pron.is_empty() {
        p.text(
            Face::Ui,
            13.5,
            x + hw + 10.0,
            y + size * 0.66,
            &pron,
            pal.muted,
            160.0,
        );
    }
    let pin = Rect::new(x + wid - 30.0, y + size * 0.6 - 15.0, 30.0, 30.0);
    let copy = Rect::new(pin.x - 34.0, pin.y, 30.0, 30.0);
    g.icon_btn(p, copy, glyph::COPY, Hit::CopyHead, false);
    let pinned = s.prefs.is_pinned(&cur);
    g.icon_btn(p, pin, STAR, Hit::PinHead, pinned);
    y += size * 1.2 + 6.0;

    // Meta: word types, meanings, letters.
    let mut meta: Vec<String> = Vec::new();
    if let Lookup::Found(es) = &s.lookup {
        let mut tags: Vec<String> = Vec::new();
        for e in es {
            for m in &e.meanings {
                if let Some(t) = m.tags.first()
                    && !tags.contains(t)
                    && [
                        "isim", "sıfat", "zarf", "fiil", "zamir", "edat", "bağlaç", "ünlem",
                    ]
                    .contains(&t.as_str())
                {
                    tags.push(t.clone());
                }
            }
        }
        if !tags.is_empty() {
            meta.push(tags.join(", "));
        }
        let n: usize = es.iter().map(|e| e.meanings.len()).sum();
        meta.push(if !en() {
            format!("{n} anlam")
        } else if n == 1 {
            "1 meaning".into()
        } else {
            format!("{n} meanings")
        });
    }
    meta.push(tf!("{} harf", "{} letters", letters(&cur)));
    p.text(
        Face::Ui,
        12.0,
        x,
        y + 8.0,
        &meta.join(" · "),
        pal.muted,
        wid,
    );
    y += 26.0;

    // Other spellings, as an InfoBar (compact; the full window has them in the side pane).
    let vars: Vec<String> =
        s.ix.variants(&cur)
            .into_iter()
            .map(str::to_string)
            .collect();
    if !vars.is_empty() && !big {
        let v = &vars[0];
        let how = if v.contains(' ') != cur.contains(' ') {
            if v.contains(' ') {
                tl("Ayrı yazılışı da var", "Also written apart")
            } else {
                tl("Bitişik yazılışı da var", "Also written together")
            }
        } else {
            tl("TDK'de ayrıca", "Also in TDK")
        };
        let extra = match s.store.lookup(v) {
            Lookup::Found(es) => es
                .first()
                .and_then(|e| {
                    e.meanings.first().map(|m| {
                        format!(
                            " ({}) \u{201c}{}\u{201d}",
                            m.tags.first().cloned().unwrap_or_default(),
                            m.text
                        )
                    })
                })
                .unwrap_or_default()
                .replace(" () ", " "),
            _ => String::new(),
        };
        let text = format!("{how}: {v}{extra}");
        let h = w::infobar(
            p,
            &g.c,
            x,
            y,
            wid - 52.0,
            glyph::INFO,
            pal.accent,
            g.c.info_bg,
            g.c.info_line,
            &text,
        );
        let open = Rect::new(x + wid - 48.0, y, 48.0, h);
        g.row(p, open, Hit::Open(v.clone()));
        p.text_center(Face::Ui, 12.5, open, tl("Aç", "Open"), pal.accent);
        y += h + 14.0;
    }

    match s.lookup.clone() {
        Lookup::Found(es) => {
            let many = es.len() > 1;
            for (ei, e) in es.iter().enumerate() {
                if many {
                    p.text(
                        Face::Semibold,
                        12.0,
                        x,
                        y + 8.0,
                        &format!("{} {}", e.headword, ei + 1),
                        g.c.tag,
                        wid,
                    );
                    y += 22.0;
                }
                for (mi, m) in e.meanings.iter().enumerate() {
                    let n = Rect::new(x, y, 22.0, 22.0);
                    let first = ei == 0 && mi == 0;
                    p.rbox(n, 11.0, if first { pal.accent } else { pal.hover });
                    p.text_center(
                        Face::Semibold,
                        11.5,
                        n,
                        &(mi + 1).to_string(),
                        if first { pal.on_accent } else { pal.text },
                    );
                    let tx = x + 34.0;
                    let tw = wid - 34.0;
                    let mut my = y;
                    if !m.tags.is_empty() {
                        p.text(
                            Face::Ui,
                            12.0,
                            tx,
                            my + 8.0,
                            &m.tags.join(", "),
                            g.c.tag,
                            tw,
                        );
                        my += 18.0;
                    }
                    // A redirect (► x) opens its target.
                    if let Some(target) = m.text.strip_prefix('►') {
                        let target = target
                            .trim()
                            .split(" (")
                            .next()
                            .unwrap_or("")
                            .trim()
                            .to_string();
                        let tw2 = p.measure(Face::Ui, 14.0, &format!("→ {target}"));
                        let r = Rect::new(tx - 4.0, my, tw2 + 8.0, 24.0);
                        g.row(p, r, Hit::Open(target.clone()));
                        p.text(
                            Face::Ui,
                            14.0,
                            tx,
                            my + 12.0,
                            &format!("→ {target}"),
                            pal.accent,
                            tw,
                        );
                        my += 26.0;
                    } else {
                        my +=
                            p.paragraph(Face::Ui, 14.0, tx, my + 2.0, tw, &m.text, pal.text) + 4.0;
                    }
                    if let Some((ex, author)) = &m.example {
                        let t = match author {
                            Some(a) => format!("\u{201c}{ex}\u{201d} — {a}"),
                            None => format!("\u{201c}{ex}\u{201d}"),
                        };
                        my += p.paragraph(Face::Ui, 12.5, tx, my + 2.0, tw, &t, pal.muted) + 4.0;
                    }
                    y = my.max(y + 24.0) + 12.0;
                }
                if !e.origin.is_empty() {
                    p.text(
                        Face::Ui,
                        12.0,
                        x,
                        y + 8.0,
                        &tf!("Köken: {}", "Origin: {}", e.origin),
                        pal.muted,
                        wid,
                    );
                    y += 26.0;
                }
                if !e.compounds.is_empty() {
                    p.fill(Rect::new(x, y, wid, 1.0), pal.line);
                    y += 12.0;
                    w::label(p, &g.c, x, y, tl("Birleşik sözcükler", "Compounds"));
                    y += 22.0;
                    let comp: Vec<String> = e.compounds.iter().take(40).cloned().collect();
                    y += g.chips(p, x, y, wid, &comp, &format!("comp{ei}"), true) + 14.0;
                }
                if !e.idioms.is_empty() {
                    w::label(
                        p,
                        &g.c,
                        x,
                        y,
                        tl("Atasözleri ve deyimler", "Idioms and sayings"),
                    );
                    y += 24.0;
                    for id in e.idioms.iter().take(30) {
                        let r = Rect::new(x - 6.0, y, wid + 6.0, 26.0);
                        if s.ix.contains(id) {
                            g.row(p, r, Hit::Open(id.clone()));
                        }
                        p.text(
                            Face::Ui,
                            13.0,
                            x,
                            r.cy(),
                            id,
                            if s.ix.contains(id) {
                                pal.accent
                            } else {
                                pal.text
                            },
                            wid,
                        );
                        y += 27.0;
                    }
                    y += 8.0;
                }
            }
        }
        Lookup::NotFound => {
            y += w::infobar(
                p,
                &g.c,
                x,
                y,
                wid,
                glyph::INFO,
                pal.muted,
                g.c.card,
                pal.line,
                tl(
                    "TDK'de tam bu biçimde bir madde yok. Kökünü ya da başka bir yazımı deneyin.",
                    "TDK has no entry under this exact form. Try the stem, or another spelling.",
                ),
            );
        }
        Lookup::Missing => {
            let pending = s.live_pending.contains(&cur);
            let (glyph_, text) = if !s.prefs.live {
                (glyph::INFO, tl("Bu madde henüz indirilmedi; sozluk.gov.tr'ye sorma kapalı (Ayarlar sekmesi).", "This entry is not downloaded yet, and asking sozluk.gov.tr is off (Settings tab).").to_string())
            } else if pending {
                (
                    glyph::GLOBE,
                    tl("sozluk.gov.tr'ye soruluyor…", "Asking sozluk.gov.tr…").to_string(),
                )
            } else if let Some(e) = &s.live_error {
                (
                    glyph::WARN,
                    tf!(
                        "sozluk.gov.tr'ye ulaşılamadı: {e}",
                        "Could not reach sozluk.gov.tr: {e}"
                    ),
                )
            } else {
                (
                    glyph::GLOBE,
                    tl("Henüz indirilmedi.", "Not downloaded yet.").to_string(),
                )
            };
            let h = w::infobar(
                p,
                &g.c,
                x,
                y,
                wid,
                glyph_,
                if s.live_error.is_some() {
                    pal.warn
                } else {
                    pal.accent
                },
                if s.live_error.is_some() {
                    g.c.warn_card
                } else {
                    g.c.info_bg
                },
                if s.live_error.is_some() {
                    g.c.warn_line
                } else {
                    g.c.info_line
                },
                &text,
            );
            y += h + 10.0;
            if pending {
                // A soft pulse while the request is out.
                let ph = mdshared_ui::anim::Periodic::new(std::time::Duration::from_millis(1100))
                    .ping_pong()
                    .phase(g.anim, "live-pulse");
                for k in 0..3 {
                    let a = (ph * 3.0 - k as f32).clamp(0.0, 1.0);
                    p.rbox_a(
                        Rect::new(x + 8.0 + k as f32 * 14.0, y, 8.0, 8.0),
                        4.0,
                        pal.accent,
                        0.25 + 0.75 * a,
                    );
                }
                y += 18.0;
            } else if s.live_error.is_some() {
                let b = Rect::new(x, y, 90.0, 30.0);
                g.btn(p, b, tl("Yeniden dene", "Try again"), Hit::Retry, false);
                y += 40.0;
            }
        }
    }
    y - area.y
}

// ---- Check ------------------------------------------------------------------------------------

fn draw_check(s: &mut Sozluk, g: &mut Pg<'_>, p: &mut Painter, page: Rect) -> f32 {
    let pal = g.c.pal;
    let x = page.x + 12.0;
    let wid = page.w - 24.0;
    let mut y = page.y + 12.0;

    // The field and Paste.
    let paste_w = 80.0;
    let field = Rect::new(x, y, wid - paste_w - 8.0, 34.0);
    p.frame(field, RADIUS, pal.field, pal.line);
    let ft = w::hover_t(g.anim, "check-focus", s.check_focus);
    let grow = (field.w - 2.0) * ft;
    p.fill(
        Rect::new(
            field.x + 1.0 + (field.w - 2.0 - grow) / 2.0,
            field.bottom() - 2.0,
            grow,
            2.0,
        ),
        pal.accent,
    );
    let inner = Rect::new(field.x + 10.0, field.y + 4.0, field.w - 40.0, field.h - 8.0);
    s.check_x = inner.x;
    s.check_area = Rect::new(inner.x, inner.y + g.dy, inner.w, inner.h);
    let colors = FieldColors {
        text: pal.text,
        placeholder: pal.muted,
        selection: pal.selection,
        caret: pal.text,
    };
    s.check_field.draw_blinking(
        p,
        g.anim,
        "check",
        s.blink,
        inner,
        Face::Ui,
        14.0,
        &colors,
        tl(
            "Bir satır yazın, yapıştırın ya da .srt bırakın",
            "Type a line, paste, or drop an .srt",
        ),
        s.check_focus,
    );
    s.check_stops = p.caret_stops(Face::Ui, 14.0, &s.check_field.display());
    g.hit(
        Rect::new(field.x, field.y, field.w - 30.0, field.h),
        Hit::CheckField,
    );
    if !s.check_text.is_empty() {
        g.icon_btn(
            p,
            Rect::new(field.right() - 31.0, field.y + 3.0, 28.0, 28.0),
            glyph::CLOSE,
            Hit::CheckClear,
            false,
        );
    }
    g.btn(
        p,
        Rect::new(field.right() + 8.0, y, paste_w, 34.0),
        tl("Yapıştır", "Paste"),
        Hit::CheckPaste,
        false,
    );
    y += 46.0;

    // Watch the clipboard.
    let tr = Rect::new(x, y, 38.0, 20.0);
    w::toggle(p, g.anim, &g.c, tr, s.prefs.watch_clipboard, "watch");
    p.text(
        Face::Ui,
        12.5,
        x + 48.0,
        tr.cy(),
        tl("Kopyaladığımı denetle", "Check what I copy"),
        pal.text,
        wid,
    );
    g.hit(Rect::new(x, y - 4.0, 180.0, 28.0), Hit::CheckWatch);
    let lines = s.check_text.lines().count();
    let words = s.check_tokens.len();
    if words > 0 {
        p.text_right(
            Face::Ui,
            12.0,
            x + wid,
            tr.cy(),
            &if en() {
                format!(
                    "{lines} {} · {words} {}",
                    if lines == 1 { "line" } else { "lines" },
                    if words == 1 { "word" } else { "words" }
                )
            } else {
                format!("{lines} satır · {words} sözcük")
            },
            pal.muted,
        );
    }
    y += 32.0;

    if s.check_text.trim().is_empty() {
        let h = w::infobar(
            p,
            &g.c,
            x,
            y,
            wid,
            glyph::INFO,
            pal.accent,
            g.c.info_bg,
            g.c.info_line,
            tl(
                "Her sözcük TDK'de, köküyle de aranır: kâğıdı yazınca kâğıt bulunur. Yazım farkları tek tıkla düzeltilir; hala / hâlâ gibi ikisi de sözlükte olan çiftler, iki anlamıyla birlikte size bırakılır.",
                "Every word is looked up in TDK, by its stem too: kâğıdı finds kâğıt. Spelling differences are fixed in one click; pairs such as hala / hâlâ are left for you, with both meanings.",
            ),
        );
        return y + h - page.y + 12.0;
    }

    // The text, words marked.
    let tw = wid - 28.0;
    let start_y = y;
    let mut ty = y + 14.0;
    let line_h = 28.0;
    let mut tx = x + 14.0;
    let text = s.check_text.clone();
    let toks = s.check_tokens.clone();
    let mut at = 0usize;
    let mut pieces: Vec<(String, Option<usize>)> = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        if t.start > at {
            pieces.push((text[at..t.start].to_string(), None));
        }
        pieces.push((t.word.clone(), Some(i)));
        at = t.end;
    }
    if at < text.len() {
        pieces.push((text[at..].to_string(), None));
    }
    // Draw the card first, then the words over it: the height is measured in a first pass.
    let measure_pass = |p: &mut Painter| -> f32 {
        let (mut mx, mut my) = (x + 14.0, y + 14.0);
        for (piece, _) in &pieces {
            for (li, part) in piece.split('\n').enumerate() {
                if li > 0 {
                    mx = x + 14.0;
                    my += line_h;
                }
                for word in part.split_inclusive(' ') {
                    let ww = p.measure(Face::Ui, 15.0, word);
                    if mx + ww > x + 14.0 + tw && mx > x + 14.0 {
                        mx = x + 14.0;
                        my += line_h;
                    }
                    mx += ww;
                }
            }
        }
        my + line_h - y + 6.0
    };
    let card_h = measure_pass(p);
    p.frame(
        Rect::new(x, start_y, wid, card_h),
        RADIUS,
        pal.field,
        pal.line,
    );
    for (piece, tok) in &pieces {
        let kind = tok.map(|i| toks[i].kind.clone());
        let kept = tok.is_some_and(|i| s.kept.contains(&lower(&toks[i].word)));
        for (li, part) in piece.split('\n').enumerate() {
            if li > 0 {
                tx = x + 14.0;
                ty += line_h;
            }
            for word in part.split_inclusive(' ') {
                let ww = p.measure(Face::Ui, 15.0, word);
                if tx + ww > x + 14.0 + tw && tx > x + 14.0 {
                    tx = x + 14.0;
                    ty += line_h;
                }
                let (ink, bg) = match (&kind, kept) {
                    (Some(Kind::Spelling { .. }), false) => (pal.warn, Some(g.c.warn_card)),
                    (Some(Kind::Pair { .. }), false) => (pal.accent, Some(g.c.info_bg)),
                    (Some(Kind::Name), _) => (pal.muted, None),
                    (Some(Kind::Unknown), _) => (pal.text, None),
                    _ => (pal.text, None),
                };
                let cy = ty + line_h / 2.0;
                if let Some(bg) = bg {
                    let r = Rect::new(
                        tx - 2.0,
                        cy - 11.0,
                        p.measure(Face::Ui, 15.0, word.trim_end()) + 4.0,
                        22.0,
                    );
                    p.rbox(r, 4.0, bg);
                    p.fill(Rect::new(r.x + 2.0, r.bottom() - 2.0, r.w - 4.0, 1.5), ink);
                }
                if matches!(kind, Some(Kind::Unknown)) {
                    // Dotted underline: not found.
                    let uw = p.measure(Face::Ui, 15.0, word.trim_end());
                    let mut ux = tx;
                    while ux < tx + uw {
                        p.fill(Rect::new(ux, cy + 9.0, 2.0, 1.0), pal.faint);
                        ux += 4.0;
                    }
                }
                p.text(Face::Ui, 15.0, tx, cy, word, ink, f32::INFINITY);
                tx += ww;
            }
        }
    }
    y = start_y + card_h + 14.0;

    // Findings.
    let findings = s.findings();
    let fixable = findings
        .iter()
        .filter(|&&i| matches!(toks[i].kind, Kind::Spelling { .. }))
        .count();
    let head = match findings.len() {
        0 => tl("Düzeltilecek bir şey yok", "Nothing to fix").to_string(),
        1 => tl("1 bulgu", "1 finding").to_string(),
        n => tf!("{n} bulgu", "{n} findings"),
    };
    p.text(Face::Semibold, 14.0, x, y + 15.0, &head, pal.text, wid);
    if fixable > 0 {
        let label = tf!("Düzelt ({fixable}) ve kopyala", "Fix {fixable} and copy");
        let bw = w::text_button_w(p, &label);
        g.btn(
            p,
            Rect::new(x + wid - bw, y, bw, 30.0),
            &label,
            Hit::CheckFixAll,
            true,
        );
    }
    y += 42.0;
    for &i in &findings {
        let t = &toks[i];
        match &t.kind {
            Kind::Spelling { fix, why } => {
                let r = Rect::new(x, y, wid, 40.0);
                p.frame(r, RADIUS, g.c.card, pal.line);
                p.rbox(Rect::new(r.x + 12.0, r.cy() - 4.0, 8.0, 8.0), 2.0, pal.warn);
                let ow = p.text(
                    Face::Ui,
                    13.5,
                    r.x + 30.0,
                    r.cy(),
                    &t.word,
                    pal.muted,
                    160.0,
                );
                p.fill(Rect::new(r.x + 30.0, r.cy(), ow, 1.0), pal.muted);
                let aw = p.text(
                    Face::Ui,
                    13.5,
                    r.x + 36.0 + ow,
                    r.cy(),
                    "→",
                    pal.muted,
                    20.0,
                );
                let fw = p.text(
                    Face::Semibold,
                    13.5,
                    r.x + 42.0 + ow + aw,
                    r.cy(),
                    fix,
                    pal.text,
                    180.0,
                );
                let note_x = r.x + 52.0 + ow + aw + fw;
                let bw = 54.0;
                if note_x + 80.0 < r.right() - bw - 8.0 {
                    p.text(
                        Face::Ui,
                        11.5,
                        note_x,
                        r.cy(),
                        why_text(why),
                        pal.muted,
                        r.right() - bw - 16.0 - note_x,
                    );
                }
                g.btn(
                    p,
                    Rect::new(r.right() - bw - 6.0, r.y + 6.0, bw, 28.0),
                    tl("Düzelt", "Fix"),
                    Hit::Apply(i, fix.clone()),
                    false,
                );
                y += 46.0;
            }
            Kind::Pair { other } => {
                let base = |w: &str| -> String {
                    // The stem's meaning when the word is inflected.
                    let l = lower(w);
                    let stem = (2..=l.chars().count())
                        .rev()
                        .map(|n| l.chars().take(n).collect::<String>())
                        .find(|st| s.ix.contains(st));
                    stem.unwrap_or(l)
                };
                let mean = |w: &str| -> String {
                    let b = base(w);
                    let head =
                        s.ix.exact(&b)
                            .first()
                            .map(|x| x.to_string())
                            .unwrap_or(b.clone());
                    match s.store.lookup(&head) {
                        Lookup::Found(es) => es
                            .first()
                            .and_then(|e| e.meanings.first())
                            .map(|m| {
                                format!(
                                    "{} {}: {}",
                                    head,
                                    m.tags.first().cloned().unwrap_or_default(),
                                    m.text
                                )
                            })
                            .unwrap_or(head),
                        _ => tf!(
                            "{head}: anlamı henüz indirilmedi",
                            "{head}: meaning not downloaded yet"
                        ),
                    }
                };
                let m1 = mean(&t.word);
                let m2 = mean(other);
                let tw2 = wid - 24.0;
                let h1 = p.paragraph_height(Face::Ui, 12.5, tw2, &m1);
                let h2 = p.paragraph_height(Face::Ui, 12.5, tw2, &m2);
                let r = Rect::new(x, y, wid, 44.0 + h1 + h2 + 44.0);
                p.frame(r, RADIUS, g.c.card, g.c.info_line);
                p.rbox(Rect::new(r.x + 12.0, r.y + 16.0, 8.0, 8.0), 2.0, pal.accent);
                p.text(
                    Face::Semibold,
                    13.5,
                    r.x + 30.0,
                    r.y + 20.0,
                    &if en() {
                        format!("{} or {}?", t.word, other)
                    } else {
                        format!(
                            "{} {}, {} {}?",
                            t.word,
                            crate::lang::mi(&t.word),
                            other,
                            crate::lang::mi(other)
                        )
                    },
                    pal.text,
                    r.w - 40.0,
                );
                let mut my = r.y + 38.0;
                my += p.paragraph(Face::Ui, 12.5, r.x + 12.0, my, tw2, &m1, pal.muted) + 2.0;
                my += p.paragraph(Face::Ui, 12.5, r.x + 12.0, my, tw2, &m2, pal.muted) + 8.0;
                let keep_w = w::text_button_w(p, tl("Böyle kalsın", "Keep"));
                g.btn(
                    p,
                    Rect::new(r.x + 12.0, my, keep_w, 28.0),
                    tl("Böyle kalsın", "Keep"),
                    Hit::Keep(t.word.clone()),
                    false,
                );
                let use_l = tf!("Bunu yaz: {other}", "Use {other}");
                let uw = w::text_button_w(p, &use_l);
                g.btn(
                    p,
                    Rect::new(r.x + 20.0 + keep_w, my, uw, 28.0),
                    &use_l,
                    Hit::Apply(i, other.clone()),
                    true,
                );
                y += r.h + 6.0;
            }
            _ => {}
        }
    }
    // Words not found, and names, as chips.
    let unknown: Vec<String> = {
        let mut v: Vec<String> = Vec::new();
        for t in &toks {
            if t.kind == Kind::Unknown && !v.contains(&t.word) {
                v.push(t.word.clone());
            }
        }
        v
    };
    if !unknown.is_empty() {
        y += 6.0;
        w::label(
            p,
            &g.c,
            x,
            y,
            tl(
                "Bulunamadı (kökü de bulunamadı)",
                "Not found (and no stem found)",
            ),
        );
        y += 22.0;
        let hov = unknown
            .iter()
            .position(|u| g.hovered(&Hit::Ignore(u.clone())));
        let (rects, h) = w::chip_rows(p, g.anim, &g.c, x, y, wid, &unknown, hov, "unknown");
        for (r, u) in rects.into_iter().zip(&unknown) {
            g.hit(r, Hit::Ignore(u.clone()));
        }
        y += h + 6.0;
        p.text(
            Face::Ui,
            11.5,
            x,
            y + 8.0,
            tl(
                "Tıkladığınız sözcük bundan sonra özel ad sayılır.",
                "Click one to take it as a name from now on.",
            ),
            pal.muted,
            wid,
        );
        y += 22.0;
    }
    let names: Vec<String> = {
        let mut v: Vec<String> = Vec::new();
        for t in &toks {
            if t.kind == Kind::Name && !v.contains(&t.word) {
                v.push(t.word.clone());
            }
        }
        v
    };
    if !names.is_empty() {
        y += 6.0;
        p.paragraph(
            Face::Ui,
            12.0,
            x,
            y,
            wid,
            &tf!(
                "Özel ad sayılanlar: {}",
                "Taken as names: {}",
                names.join(", ")
            ),
            pal.muted,
        );
        y += 24.0;
    }
    y - page.y + 12.0
}

// ---- Pinned -----------------------------------------------------------------------------------

fn draw_pinned(s: &mut Sozluk, g: &mut Pg<'_>, p: &mut Painter, page: Rect) -> f32 {
    let pal = g.c.pal;
    let x = page.x + 12.0;
    let wid = page.w - 24.0;
    let mut y = page.y + 12.0;
    let pinned = s.prefs.pinned.clone();
    w::label(p, &g.c, x + 4.0, y, tl("Sabitlenen", "Pinned"));
    y += 24.0;
    if pinned.is_empty() {
        y += w::infobar(
            p,
            &g.c,
            x,
            y,
            wid,
            STAR,
            pal.accent,
            g.c.info_bg,
            g.c.info_line,
            tl(
                "Sözcükleri burada tutmak için yıldızla ya da Ctrl+D ile sabitleyin.",
                "Pin a word with the star or Ctrl+D to keep it here.",
            ),
        ) + 12.0;
    }
    for wd in &pinned {
        let r = Rect::new(x, y, wid, 34.0);
        g.row(p, r, Hit::Open(wd.clone()));
        p.icon(STAR, 13.0, r.x + 16.0, r.cy(), pal.accent);
        p.text(Face::Ui, 13.5, r.x + 32.0, r.cy(), wd, pal.text, r.w - 80.0);
        let gloss = match s.store.lookup(wd) {
            Lookup::Found(es) => es
                .first()
                .and_then(|e| e.meanings.first())
                .map(|m| m.text.clone())
                .unwrap_or_default(),
            _ => String::new(),
        };
        let nw = p.measure(Face::Ui, 13.5, wd);
        p.text(
            Face::Ui,
            12.0,
            r.x + 44.0 + nw,
            r.cy(),
            &gloss,
            pal.muted,
            r.w - 90.0 - nw,
        );
        let un = Rect::new(r.right() - 32.0, r.y + 3.0, 28.0, 28.0);
        g.icon_btn(p, un, glyph::CLOSE, Hit::Unpin(wd.clone()), false);
        y += 36.0;
    }
    y += 12.0;
    let hist: Vec<String> = s.prefs.history.iter().take(20).cloned().collect();
    if !hist.is_empty() {
        w::label(p, &g.c, x + 4.0, y, tl("Son bakılanlar", "Recent"));
        y += 24.0;
        y += g.chips(p, x, y, wid, &hist, "recent", true) + 12.0;
    }
    y - page.y
}

// ---- Database ---------------------------------------------------------------------------------

fn draw_database(s: &mut Sozluk, g: &mut Pg<'_>, p: &mut Painter, page: Rect) -> f32 {
    let pal = g.c.pal;
    let x = page.x + 12.0;
    let wid = (page.w - 24.0).min(640.0);
    let mut y = page.y + 12.0;
    let n = s.ix.len() as u64;

    // Appearance.
    let card = Rect::new(x, y, wid, 190.0);
    w::card(p, &g.c, card);
    p.text(
        Face::Semibold,
        13.5,
        card.x + 14.0,
        card.y + 20.0,
        tl("Görünüm", "Appearance"),
        pal.text,
        wid,
    );
    let seg_w = (card.w - 130.0).min(300.0);
    p.text(
        Face::Ui,
        12.5,
        card.x + 14.0,
        card.y + 56.0,
        tl("Tema", "Theme"),
        pal.muted,
        100.0,
    );
    let r = Rect::new(card.right() - 14.0 - seg_w, card.y + 42.0, seg_w, 30.0);
    let hov = match &g.hover {
        Some(Hit::Theme(i)) => Some(*i),
        _ => None,
    };
    let segs = w::segmented_text(
        p,
        g.anim,
        &g.c,
        r,
        &[
            tl("Sistem", "System"),
            tl("Açık", "Light"),
            tl("Koyu", "Dark"),
        ],
        s.prefs.mode.index(),
        hov,
        "theme-card",
    );
    for (i, r) in segs.into_iter().enumerate() {
        g.hit(r, Hit::Theme(i));
    }
    p.text(
        Face::Ui,
        12.5,
        card.x + 14.0,
        card.y + 96.0,
        tl("Arka plan", "Backdrop"),
        pal.muted,
        100.0,
    );
    let r = Rect::new(card.right() - 14.0 - seg_w, card.y + 82.0, seg_w, 30.0);
    let hov = match &g.hover {
        Some(Hit::Backdrop(i)) => Some(*i),
        _ => None,
    };
    let segs = w::segmented_text(
        p,
        g.anim,
        &g.c,
        r,
        &[tl("Kapalı", "Off"), "Mica", tl("Akrilik", "Acrylic")],
        s.prefs.backdrop.index(),
        hov,
        "backdrop",
    );
    for (i, r) in segs.into_iter().enumerate() {
        g.hit(r, Hit::Backdrop(i));
    }
    p.text(
        Face::Ui,
        12.5,
        card.x + 14.0,
        card.y + 136.0,
        "Dil · Language",
        pal.muted,
        110.0,
    );
    let r = Rect::new(card.right() - 14.0 - seg_w, card.y + 122.0, seg_w, 30.0);
    let hov = match &g.hover {
        Some(Hit::Lang(i)) => Some(*i),
        _ => None,
    };
    let names: Vec<&str> = crate::lang::Lang::ALL.iter().map(|l| l.name()).collect();
    let segs = w::segmented_text(
        p,
        g.anim,
        &g.c,
        r,
        &names,
        s.prefs.lang.index(),
        hov,
        "lang",
    );
    for (i, r) in segs.into_iter().enumerate() {
        g.hit(r, Hit::Lang(i));
    }
    p.text(
        Face::Ui,
        11.5,
        card.x + 14.0,
        card.y + 172.0,
        tl(
            "Windows 11: çubukların ve yan bölmelerin arkasında, pencere etkinken.",
            "Windows 11: behind the bars and side panes, while the window is active.",
        ),
        pal.muted,
        card.w - 28.0,
    );
    y += card.h + 12.0;

    if let Some(until) = s.live_blocked_until
        && until > Instant::now()
    {
        let secs = (until - Instant::now()).as_secs();
        y += w::infobar(
            p,
            &g.c,
            x,
            y,
            wid,
            glyph::WARN,
            pal.warn,
            g.c.warn_card,
            g.c.warn_line,
            &tf!(
                "sozluk.gov.tr yavaşlamamızı istedi. Canlı aramalar {secs} sn daha bekleyecek; yerel olan her şey çalışıyor.",
                "sozluk.gov.tr asked us to slow down. Live lookups wait {secs} s more; everything local keeps working."
            ),
        ) + 10.0;
    }

    // Full entries.
    let card_h = 150.0;
    let card = Rect::new(x, y, wid, card_h);
    w::card(p, &g.c, card);
    p.text(
        Face::Semibold,
        13.5,
        card.x + 14.0,
        card.y + 20.0,
        tl("Tam maddeler", "Full entries"),
        pal.text,
        wid,
    );
    let (ok, nf, err) = s.counts.unwrap_or((0, 0, 0));
    let done = (ok + nf) as f32 / n.max(1) as f32;
    let bar = Rect::new(card.x + 14.0, card.y + 40.0, card.w - 28.0, 4.0);
    p.rbox(bar, 2.0, pal.hover);
    let shown: f32 = g.anim.value("db-progress", done, fluent::MOVE_LARGE);
    p.rbox(
        Rect::new(bar.x, bar.y, bar.w * shown.clamp(0.0, 1.0), 4.0),
        2.0,
        pal.accent,
    );
    let line = |p: &mut Painter, y: f32, k: &str, v: &str| {
        p.text(
            Face::Ui,
            12.5,
            card.x + 14.0,
            y,
            k,
            pal.muted,
            card.w * 0.55,
        );
        p.text_right(Face::Ui, 12.5, card.right() - 14.0, y, v, pal.text);
    };
    if s.store.has_crawl() {
        line(
            p,
            card.y + 62.0,
            tl("İndirilen", "Downloaded"),
            &tf!(
                "{} / {} (%{})",
                "{} of {} ({}%)",
                fmt_n(ok + nf),
                fmt_n(n),
                crate::lang::decimal(f64::from(done) * 100.0, 1)
            ),
        );
        line(p, card.y + 84.0, tl("Bulunan", "Found"), &fmt_n(ok));
        line(
            p,
            card.y + 106.0,
            tl("Bu biçimde bulunmayan", "Not found under that form"),
            &fmt_n(nf),
        );
        line(
            p,
            card.y + 128.0,
            tl("Yeniden denenecek", "To retry"),
            &fmt_n(err),
        );
    } else {
        p.paragraph(
            Face::Ui,
            12.5,
            card.x + 14.0,
            card.y + 54.0,
            card.w - 28.0,
            tl("gts.sqlite bulunamadı. Tarama dosyasını sozluk.exe'nin yanına ya da %LOCALAPPDATA%\\sozluk klasörüne koyun ya da SOZLUK_DB'yi ayarlayın. Altyazı becerisinin kopyası kendiliğinden bulunur.", "No gts.sqlite found. Put the crawl's file next to sozluk.exe or in %LOCALAPPDATA%\\sozluk, or set SOZLUK_DB. The subtitle skill's copy is found by itself."),
            pal.muted,
        );
    }
    y += card_h + 12.0;

    // Offline dictionary: the crawl server's snapshot.
    let card = Rect::new(x, y, wid, 128.0);
    w::card(p, &g.c, card);
    p.text(
        Face::Semibold,
        13.5,
        card.x + 14.0,
        card.y + 20.0,
        tl("Çevrimdışı sözlük", "Offline dictionary"),
        pal.text,
        wid,
    );
    let mb = |n: u64| crate::lang::decimal(n as f64 / 1_048_576.0, 1);
    let (state, warn, part) = match &s.dict {
        Some(Event::Checking) => (tl("Denetleniyor…", "Checking…").to_string(), false, None),
        Some(Event::Progress(n, Some(t))) => (
            tf!(
                "İndiriliyor: {} / {} MB",
                "Downloading: {} of {} MB",
                mb(*n),
                mb(*t)
            ),
            false,
            Some(*n as f32 / (*t).max(1) as f32),
        ),
        Some(Event::Progress(n, None)) => (
            tf!("İndiriliyor: {} MB", "Downloading: {} MB", mb(*n)),
            false,
            None,
        ),
        Some(Event::UpToDate(snap)) => (
            tf!("Güncel ({})", "Up to date ({})", snap.written),
            false,
            None,
        ),
        Some(Event::Ready(snap)) => (
            tf!("Güncellendi ({})", "Updated ({})", snap.written),
            false,
            None,
        ),
        Some(Event::Failed(why)) => (tf!("Olmadı: {}", "Failed: {}", why), true, None),
        None => match &s.dict_installed {
            Some(snap) => (
                tf!("Kurulu: {}", "Installed: {}", snap.written),
                false,
                None,
            ),
            None => (
                tl("Henüz indirilmedi", "Not downloaded yet").to_string(),
                false,
                None,
            ),
        },
    };
    p.text(
        Face::Ui,
        12.5,
        card.x + 14.0,
        card.y + 46.0,
        &state,
        if warn { pal.warn } else { pal.muted },
        card.w - 28.0,
    );
    if let Some(t) = part {
        let bar = Rect::new(card.x + 14.0, card.y + 60.0, card.w - 28.0, 4.0);
        p.rbox(bar, 2.0, pal.hover);
        p.rbox(
            Rect::new(bar.x, bar.y, bar.w * t.clamp(0.0, 1.0), 4.0),
            2.0,
            pal.accent,
        );
    }
    let tr = Rect::new(card.right() - 52.0, card.y + 72.0, 38.0, 20.0);
    w::toggle(p, g.anim, &g.c, tr, s.prefs.auto_dict, "auto-dict");
    p.text(
        Face::Ui,
        13.0,
        card.x + 14.0,
        tr.cy(),
        tl(
            "Günde bir kez denetle, yenisi varsa indir",
            "Check once a day and download a newer one",
        ),
        pal.text,
        card.w - 80.0,
    );
    g.hit(
        Rect::new(card.x, card.y + 66.0, card.w, 32.0),
        Hit::DictAuto,
    );
    if !s.dict_busy {
        let label = tl("Şimdi denetle", "Check now");
        let bw = w::text_button_w(p, label);
        g.btn(
            p,
            Rect::new(card.x + 14.0, card.y + 96.0, bw, 26.0),
            label,
            Hit::DictCheck,
            false,
        );
    }
    y += card.h + 12.0;

    // Headword list.
    let card = Rect::new(x, y, wid, 112.0);
    w::card(p, &g.c, card);
    p.text(
        Face::Semibold,
        13.5,
        card.x + 14.0,
        card.y + 20.0,
        tl("Madde listesi (yerleşik)", "Headword list (built in)"),
        pal.text,
        wid,
    );
    let line = |p: &mut Painter, y: f32, k: &str, v: &str| {
        p.text(Face::Ui, 12.5, card.x + 14.0, y, k, pal.muted, card.w * 0.5);
        p.text_right(Face::Ui, 12.5, card.right() - 14.0, y, v, pal.text);
    };
    line(
        p,
        card.y + 46.0,
        tl("Sözcük", "Words"),
        &tf!(
            "{} ({} çok sözcüklü)",
            "{} ({} multiword)",
            fmt_n(n),
            fmt_n(s.ix.multiword as u64)
        ),
    );
    line(
        p,
        card.y + 68.0,
        tl("Alındığı gün", "Fetched"),
        &s.ix.fetched,
    );
    line(
        p,
        card.y + 90.0,
        tl("Saklanan canlı yanıtlar", "Live answers kept"),
        &fmt_n(s.cache_n),
    );
    y += card.h + 12.0;

    // Switches and buttons.
    let card = Rect::new(x, y, wid, 92.0);
    w::card(p, &g.c, card);
    let tr = Rect::new(card.right() - 52.0, card.y + 16.0, 38.0, 20.0);
    w::toggle(p, g.anim, &g.c, tr, s.prefs.live, "live");
    p.text(
        Face::Ui,
        13.0,
        card.x + 14.0,
        tr.cy(),
        tl(
            "Eksik madde için sozluk.gov.tr'ye sor",
            "Ask sozluk.gov.tr when an entry is missing",
        ),
        pal.text,
        card.w - 80.0,
    );
    g.hit(Rect::new(card.x, card.y + 6.0, card.w, 40.0), Hit::DbLive);
    let by = card.y + 52.0;
    let b1 = w::text_button_w(p, tl("Yeniden oku", "Read again"));
    g.btn(
        p,
        Rect::new(card.x + 14.0, by, b1, 28.0),
        tl("Yeniden oku", "Read again"),
        Hit::DbRefresh,
        false,
    );
    let b2 = w::text_button_w(p, tl("Dosyayı göster", "Show file"));
    g.btn(
        p,
        Rect::new(card.x + 22.0 + b1, by, b2, 28.0),
        tl("Dosyayı göster", "Show file"),
        Hit::DbFolder,
        false,
    );
    y += card.h + 12.0;

    if let Some(path) = &s.store.crawl_path {
        p.paragraph(
            Face::Ui,
            11.5,
            x + 2.0,
            y,
            wid,
            &path.display().to_string(),
            pal.muted,
        );
        y += 20.0;
    }
    let _ = Mode::ALL;
    y + 12.0 - page.y
}

/// The note beside a spelling fix, in the interface language.
fn why_text(why: &str) -> &'static str {
    match why {
        "written apart" => tl("ayrı yazılır", "written apart"),
        "TDK spelling of the stem" => tl("kökün TDK yazımı", "TDK spelling of the stem"),
        _ => tl("TDK yazımı", "TDK spelling"),
    }
}
