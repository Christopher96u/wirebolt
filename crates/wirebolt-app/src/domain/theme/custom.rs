//! Complete custom palettes, using the same WCAG math as authored presets.
use super::{
    DomainMethodChip, DomainThemePreset, MethodChipKind, ResolvedTokens, RgbColor, ThemeTokens,
    strengthen, theme,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemePreview {
    pub name: String,
    pub tokens: ThemeTokens,
    pub notices: Vec<String>,
}

/// A custom palette has one pair for every typed method, unlike the five authored pairs.
pub const METHOD_COUNT: usize = MethodChipKind::ALL.len();

#[must_use]
pub fn derive(background: RgbColor, accent: RgbColor) -> ThemePreview {
    let dark = background.luminance() < 0.179;
    let mut d = theme(if dark {
        DomainThemePreset::Black
    } else {
        DomainThemePreset::Paper
    });
    let step = if dark {
        RgbColor::hex(0x00ff_ffff)
    } else {
        RgbColor::hex(0)
    };
    d.window = background;
    d.sidebar = mix(background, step, 0.025);
    d.card = mix(background, step, 0.055);
    d.selected = mix(background, accent, 0.15);
    d.tab_active = mix(background, step, 0.04);
    d.overlay = mix(background, step, 0.03);
    d.hairline = mix(background, step, 0.18);
    d.card_border = mix(background, step, 0.24);
    let text = if dark {
        RgbColor::hex(0x00f5_f5f5)
    } else {
        RgbColor::hex(0x0018_1818)
    };
    d.primary = text;
    d.secondary = mix(background, text, 0.85);
    d.muted = mix(background, text, 0.65);
    d.dim = mix(background, text, 0.5);
    d.line_number = d.dim;
    d.send_background = accent;
    d.send_foreground = text;
    d.variable_background = mix(background, accent, 0.1);
    d.variable_foreground = accent;
    // Small hue rotation harmonizes with the accent without reversing semantic meanings.
    let rotation = (hue(accent) % 30.0 - 15.0) * 0.5;
    let hues = [
        140.0, 210.0, 24.0, 280.0, 0.0, 175.0, 330.0, 65.0, 95.0, 245.0,
    ];
    d.method_foregrounds = hues
        .map(|h| hsl(h + rotation, 0.72, if dark { 0.68 } else { 0.32 }))
        .to_vec();
    d.method_foregrounds[8] = RgbColor::hex(if dark { 0x00bb_bbbb } else { 0x0044_4444 });
    d.method_backgrounds = d
        .method_foregrounds
        .iter()
        .map(|c| mix(background, *c, 0.08))
        .collect();
    d.success = d.method_foregrounds[0];
    d.warning = d.method_foregrounds[7];
    d.error = d.method_foregrounds[4];
    d.syntax_key = accent;
    d.syntax_string = d.method_foregrounds[0];
    d.syntax_number = d.method_foregrounds[2];
    d.syntax_keyword = d.method_foregrounds[3];
    d.syntax_punctuation = d.secondary;
    let (tokens, notices) = checked(d, false);
    ThemePreview {
        name: "My theme".into(),
        tokens,
        notices,
    }
}

/// Surface bounds leave room for AAA text, distinct semantic hues and neutral native
/// controls with white glyphs. No single foreground can meet AAA on mid-grey surfaces.
fn surface(color: RgbColor, dark: bool) -> RgbColor {
    let target = RgbColor::hex(if dark { 0 } else { 0x00ff_ffff });
    for step in 0..=255 {
        let candidate = mix(color, target, f64::from(step) / 255.0);
        if if dark {
            candidate.luminance() <= 0.025
        } else {
            candidate.luminance() >= 0.65
        } {
            return candidate;
        }
    }
    target
}

/// # Errors
/// Returns an error for an incomplete custom method palette.
pub fn validate(tokens: &ThemeTokens) -> Result<(), String> {
    if tokens.method_backgrounds.len() != METHOD_COUNT
        || tokens.method_foregrounds.len() != METHOD_COUNT
    {
        return Err("The custom theme has an incomplete method palette.".into());
    }
    Ok(())
}

#[must_use]
pub fn checked(mut d: ThemeTokens, increased: bool) -> (ThemeTokens, Vec<String>) {
    let dark = d.preset != DomainThemePreset::Paper;
    let mut notices = Vec::new();
    for (name, value) in [
        ("surface/window", &mut d.window),
        ("surface/sidebar", &mut d.sidebar),
        ("surface/card", &mut d.card),
        ("surface/selected", &mut d.selected),
        ("surface/tab", &mut d.tab_active),
        ("surface/overlay", &mut d.overlay),
        ("variable/background", &mut d.variable_background),
    ] {
        adjusted(name, surface(*value, dark), value, &mut notices);
    }
    for value in &mut d.method_backgrounds {
        *value = surface(*value, dark);
    }
    let s = text_surfaces(&d);
    let ratio = if increased { 7.0 } else { 4.5 };
    adjusted(
        "text/primary",
        strengthen(d.primary, &s, 7.0),
        &mut d.primary,
        &mut notices,
    );
    for (name, value) in [
        ("text/secondary", &mut d.secondary),
        ("text/muted", &mut d.muted),
        ("syntax/comment", &mut d.dim),
        ("text/line-number", &mut d.line_number),
        ("syntax/key", &mut d.syntax_key),
        ("syntax/string", &mut d.syntax_string),
        ("syntax/number", &mut d.syntax_number),
        ("syntax/keyword", &mut d.syntax_keyword),
        ("syntax/punctuation", &mut d.syntax_punctuation),
        ("status/success", &mut d.success),
        ("status/warning", &mut d.warning),
        ("status/error", &mut d.error),
    ] {
        adjusted(name, strengthen(*value, &s, ratio), value, &mut notices);
    }
    adjusted(
        "variable/foreground",
        strengthen(d.variable_foreground, &s, ratio),
        &mut d.variable_foreground,
        &mut notices,
    );
    // A filled accent action also needs room for AAA when Increase Contrast is on.
    if increased && best_text(d.send_background).contrast(d.send_background) < 7.0 {
        adjusted(
            "action/background",
            surface(d.send_background, dark),
            &mut d.send_background,
            &mut notices,
        );
    }
    if d.send_foreground.contrast(d.send_background) < ratio {
        adjusted(
            "action/foreground",
            best_text(d.send_background),
            &mut d.send_foreground,
            &mut notices,
        );
    }
    for (foreground, background) in d.method_foregrounds.iter_mut().zip(&d.method_backgrounds) {
        let mut surfaces = s.to_vec();
        surfaces.push(*background);
        *foreground = strengthen(*foreground, &surfaces, ratio);
    }
    d.control = RgbColor::hex(0x0076_7676);
    (d, notices)
}

fn adjusted(name: &str, candidate: RgbColor, value: &mut RgbColor, notices: &mut Vec<String>) {
    if *value != candidate {
        *value = candidate;
        notices.push(format!("Adjusted {name} for contrast."));
    }
}

#[must_use]
pub fn resolve(tokens: ThemeTokens, increased: bool) -> ResolvedTokens {
    let (d, _) = checked(tokens, increased);
    let surfaces = text_surfaces(&d);
    let chips = MethodChipKind::ALL
        .into_iter()
        .enumerate()
        .map(|(i, kind)| {
            (
                kind,
                DomainMethodChip {
                    background: d.method_backgrounds.get(i).copied().unwrap_or(d.tab_active),
                    foreground: d.method_foregrounds.get(i).copied().unwrap_or(d.primary),
                },
            )
        })
        .collect();
    ResolvedTokens {
        primary: d.primary,
        muted: d.muted,
        hairline: if increased {
            strengthen(d.hairline, &surfaces, 3.0)
        } else {
            d.hairline
        },
        card_border: if increased {
            strengthen(d.card_border, &surfaces, 3.0)
        } else {
            d.card_border
        },
        focus_border: strengthen(d.card_border, &surfaces, if increased { 4.5 } else { 3.0 }),
        definition: d,
        chips,
    }
}

// Error text also appears inside unresolved variable chips, so those backgrounds
// participate in the same contract as editor, sidebar and selection surfaces.
fn text_surfaces(tokens: &ThemeTokens) -> [RgbColor; 7] {
    [
        tokens.window,
        tokens.sidebar,
        tokens.card,
        tokens.selected,
        tokens.tab_active,
        tokens.overlay,
        tokens.variable_background,
    ]
}

fn best_text(background: RgbColor) -> RgbColor {
    let black = RgbColor::hex(0);
    let white = RgbColor::hex(0x00ff_ffff);
    if black.contrast(background) > white.contrast(background) {
        black
    } else {
        white
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Clamped and rounded sRGB channels are bytes."
)]
#[must_use]
pub fn mix(a: RgbColor, b: RgbColor, amount: f64) -> RgbColor {
    let channel = |a, b| {
        (f64::from(a) * (1.0 - amount) + f64::from(b) * amount)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    RgbColor {
        red: channel(a.red, b.red),
        green: channel(a.green, b.green),
        blue: channel(a.blue, b.blue),
    }
}
fn hue(c: RgbColor) -> f64 {
    let [r, g, b] = [c.red, c.green, c.blue].map(f64::from);
    let max = c.red.max(c.green).max(c.blue);
    let delta = f64::from(max - c.red.min(c.green).min(c.blue));
    if delta == 0.0 {
        return 0.0;
    }
    let sector = if max == c.red {
        (g - b) / delta
    } else if max == c.green {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    };
    (sector * 60.0).rem_euclid(360.0)
}
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Normalized HSL conversion produces byte channels."
)]
fn hsl(hue: f64, saturation: f64, lightness: f64) -> RgbColor {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let sector = hue.rem_euclid(360.0) / 60.0;
    let intermediate = chroma * (1.0 - (sector % 2.0 - 1.0).abs());
    let (red, green, blue) = match sector as u8 {
        0 => (chroma, intermediate, 0.0),
        1 => (intermediate, chroma, 0.0),
        2 => (0.0, chroma, intermediate),
        3 => (0.0, intermediate, chroma),
        4 => (intermediate, 0.0, chroma),
        _ => (chroma, 0.0, intermediate),
    };
    let channel = |v: f64| ((v + lightness - chroma / 2.0) * 255.0).round() as u8;
    RgbColor {
        red: channel(red),
        green: channel(green),
        blue: channel(blue),
    }
}
