//! Small drawings the screens share: buttons whose hover fades in, the sliding segmented
//! switch, chips that wrap, cards and the InfoBar. Each takes the frame's colours and the
//! animator; hits are recorded by the caller.

use std::hash::Hash;

use mdshared_ui::anim::{Animator, Motion, Spring, fluent};
use mdshared_ui::gfx::{Face, Painter, Rect, Rgb, mix};

use crate::theme::Colors;

pub const RADIUS: f32 = 5.0;
pub const CARD_RADIUS: f32 = 8.0;

/// 0 → 1 as the pointer rests on something, for its fill.
pub fn hover_t(anim: &mut Animator, id: impl Hash, hovered: bool) -> f32 {
    anim.value(
        ("hover", id),
        if hovered { 1.0_f32 } else { 0.0 },
        fluent::FADE,
    )
}

/// A press that sinks the fill a little further.
pub fn press_t(anim: &mut Animator, id: impl Hash, pressed: bool) -> f32 {
    anim.value(
        ("press", id),
        if pressed { 1.0_f32 } else { 0.0 },
        fluent::FADE,
    )
}

/// A button with only an icon: transparent at rest, the hover fill under the pointer.
#[allow(clippy::too_many_arguments)]
pub fn icon_button(
    p: &mut Painter,
    anim: &mut Animator,
    c: &Colors,
    r: Rect,
    glyph: &str,
    id: impl Hash + Clone,
    hovered: bool,
    pressed: bool,
    on: bool,
) {
    let h = hover_t(anim, id.clone(), hovered);
    let pr = press_t(anim, id.clone(), pressed);
    let on_t = anim.value(("on", id), if on { 1.0_f32 } else { 0.0 }, fluent::MOVE);
    if on_t > 0.0 {
        p.rbox_a(r, RADIUS, c.pal.on, on_t);
    }
    if h > 0.0 {
        let fill = mix(c.pal.hover, c.pal.press, pr);
        p.rbox_a(r, RADIUS, fill, h * if on { 0.6 } else { 1.0 });
    }
    let ink = mix(c.pal.text, c.pal.accent, on_t);
    p.icon(glyph, 16.0, r.cx(), r.cy(), ink);
}

/// A text button: `primary` fills it with the accent.
#[allow(clippy::too_many_arguments)]
pub fn text_button(
    p: &mut Painter,
    anim: &mut Animator,
    c: &Colors,
    r: Rect,
    label: &str,
    id: impl Hash + Clone,
    hovered: bool,
    pressed: bool,
    primary: bool,
) {
    let h = hover_t(anim, id.clone(), hovered);
    let pr = press_t(anim, id, pressed);
    if primary {
        let base = c.pal.accent;
        let lit = mix(base, c.pal.page, 0.12 * h + 0.2 * pr);
        p.rbox(r, RADIUS, lit);
        p.text_center(Face::Semibold, 12.5, r, label, c.pal.on_accent);
    } else {
        let fill = mix(mix(c.chip, c.pal.hover, h), c.pal.press, pr);
        p.frame(r, RADIUS, fill, c.pal.popup_line);
        p.text_center(Face::Ui, 12.5, r, label, c.pal.text);
    }
}

pub fn text_button_w(p: &mut Painter, label: &str) -> f32 {
    p.measure(Face::Ui, 12.5, label).ceil() + 24.0
}

/// The spring the sliding parts ride: quick, no bounce.
pub fn slide() -> Motion {
    Motion::spring(Spring::response(0.22, 1.0).with_threshold(0.2))
}

/// A segmented switch of icons with a thumb that slides to the chosen one. Returns each
/// segment's rectangle for the caller's hits.
#[allow(clippy::too_many_arguments)]
pub fn segmented_icons(
    p: &mut Painter,
    anim: &mut Animator,
    c: &Colors,
    r: Rect,
    glyphs: &[&str],
    chosen: usize,
    hovered: Option<usize>,
    id: &str,
) -> Vec<Rect> {
    p.rbox(r, RADIUS + 1.0, mix(c.pal.title, c.pal.press, 0.55));
    let n = glyphs.len() as f32;
    let seg_w = (r.w - 4.0) / n;
    let segs: Vec<Rect> = (0..glyphs.len())
        .map(|i| Rect::new(r.x + 2.0 + i as f32 * seg_w, r.y + 2.0, seg_w, r.h - 4.0))
        .collect();
    let target = segs[chosen.min(segs.len() - 1)];
    let thumb: Rect = anim.value((id, "thumb"), target, slide());
    p.shadow_with(thumb, RADIUS, c.pal.dark, 0.35);
    p.rbox(thumb, RADIUS, c.thumb_fill);
    for (i, (g, s)) in glyphs.iter().zip(&segs).enumerate() {
        let h = hover_t(anim, (id, i), hovered == Some(i) && i != chosen);
        if h > 0.0 {
            p.rbox_a(s.inset(1.0), RADIUS, c.pal.hover, h * 0.7);
        }
        // The ink follows the thumb: accent where it sits, muted elsewhere.
        let near = 1.0 - ((thumb.cx() - s.cx()).abs() / seg_w).min(1.0);
        p.icon(
            g,
            14.0,
            s.cx(),
            s.cy(),
            mix(c.pal.muted, c.pal.accent, near),
        );
    }
    segs
}

