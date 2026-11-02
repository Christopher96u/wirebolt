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
    tokens.send_background = accent;
    tokens.send_foreground = text;
    tokens.variable_background = mix(background, accent, 0.1);
    tokens.variable_foreground = accent;
    // Small hue rotation harmonizes with the accent without reversing semantic meanings.
    let rotation = (hue(accent) % 30.0 - 15.0) * 0.5;
    let hues = [
        140.0, 210.0, 24.0, 280.0, 0.0, 175.0, 330.0, 65.0, 95.0, 245.0,
    ];
    tokens.method_foregrounds = hues
        .map(|h| hsl(h + rotation, 0.72, if dark { 0.68 } else { 0.32 }))
        .to_vec();
    tokens.method_foregrounds[method_index(MethodChipKind::Custom)] =
        RgbColor::hex(if dark { 0x00bb_bbbb } else { 0x0044_4444 });
    tokens.method_backgrounds = tokens
        .method_foregrounds
        .iter()
        .map(|color| mix(background, *color, 0.08))
        .collect();
    tokens.success = tokens.method_foregrounds[method_index(MethodChipKind::Get)];
    tokens.warning = tokens.method_foregrounds[method_index(MethodChipKind::Query)];
    tokens.error = tokens.method_foregrounds[method_index(MethodChipKind::Delete)];
    tokens.syntax_key = accent;
    tokens.syntax_string = tokens.method_foregrounds[method_index(MethodChipKind::Get)];
    tokens.syntax_number = tokens.method_foregrounds[method_index(MethodChipKind::Put)];
    tokens.syntax_keyword = tokens.method_foregrounds[method_index(MethodChipKind::Patch)];
    tokens.syntax_punctuation = tokens.secondary;
    let (tokens, notices) = checked(tokens, false);
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
pub fn checked(mut tokens: ThemeTokens, increased: bool) -> (ThemeTokens, Vec<String>) {
    let dark = tokens.preset != DomainThemePreset::Paper;
    let mut notices = Vec::new();
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
    for value in &mut tokens.method_backgrounds {
        *value = surface(*value, dark);
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
    for (foreground, background) in tokens
        .method_foregrounds
        .iter_mut()
        .zip(&tokens.method_backgrounds)
    {
        let mut surfaces = surfaces.to_vec();
        surfaces.push(*background);
        *foreground = strengthen(*foreground, &surfaces, ratio);
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
        .enumerate()
        .map(|(i, kind)| {
            (
                kind,
                DomainMethodChip {
                    background: tokens
                        .method_backgrounds
                        .get(i)
                        .copied()
                        .unwrap_or(tokens.tab_active),
                    foreground: tokens
                        .method_foregrounds
                        .get(i)
                        .copied()
                        .unwrap_or(tokens.primary),
                },
            )
        })
        .collect();
    ResolvedTokens {
        is_dark: tokens.preset != DomainThemePreset::Paper,
        statuses: super::resolved_statuses(&tokens, &chips, increased, false),
        primary: tokens.primary,
        muted: tokens.muted,
        hairline: if increased {
            strengthen(tokens.hairline, &surfaces, 3.0)
        } else {
            tokens.hairline
        },
        card_border: if increased {
            strengthen(tokens.card_border, &surfaces, 3.0)
        } else {
            tokens.card_border
        },
        focus_border: strengthen(
            tokens.card_border,
            &surfaces,
            if increased { 4.5 } else { 3.0 },
        ),
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

fn method_index(kind: MethodChipKind) -> usize {
    MethodChipKind::ALL
        .iter()
        .position(|candidate| *candidate == kind)
        .expect("all method kinds have a palette entry")
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
