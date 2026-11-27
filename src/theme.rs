//! The look: the three-way theme (System, Light, Dark) and the colours, which glide to a new
//! theme on the animator instead of switching in one frame.

use std::time::Duration;

use mdshared_ui::anim::{Animator, Motion, fluent};
use mdshared_ui::gfx::{Rgb, rgb};
use mdshared_ui::theme::{DARK, LIGHT, Palette};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    System,
    Light,
    Dark,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::System, Mode::Light, Mode::Dark];

    pub fn label(self) -> &'static str {
        match self {
            Mode::System => "Follow the system",
            Mode::Light => "Light",
            Mode::Dark => "Dark",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Mode::System => "system",
            Mode::Light => "light",
            Mode::Dark => "dark",
        }
    }

    pub fn from_key(s: &str) -> Mode {
        match s {
            "light" => Mode::Light,
            "dark" => Mode::Dark,
            _ => Mode::System,
        }
    }

    pub fn next(self) -> Mode {
        match self {
            Mode::System => Mode::Light,
            Mode::Light => Mode::Dark,
            Mode::Dark => Mode::System,
        }
    }

    pub fn index(self) -> usize {
        Mode::ALL.iter().position(|m| *m == self).unwrap_or(0)
    }

    pub fn dark(self, system_dark: bool) -> bool {
        match self {
            Mode::System => system_dark,
            Mode::Light => false,
            Mode::Dark => true,
        }
    }
}

/// TDK's colours, from sozluk.gov.tr's own stylesheet (read 2026-11-27): the red `#c62828`
/// (`--primary-color`), paper `#f7f5ec` and slate text `#2c3e50` in the light theme; navy
/// `#1a1a2e`, cards `#24243e`, borders `#3a3a5a` and gold `#daa520` in the dark one. Where the
/// site's own shade is under 4.5:1 for small text the colour is moved within its family: the dark
/// red is `#e46f64` (the site's `#c64b3f` is 3.6:1 on navy), muted text is the site's `#546e7a`.
/// The toolkit colours not named here (folder, find marks, danger) stay its own.
pub const TDK_LIGHT: Palette = Palette {
    dark: false,
    page: rgb(0xf7f5ec),
    title: rgb(0xefebdf),
    bar: rgb(0xf3f0e4),
    pane: rgb(0xf1eee1),
    text: rgb(0x2c3e50),
    muted: rgb(0x546e7a),
    faint: rgb(0x8a959c),
    hover: rgb(0xe8e3d3),
    press: rgb(0xddd6c2),
    on: rgb(0xf6dedb),
    line: rgb(0xe2ddcd),
    accent: rgb(0xc62828),
    on_accent: rgb(0xffffff),
    field: rgb(0xffffff),
    popup: rgb(0xfffdf8),
    popup_line: rgb(0xd9d2bf),
    selection: rgb(0xe57373),
    warn: rgb(0x9d5d00),
    warn_bg: rgb(0xfff4ce),
    ..LIGHT
};

pub const TDK_DARK: Palette = Palette {
    dark: true,
    page: rgb(0x1a1a2e),
    title: rgb(0x151527),
    bar: rgb(0x1d1d33),
    pane: rgb(0x1b1b30),
    text: rgb(0xe1e1e1),
    muted: rgb(0xa5a8c0),
    faint: rgb(0x7a7d99),
    hover: rgb(0x2b2b48),
    press: rgb(0x35355a),
    on: rgb(0x3a2a3c),
    line: rgb(0x3a3a5a),
    accent: rgb(0xe46f64),
    on_accent: rgb(0x1a1a2e),
    field: rgb(0x16162a),
    popup: rgb(0x24243e),
    popup_line: rgb(0x3a3a5a),
    selection: rgb(0x8e3b36),
    warn: rgb(0xfcb54a),
    warn_bg: rgb(0x3a3020),
    ..DARK
};

/// The palette and the few colours this app adds (InfoBar and status colours, the word-type
/// tag: the site's sienna `#a0522d`, gold `#daa520` in the dark).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colors {
    pub pal: Palette,
    pub success: Rgb,
    pub info_bg: Rgb,
    pub info_line: Rgb,
    pub warn_card: Rgb,
    pub warn_line: Rgb,
    pub tag: Rgb,
    pub chip: Rgb,
    pub card: Rgb,
    pub thumb_fill: Rgb,
}

impl Colors {
    pub fn of(dark: bool) -> Colors {
        if dark {
            Colors {
                pal: TDK_DARK,
                success: rgb(0x6ccb5f),
                info_bg: rgb(0x2c3a47),
                info_line: rgb(0x3d4f5f),
                warn_card: rgb(0x332a1c),
                warn_line: rgb(0x5a4524),
                tag: rgb(0xdaa520),
                chip: rgb(0x24243e),
                card: rgb(0x24243e),
                thumb_fill: rgb(0x34345a),
            }
        } else {
            Colors {
                pal: TDK_LIGHT,
                success: rgb(0x2e7d32),
                info_bg: rgb(0xf3f6f7),
                info_line: rgb(0xdde3e6),
                warn_card: rgb(0xfff7e6),
                warn_line: rgb(0xf1dfb8),
                tag: rgb(0xa0522d),
                chip: rgb(0xffffff),
                card: rgb(0xffffff),
                thumb_fill: rgb(0xffffff),
            }
        }
    }
}

/// How long a theme change takes: long enough to see the colours travel, short enough that it
/// never feels like waiting.
pub const THEME_MOTION: Motion = Motion::tween(Duration::from_millis(360), fluent::POINT_TO_POINT);

/// The colours of this frame: every one moving toward `target` by [`THEME_MOTION`]. The first
/// frame is at the target (the animator's rule for a name first seen), so the app opens in its
/// theme without a fade. Under reduced motion the host's policy makes it a cut.
pub fn animated(anim: &mut Animator, target: &Colors) -> Colors {
    let m = THEME_MOTION;
    let mut c = *target;
    macro_rules! glide {
        ($($f:ident).+) => {
            c.$($f).+ = anim.value(("theme", stringify!($($f).+)), target.$($f).+, m);
        };
    }
    glide!(pal.page);
    glide!(pal.title);
    glide!(pal.bar);
    glide!(pal.pane);
    glide!(pal.text);
    glide!(pal.muted);
    glide!(pal.faint);
    glide!(pal.hover);
    glide!(pal.press);
    glide!(pal.on);
    glide!(pal.line);
    glide!(pal.accent);
    glide!(pal.on_accent);
    glide!(pal.field);
    glide!(pal.popup);
    glide!(pal.popup_line);
    glide!(pal.selection);
    glide!(pal.warn);
    glide!(pal.warn_bg);
    glide!(success);
    glide!(info_bg);
    glide!(info_line);
    glide!(warn_card);
    glide!(warn_line);
    glide!(tag);
    glide!(chip);
    glide!(card);
    glide!(thumb_fill);
    c
}