/// A rounded chip; returns its width.
pub fn chip(
    p: &mut Painter,
    anim: &mut Animator,
    c: &Colors,
    x: f32,
    y: f32,
    label: &str,
    id: impl Hash,
    hovered: bool,
) -> Rect {
    let w = p.measure(Face::Ui, 12.5, label).ceil() + 20.0;
    let r = Rect::new(x, y, w, 24.0);
    let h = hover_t(anim, id, hovered);
    p.frame(r, 12.0, mix(c.chip, c.pal.hover, h), c.pal.line);
    p.text(
        Face::Ui,
        12.5,
        r.x + 10.0,
        r.cy(),
        label,
        mix(c.pal.text, c.pal.accent, h),
        f32::INFINITY,
    );
    r
}

/// Chips laid left to right, wrapping at `w`. Returns each chip's rectangle and the height used.
pub fn chip_rows(
    p: &mut Painter,
    anim: &mut Animator,
    c: &Colors,
    x: f32,
    y: f32,
    w: f32,
    labels: &[String],
    hovered: Option<usize>,
    id: &str,
) -> (Vec<Rect>, f32) {
    let mut rects = Vec::new();
    let (mut cx, mut cy) = (x, y);
    for (i, l) in labels.iter().enumerate() {
        let cw = p.measure(Face::Ui, 12.5, l).ceil() + 20.0;
        if cx > x && cx + cw > x + w {
            cx = x;
            cy += 30.0;
        }
        let r = chip(p, anim, c, cx, cy, l, (id, i), hovered == Some(i));
        rects.push(r);
        cx += r.w + 6.0;
    }
    let h = if labels.is_empty() {
        0.0
    } else {
        cy - y + 24.0
    };
    (rects, h)
}

pub fn card(p: &mut Painter, c: &Colors, r: Rect) {
    p.frame(r, CARD_RADIUS, c.card, c.pal.line);
}

pub fn label(p: &mut Painter, c: &Colors, x: f32, y: f32, text: &str) {
    p.text(
        Face::Semibold,
        12.0,
        x,
        y + 8.0,
        text,
        c.pal.muted,
        f32::INFINITY,
    );
}

/// An InfoBar: tinted card, glyph, wrapped text. Returns the height.
pub fn infobar(
    p: &mut Painter,
    c: &Colors,
    x: f32,
    y: f32,
    w: f32,
    glyph: &str,
    ink: Rgb,
    bg: Rgb,
    line: Rgb,
    text: &str,
) -> f32 {
    let tw = w - 46.0;
    let th = p.paragraph_height(Face::Ui, 12.5, tw, text);
    let h = th.max(18.0) + 18.0;
    p.frame(Rect::new(x, y, w, h), RADIUS, bg, line);
    p.icon(glyph, 15.0, x + 18.0, y + 18.0, ink);
    p.paragraph(Face::Ui, 12.5, x + 34.0, y + 9.0, tw, text, c.pal.text);
    h
}

/// A toggle switch, its knob gliding across.
pub fn toggle(p: &mut Painter, anim: &mut Animator, c: &Colors, r: Rect, on: bool, id: impl Hash) {
    let t = anim.value(("toggle", id), if on { 1.0_f32 } else { 0.0 }, slide());
    let track = mix(c.card, c.pal.accent, t);
    p.rbox(r, r.h / 2.0, track);
    if t < 0.99 {
        let edge = mix(c.pal.faint, c.pal.accent, t);
        p.frame(r, r.h / 2.0, track, edge);
    }
    let d = r.h - 8.0 + 2.0 * t;
    let x = r.x + 4.0 + (r.w - 8.0 - d) * t;
    let knob = Rect::new(x, r.cy() - d / 2.0, d, d);
    p.rbox(knob, d / 2.0, mix(c.pal.muted, c.pal.on_accent, t));
}

/// A segmented switch of words, the thumb sliding to the chosen one. Returns the segments.
#[allow(clippy::too_many_arguments)]
pub fn segmented_text(
    p: &mut Painter,
    anim: &mut Animator,
    c: &Colors,
    r: Rect,
    labels: &[&str],
    chosen: usize,
    hovered: Option<usize>,
    id: &str,
) -> Vec<Rect> {
    p.rbox(r, RADIUS + 1.0, mix(c.card, c.pal.press, 0.45));
    let n = labels.len() as f32;
    let seg_w = (r.w - 4.0) / n;
    let segs: Vec<Rect> = (0..labels.len())
        .map(|i| Rect::new(r.x + 2.0 + i as f32 * seg_w, r.y + 2.0, seg_w, r.h - 4.0))
        .collect();
    let target = segs[chosen.min(segs.len() - 1)];
    let thumb: Rect = anim.value((id, "thumb"), target, slide());
    p.shadow_with(thumb, RADIUS, c.pal.dark, 0.3);
    p.rbox(thumb, RADIUS, c.thumb_fill);
    for (i, (l, s)) in labels.iter().zip(&segs).enumerate() {
        let h = hover_t(anim, (id, i), hovered == Some(i) && i != chosen);
        if h > 0.0 {
            p.rbox_a(s.inset(1.0), RADIUS, c.pal.hover, h * 0.7);
        }
        let near = 1.0 - ((thumb.cx() - s.cx()).abs() / seg_w).min(1.0);
        let face = if i == chosen {
            Face::Semibold
        } else {
            Face::Ui
        };
        p.text_center(face, 12.5, *s, l, mix(c.pal.muted, c.pal.text, near));
    }
    segs
}
