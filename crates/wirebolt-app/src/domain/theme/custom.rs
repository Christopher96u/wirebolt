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

#[must_use]
pub fn derive(background: RgbColor, accent: RgbColor) -> ThemePreview {
    let dark = background.luminance() < 0.179;
    let mut tokens = theme(if dark {
        DomainThemePreset::Black
    } else {
        DomainThemePreset::Paper
    });
    let step = if dark {
        RgbColor::hex(0x00ff_ffff)
    } else {
        RgbColor::hex(0)
    };
    tokens.window = background;
    tokens.sidebar = mix(background, step, 0.025);
    tokens.card = mix(background, step, 0.055);
    tokens.selected = mix(background, accent, 0.15);
    tokens.tab_active = mix(background, step, 0.04);
    tokens.overlay = mix(background, step, 0.03);
    let text = if dark {
        RgbColor::hex(0x00f5_f5f5)
    } else {
        RgbColor::hex(0x0018_1818)
    };
    tokens.primary = text;
    tokens.secondary = mix(background, text, 0.85);
    tokens.muted = mix(background, text, 0.65);
    tokens.dim = mix(background, text, 0.5);
    tokens.line_number = tokens.dim;
    tokens.send_background = neutral_action(dark);
    tokens.send_foreground = best_text(tokens.send_background);
    tokens.variable_background = mix(background, accent, 0.1);
    tokens.variable_foreground = accent;
    // Small hue rotation harmonizes with the accent without reversing semantic meanings.
    let rotation = (hue(accent) % 30.0 - 15.0) * 0.5;
    tokens.methods = super::MethodTokens::from_fn(|kind| {
        let hue = match kind {
            MethodChipKind::Get => 140.0,
            MethodChipKind::Post => 210.0,
            MethodChipKind::Put => 24.0,
            MethodChipKind::Patch => 280.0,
            MethodChipKind::Delete => 0.0,
            MethodChipKind::Head => 175.0,
            MethodChipKind::Options => 330.0,
            MethodChipKind::Query => 65.0,
            MethodChipKind::Custom => 95.0,
            MethodChipKind::WebSocket => 245.0,
        };
        let foreground = if kind == MethodChipKind::Custom {
            RgbColor::hex(if dark { 0x00bb_bbbb } else { 0x0044_4444 })
        } else {
            hsl(hue + rotation, 0.72, if dark { 0.68 } else { 0.32 })
        };
        DomainMethodChip {
            background: mix(background, foreground, 0.08),
            foreground,
        }
    });
    tokens.success = tokens.methods.get.foreground;
    tokens.warning = tokens.methods.query.foreground;
    tokens.error = tokens.methods.delete.foreground;
    tokens.syntax_key = accent;
    tokens.syntax_string = tokens.methods.get.foreground;
    tokens.syntax_number = tokens.methods.put.foreground;
    tokens.syntax_keyword = tokens.methods.patch.foreground;
    tokens.syntax_punctuation = tokens.secondary;
    let (tokens, notices) = checked(tokens, false);
    ThemePreview {
        name: "My theme".into(),
        tokens,
        notices,
    }
}

/// Surface bounds leave room for AAA text, distinct semantic hues and neutral native
/// controls with white glyphs. No single foreground can meet AAA on mid-gray surfaces.
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

