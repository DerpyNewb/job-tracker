//! Fonts and colours: the portfolio site's Source Sans 3 and pine-on-cool-white palette.

use std::sync::Arc;

use egui::{
    Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke,
    TextStyle, Theme, Visuals,
};
use serde::{Deserialize, Serialize};

pub const SEMIBOLD: &str = "semibold";

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub page: Color32,
    pub ink: Color32,
    pub muted: Color32,
    pub lines: Color32,
    pub pine: Color32,
    /// Text on a pine fill.
    pub on_pine: Color32,
    /// Text fields and the table.
    pub field: Color32,
    /// Buttons at rest, and the side columns.
    pub raised: Color32,
    pub hover: Color32,
    pub selected: Color32,
    pub error: Color32,
}

pub const LIGHT: Palette = Palette {
    page: Color32::from_rgb(0xf7, 0xf8, 0xf6),
    ink: Color32::from_rgb(0x18, 0x20, 0x1c),
    muted: Color32::from_rgb(0x56, 0x62, 0x5b),
    lines: Color32::from_rgb(0xd5, 0xdc, 0xd7),
    pine: Color32::from_rgb(0x1d, 0x61, 0x45),
    on_pine: Color32::from_rgb(0xff, 0xff, 0xff),
    field: Color32::from_rgb(0xff, 0xff, 0xff),
    raised: Color32::from_rgb(0xee, 0xf1, 0xee),
    hover: Color32::from_rgb(0xe3, 0xe9, 0xe5),
    selected: Color32::from_rgb(0xd8, 0xe8, 0xde),
    error: Color32::from_rgb(0xa8, 0x2a, 0x1f),
};

pub const DARK: Palette = Palette {
    page: Color32::from_rgb(0x10, 0x15, 0x12),
    ink: Color32::from_rgb(0xe4, 0xeb, 0xe7),
    muted: Color32::from_rgb(0x9f, 0xb0, 0xa6),
    lines: Color32::from_rgb(0x2a, 0x33, 0x2e),
    pine: Color32::from_rgb(0x6c, 0xc5, 0x96),
    on_pine: Color32::from_rgb(0x10, 0x15, 0x12),
    field: Color32::from_rgb(0x17, 0x1e, 0x1a),
    raised: Color32::from_rgb(0x15, 0x1b, 0x17),
    hover: Color32::from_rgb(0x22, 0x2b, 0x26),
    selected: Color32::from_rgb(0x1f, 0x3a, 0x2c),
    error: Color32::from_rgb(0xf0, 0x8a, 0x7e),
};

/// The accent colour: buttons, links, focus outlines and the selected row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Accent {
    #[default]
    Pine,
    Blue,
    Teal,
    Plum,
    Copper,
    Slate,
}

impl Accent {
    pub const ALL: [Accent; 6] = [
        Accent::Pine,
        Accent::Blue,
        Accent::Teal,
        Accent::Plum,
        Accent::Copper,
        Accent::Slate,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Accent::Pine => "Pine",
            Accent::Blue => "Blue",
            Accent::Teal => "Teal",
            Accent::Plum => "Plum",
            Accent::Copper => "Copper",
            Accent::Slate => "Slate",
        }
    }

    /// (accent, selected fill) for light, then for dark.
    fn colors(self) -> [(Color32, Color32); 2] {
        let c = Color32::from_rgb;
        match self {
            Accent::Pine => [(LIGHT.pine, LIGHT.selected), (DARK.pine, DARK.selected)],
            Accent::Blue => [(c(0x1f, 0x5f, 0xa8), c(0xd9, 0xe5, 0xf3)), (c(0x7f, 0xb3, 0xea), c(0x1d, 0x33, 0x50))],
            Accent::Teal => [(c(0x0f, 0x6b, 0x73), c(0xd3, 0xea, 0xeb)), (c(0x5c, 0xc7, 0xcf), c(0x17, 0x3a, 0x3d))],
            Accent::Plum => [(c(0x6b, 0x3f, 0xa0), c(0xe6, 0xdd, 0xf2)), (c(0xb9, 0x9a, 0xe8), c(0x33, 0x26, 0x4a))],
            Accent::Copper => [(c(0x8f, 0x4a, 0x0e), c(0xf3, 0xe1, 0xcf)), (c(0xe9, 0xa3, 0x5f), c(0x43, 0x30, 0x1c))],
            Accent::Slate => [(c(0x44, 0x56, 0x6a), c(0xdd, 0xe3, 0xe9)), (c(0xa9, 0xbc, 0xcd), c(0x29, 0x33, 0x3e))],
        }
    }
}