#[must_use]
pub fn checked(mut tokens: ThemeTokens, increased: bool) -> (ThemeTokens, Vec<String>) {
    let dark = tokens.preset != DomainThemePreset::Paper;
    let mut notices = Vec::new();
    let action = neutral_action(dark);
    if tokens.send_background != action {
        tokens.send_background = action;
        notices.push("Action color made neutral to match other controls.".into());
    }
    tokens.send_foreground = best_text(action);
    for (name, value) in [
        ("Window background", &mut tokens.window),
        ("Sidebar background", &mut tokens.sidebar),
        ("Group background", &mut tokens.card),
        ("Selection color", &mut tokens.selected),
        ("Tab background", &mut tokens.tab_active),
        ("Panel background", &mut tokens.overlay),
        ("Variable background", &mut tokens.variable_background),
    ] {
        adjusted(name, surface(*value, dark), value, &mut notices);
    }
    for chip in tokens.methods.values_mut() {
        chip.background = surface(chip.background, dark);
    }
    // Borders are neutral and based on repaired surfaces, never on the accent.
    let neutral = RgbColor::hex(if dark { 0x00ff_ffff } else { 0 });
    tokens.hairline = mix(neutral_gray(tokens.window), neutral, 0.18);
    tokens.card_border = mix(neutral_gray(tokens.card), neutral, 0.24);
    let surfaces = text_surfaces(&tokens);
    let ratio = if increased { 7.0 } else { 4.5 };
    adjusted(
        "Primary text color",
        strengthen(tokens.primary, &surfaces, 7.0),
        &mut tokens.primary,
        &mut notices,
    );
    for (name, value) in [
        ("Secondary text color", &mut tokens.secondary),
        ("Muted text color", &mut tokens.muted),
        ("Comment color", &mut tokens.dim),
        ("Line number color", &mut tokens.line_number),
        ("Property color", &mut tokens.syntax_key),
        ("String color", &mut tokens.syntax_string),
        ("Number color", &mut tokens.syntax_number),
        ("Keyword color", &mut tokens.syntax_keyword),
        ("Punctuation color", &mut tokens.syntax_punctuation),
        ("Success color", &mut tokens.success),
        ("Warning color", &mut tokens.warning),
        ("Error color", &mut tokens.error),
    ] {
        adjusted(
            name,
            strengthen(*value, &surfaces, ratio),
            value,
            &mut notices,
        );
    }
    adjusted(
        "Variable text color",
        strengthen(tokens.variable_foreground, &surfaces, ratio),
        &mut tokens.variable_foreground,
        &mut notices,
    );
    // A filled accent action also needs room for AAA when Increase Contrast is on.
    if increased && best_text(tokens.send_background).contrast(tokens.send_background) < 7.0 {
        adjusted(
            "Action background",
            surface(tokens.send_background, dark),
            &mut tokens.send_background,
            &mut notices,
        );
    }
    if tokens.send_foreground.contrast(tokens.send_background) < ratio {
        adjusted(
            "Action text color",
            best_text(tokens.send_background),
            &mut tokens.send_foreground,
            &mut notices,
        );
    }
    for chip in tokens.methods.values_mut() {
        let mut surfaces = surfaces.to_vec();
        surfaces.push(chip.background);
        chip.foreground = strengthen(chip.foreground, &surfaces, ratio);
    }
    tokens.control = RgbColor::hex(0x0076_7676);
    (tokens, notices)
}

fn adjusted(name: &str, candidate: RgbColor, value: &mut RgbColor, notices: &mut Vec<String>) {
    if *value != candidate {
        let direction = if candidate.luminance() < value.luminance() {
            "darkened"
        } else {
            "lightened"
        };
        *value = candidate;
        notices.push(format!("{name} {direction} so text stays readable."));
    }
}

#[must_use]
pub fn resolve(tokens: ThemeTokens, increased: bool) -> ResolvedTokens {
    let (tokens, _) = checked(tokens, increased);
    let surfaces = text_surfaces(&tokens);
    let chips = MethodChipKind::ALL
        .into_iter()
        .map(|kind| (kind, tokens.methods.get(kind).clone()))
        .collect();
    let borders = super::resolve_borders(&tokens, &surfaces, increased);
    ResolvedTokens {
        is_dark: tokens.preset != DomainThemePreset::Paper,
        statuses: super::resolved_statuses(&tokens, &chips, increased, false),
        primary: tokens.primary,
        muted: tokens.muted,
        hairline: borders.hairline,
        card_border: borders.card,
        focus_border: borders.focus,
        definition: tokens,
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

fn neutral_action(dark: bool) -> RgbColor {
    RgbColor::hex(if dark { 0x00d4_d4d4 } else { 0x003f_3f3f })
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
fn hue(color: RgbColor) -> f64 {
    let [r, g, b] = [color.red, color.green, color.blue].map(f64::from);
    let max = color.red.max(color.green).max(color.blue);
    let delta = f64::from(max - color.red.min(color.green).min(color.blue));
    if delta == 0.0 {
        return 0.0;
    }
    let sector = if max == color.red {
        (g - b) / delta
    } else if max == color.green {
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

fn neutral_gray(surface: RgbColor) -> RgbColor {
    // Weighted encoded channels keep the border visually near its surface.
    let gray = u8::try_from(
        (u32::from(surface.red) * 2126
            + u32::from(surface.green) * 7152
            + u32::from(surface.blue) * 722)
            / 10000,
    )
    .expect("weighted byte average");
    RgbColor {
        red: gray,
        green: gray,
        blue: gray,
    }
}