pub fn palette(theme: Theme, accent: Accent) -> Palette {
    let (base, i) = match theme {
        Theme::Light => (LIGHT, 0),
        Theme::Dark => (DARK, 1),
    };
    let (pine, selected) = accent.colors()[i];
    Palette {
        pine,
        selected,
        ..base
    }
}

/// Restyles both themes for `accent`.
pub fn set_accent(ctx: &Context, accent: Accent) {
    ctx.style_mut_of(Theme::Light, |s| {
        apply(s, &palette(Theme::Light, accent), Visuals::light())
    });
    ctx.style_mut_of(Theme::Dark, |s| {
        apply(s, &palette(Theme::Dark, accent), Visuals::dark())
    });
}

pub fn install(ctx: &Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "source-sans".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/SourceSans3-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "source-sans-semibold".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/SourceSans3-Semibold.ttf"
        ))),
    );
    // egui's own fonts stay after Source Sans as a fallback for symbols it lacks.
    let fallback = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "source-sans".into());
    let mut semibold = vec!["source-sans-semibold".to_string()];
    semibold.extend(fallback);
    fonts
        .families
        .insert(FontFamily::Name(SEMIBOLD.into()), semibold);
    ctx.set_fonts(fonts);

    set_accent(ctx, Accent::Pine);
}

fn apply(style: &mut egui::Style, p: &Palette, base: Visuals) {
    let semibold = FontFamily::Name(SEMIBOLD.into());
    style.text_styles = [
        (TextStyle::Heading, FontId::new(19.0, semibold.clone())),
        (TextStyle::Body, FontId::new(15.0, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(15.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Small,
            FontId::new(13.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(14.0, FontFamily::Monospace),
        ),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.interact_size.y = 28.0;
    // Scroll bars take their own space instead of floating over the Follow up cards.
    style.spacing.scroll = egui::style::ScrollStyle::solid();

    let mut v = base;
    v.panel_fill = p.page;
    v.window_fill = p.page;
    v.window_stroke = Stroke::new(1.0, p.lines);
    v.extreme_bg_color = p.field;
    v.faint_bg_color = p.raised;
    v.code_bg_color = p.raised;
    v.hyperlink_color = p.pine;
    v.error_fg_color = p.error;
    v.warn_fg_color = p.error;
    v.selection.bg_fill = p.selected;
    v.selection.stroke = Stroke::new(1.5, p.pine);
    v.text_edit_bg_color = Some(p.field);

    let radius = CornerRadius::same(4);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.page;
    w.noninteractive.weak_bg_fill = p.page;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.lines);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.ink);
    w.noninteractive.corner_radius = radius;
    for (state, fill) in [
        (&mut w.inactive, p.raised),
        (&mut w.hovered, p.hover),
        (&mut w.active, p.selected),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.fg_stroke = Stroke::new(1.0, p.ink);
        state.corner_radius = radius;
        state.expansion = 0.0;
    }
    w.inactive.bg_stroke = Stroke::new(1.0, p.lines);
    w.hovered.bg_stroke = Stroke::new(1.0, p.muted);
    w.active.bg_stroke = Stroke::new(1.0, p.pine);
    w.open = w.active;
    style.visuals = v;
}

/// Text in the semibold face.
pub fn strong(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).family(FontFamily::Name(SEMIBOLD.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contrast(a: Color32, b: Color32) -> f32 {
        let lum = |c: Color32| {
            let ch = |v: u8| {
                let s = v as f32 / 255.0;
                if s <= 0.03928 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
        };
        let (x, y) = (lum(a) + 0.05, lum(b) + 0.05);
        x.max(y) / x.min(y)
    }

    #[test]
    fn every_accent_keeps_text_readable() {
        for accent in Accent::ALL {
            for theme in [Theme::Light, Theme::Dark] {
                let p = palette(theme, accent);
                let at = format!("{accent:?} {theme:?}");
                assert!(contrast(p.on_pine, p.pine) >= 4.5, "button text, {at}");
                assert!(contrast(p.pine, p.page) >= 4.5, "accent text on the page, {at}");
                assert!(contrast(p.pine, p.field) >= 4.5, "accent text in the table, {at}");
                assert!(contrast(p.ink, p.selected) >= 4.5, "selected row text, {at}");
            }
        }
    }
}
