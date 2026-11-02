//! Authored opaque sRGB tokens and WCAG contrast resolution shared by native renderers.
pub mod custom;
pub mod library;
pub mod vscode;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RgbColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}
impl RgbColor {
    #[must_use]
    pub const fn hex(value: u32) -> Self {
        Self {
            red: ((value >> 16) & 255) as u8,
            green: ((value >> 8) & 255) as u8,
            blue: (value & 255) as u8,
        }
    }
    #[must_use]
    pub fn luminance(self) -> f64 {
        fn linear(c: u8) -> f64 {
            let v = f64::from(c) / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * linear(self.red) + 0.7152 * linear(self.green) + 0.0722 * linear(self.blue)
    }
    #[must_use]
    pub fn contrast(self, other: Self) -> f64 {
        let (a, b) = (self.luminance(), other.luminance());
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdaptiveColor {
    pub light: RgbColor,
    pub dark: RgbColor,
    pub light_high_contrast: RgbColor,
    pub dark_high_contrast: RgbColor,
}
impl AdaptiveColor {
    const fn new(l: u32, d: u32, lh: u32, dh: u32) -> Self {
        Self {
            light: RgbColor::hex(l),
            dark: RgbColor::hex(d),
            light_high_contrast: RgbColor::hex(lh),
            dark_high_contrast: RgbColor::hex(dh),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeTokens {
    pub preset: DomainThemePreset,
    pub window: RgbColor,
    pub sidebar: RgbColor,
    pub card: RgbColor,
    pub selected: RgbColor,
    pub tab_active: RgbColor,
    pub overlay: RgbColor,
    pub hairline: RgbColor,
    pub card_border: RgbColor,
    pub primary: RgbColor,
    pub secondary: RgbColor,
    pub muted: RgbColor,
    pub dim: RgbColor,
    pub line_number: RgbColor,
    pub send_background: RgbColor,
    pub send_foreground: RgbColor,
    pub variable_background: RgbColor,
    pub variable_foreground: RgbColor,
    pub syntax_key: RgbColor,
    pub syntax_string: RgbColor,
    pub syntax_number: RgbColor,
    pub syntax_keyword: RgbColor,
    pub syntax_punctuation: RgbColor,
    pub success: RgbColor,
    pub warning: RgbColor,
    pub error: RgbColor,
    pub methods: MethodTokens,
    pub control: RgbColor,
}
#[must_use]
pub fn theme(preset: DomainThemePreset) -> ThemeTokens {
    let values: [u32; 36] = match preset {
        DomainThemePreset::Black => BLACK,
        DomainThemePreset::Paper => PAPER,
        DomainThemePreset::Ember => EMBER,
    };
    let c = values.map(RgbColor::hex);
    ThemeTokens {
        preset,
        window: c[0],
        sidebar: c[1],
        card: c[2],
        selected: c[3],
        tab_active: c[4],
        overlay: c[5],
        hairline: c[6],
        card_border: c[7],
        primary: c[8],
        secondary: c[9],
        muted: c[10],
        dim: c[11],
        line_number: c[12],
        send_background: c[13],
        send_foreground: c[14],
        variable_background: c[15],
        variable_foreground: c[16],
        syntax_key: c[17],
        syntax_string: c[18],
        syntax_number: c[19],
        syntax_keyword: c[20],
        syntax_punctuation: c[21],
        success: c[22],
        warning: c[23],
        error: c[24],
        methods: MethodTokens::from_fn(|kind| {
            let authored = match kind {
                MethodChipKind::Get => Some(0),
                MethodChipKind::Post => Some(1),
                MethodChipKind::Put => Some(2),
                MethodChipKind::Delete => Some(3),
                MethodChipKind::WebSocket => Some(4),
                _ => None,
            };
            let adaptive = kind.color();
            DomainMethodChip {
                background: authored.map_or(c[4], |i| c[25 + i]),
                foreground: authored.map_or_else(
                    || {
                        if preset == DomainThemePreset::Paper {
                            adaptive.light
                        } else {
                            adaptive.dark
                        }
                    },
                    |i| c[30 + i],
                ),
            }
        }),
        control: c[35],
    }
}
#[must_use]
pub fn semantic(name: DomainSemanticColor) -> AdaptiveColor {
    match name {
        DomainSemanticColor::Green => {
            AdaptiveColor::new(0x0015_742C, 0x004A_DE80, 0x000F_5621, 0x0086_EFAC)
        }
        DomainSemanticColor::Blue => {
            AdaptiveColor::new(0x000A_62C4, 0x005A_B0FF, 0x0008_4892, 0x009C_CFFF)
        }
        DomainSemanticColor::Orange => {
            AdaptiveColor::new(0x00A0_4D04, 0x00FF_A24D, 0x0077_3903, 0x00FF_C58A)
        }
        DomainSemanticColor::Purple => {
            AdaptiveColor::new(0x007E_3FC6, 0x00C7_9BFF, 0x005F_2A9E, 0x00DD_C2FF)
        }
        DomainSemanticColor::Red => {
            AdaptiveColor::new(0x00C2_222A, 0x00FF_7B7B, 0x0091_1A20, 0x00FF_ABAB)
        }
        DomainSemanticColor::Teal => {
            AdaptiveColor::new(0x0009_6F73, 0x003D_D6D0, 0x0007_5355, 0x008A_EAE5)
        }
        DomainSemanticColor::Pink => {
            AdaptiveColor::new(0x00B0_277A, 0x00FF_7AC6, 0x008A_1B5F, 0x00FF_B0DD)
        }
        DomainSemanticColor::Indigo => {
            AdaptiveColor::new(0x004B_44D0, 0x00A5_A0FF, 0x003A_33A8, 0x00C9_C6FF)
        }
        DomainSemanticColor::Yellow => {
            AdaptiveColor::new(0x007E_6003, 0x00FF_D426, 0x005E_4702, 0x00FF_E270)
        }
        DomainSemanticColor::Neutral => {
            AdaptiveColor::new(0x005F_6368, 0x00A8_ABB0, 0x0044_474B, 0x00C9_CCD0)
        }
    }
}
/// The finite set of visual HTTP/WebSocket method treatments, including custom verbs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MethodChipKind {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
    Query,
    Custom,
    WebSocket,
}
impl MethodChipKind {
    pub const ALL: [Self; 10] = [
        Self::Get,
        Self::Post,
        Self::Put,
        Self::Patch,
        Self::Delete,
        Self::Head,
        Self::Options,
        Self::Query,
        Self::Custom,
        Self::WebSocket,
    ];
    #[must_use]
    pub const fn method(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
            Self::Query => "QUERY",
            Self::Custom => "CUSTOM",
            Self::WebSocket => "WS",
        }
    }
    fn color(self) -> AdaptiveColor {
        semantic(match self {
            Self::Get => DomainSemanticColor::Green,
            Self::Post => DomainSemanticColor::Blue,
            Self::Put => DomainSemanticColor::Orange,
            Self::Patch => DomainSemanticColor::Purple,
            Self::Delete => DomainSemanticColor::Red,
            Self::Head => DomainSemanticColor::Teal,
            Self::Options => DomainSemanticColor::Pink,
            Self::Query => DomainSemanticColor::Yellow,
            Self::Custom => DomainSemanticColor::Neutral,
            Self::WebSocket => DomainSemanticColor::Indigo,
        })
    }
}
#[must_use]
pub fn method_color(method: &str, ws: bool) -> AdaptiveColor {
    if ws {
        return MethodChipKind::WebSocket.color();
    }
    MethodChipKind::ALL
        .into_iter()
        .filter(|kind| *kind != MethodChipKind::WebSocket)
        .find(|kind| kind.method() == method)
        .unwrap_or(MethodChipKind::Custom)
        .color()
}
#[must_use]
pub fn status_class(status: u16) -> DomainStatusClass {
    match status {
        100..=199 => DomainStatusClass::Informational,
        200..=299 => DomainStatusClass::Success,
        300..=399 => DomainStatusClass::Redirection,
        400..=499 => DomainStatusClass::ClientError,
        _ => DomainStatusClass::ServerError,
    }
}
#[must_use]
pub fn status_color(status: u16) -> AdaptiveColor {
    semantic(match status {
        100..=199 => DomainSemanticColor::Blue,
        200..=299 => DomainSemanticColor::Green,
        300..=399 => DomainSemanticColor::Orange,
        400..=499 => DomainSemanticColor::Yellow,
        _ => DomainSemanticColor::Red,
    })
}
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Interpolation of two byte channels stays in 0...255 and is rounded before conversion."
)]
pub fn strengthen(color: RgbColor, surfaces: &[RgbColor], ratio: f64) -> RgbColor {
    if surfaces.iter().all(|s| color.contrast(*s) >= ratio) {
        return color;
    }
    let target = if surfaces[0].luminance() > 0.5 {
        0.0
    } else {
        255.0
    };
    for step in 1..=255 {
        let component = |v: u8| {
            (f64::from(v) + (target - f64::from(v)) * f64::from(step) / 255.0).round() as u8
        };
        let c = RgbColor {
            red: component(color.red),
            green: component(color.green),
            blue: component(color.blue),
        };
        if surfaces.iter().all(|s| c.contrast(*s) >= ratio) {
            return c;
        }
    }
    RgbColor::hex(if target == 0.0 { 0 } else { 0x00ff_ffff })
}
#[derive(Clone, Debug, PartialEq)]
pub struct TypographyTokens {
    pub body: f64,
    pub detail: f64,
    pub tab: f64,
    pub field: f64,
    pub settings_label: f64,
    pub settings_detail: f64,
    pub brand: f64,
    pub chip: f64,
    pub title: f64,
}
#[must_use]
pub fn typography(compact: bool) -> TypographyTokens {
    TypographyTokens {
        body: if compact { 13.0 } else { 16.0 },
        detail: if compact { 11.0 } else { 14.0 },
        tab: if compact { 12.0 } else { 14.0 },
        field: if compact { 11.0 } else { 13.5 },
        settings_label: if compact { 13.0 } else { 15.0 },
        settings_detail: if compact { 11.0 } else { 13.0 },
        brand: 17.0,
        chip: 10.0,
        title: 26.0,
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainMethodChip {
    pub background: RgbColor,
    pub foreground: RgbColor,
}
/// Every palette names the same complete set of HTTP and WebSocket treatments.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MethodTokens {
    pub get: DomainMethodChip,
    pub post: DomainMethodChip,
    pub put: DomainMethodChip,
    pub patch: DomainMethodChip,
    pub delete: DomainMethodChip,
    pub head: DomainMethodChip,
    pub options: DomainMethodChip,
    pub query: DomainMethodChip,
    pub custom: DomainMethodChip,
    pub web_socket: DomainMethodChip,
}
impl MethodTokens {
    pub fn from_fn(mut value: impl FnMut(MethodChipKind) -> DomainMethodChip) -> Self {
        Self {
            get: value(MethodChipKind::Get),
            post: value(MethodChipKind::Post),
            put: value(MethodChipKind::Put),
            patch: value(MethodChipKind::Patch),
            delete: value(MethodChipKind::Delete),
            head: value(MethodChipKind::Head),
            options: value(MethodChipKind::Options),
            query: value(MethodChipKind::Query),
            custom: value(MethodChipKind::Custom),
            web_socket: value(MethodChipKind::WebSocket),
        }
    }
    #[must_use]
    pub const fn get(&self, kind: MethodChipKind) -> &DomainMethodChip {
        match kind {
            MethodChipKind::Get => &self.get,
            MethodChipKind::Post => &self.post,
            MethodChipKind::Put => &self.put,
            MethodChipKind::Patch => &self.patch,
            MethodChipKind::Delete => &self.delete,
            MethodChipKind::Head => &self.head,
            MethodChipKind::Options => &self.options,
            MethodChipKind::Query => &self.query,
            MethodChipKind::Custom => &self.custom,
            MethodChipKind::WebSocket => &self.web_socket,
        }
    }
    pub fn values_mut(&mut self) -> [&mut DomainMethodChip; 10] {
        [
            &mut self.get,
            &mut self.post,
            &mut self.put,
            &mut self.patch,
            &mut self.delete,
            &mut self.head,
            &mut self.options,
            &mut self.query,
            &mut self.custom,
            &mut self.web_socket,
        ]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedTokens {
    pub is_dark: bool,
    pub statuses: std::collections::HashMap<DomainStatusClass, RgbColor>,
    pub definition: ThemeTokens,
    pub primary: RgbColor,
    pub muted: RgbColor,
    pub hairline: RgbColor,
    pub card_border: RgbColor,
    pub focus_border: RgbColor,
    pub chips: std::collections::HashMap<MethodChipKind, DomainMethodChip>,
}
#[must_use]
pub fn resolve(preset: DomainThemePreset, increased: bool) -> ResolvedTokens {
    let d = theme(preset);
    let s = [d.window, d.sidebar, d.card];
    let mut chips = std::collections::HashMap::new();
    for kind in MethodChipKind::ALL {
        let chip = d.methods.get(kind);
        let adaptive = kind.color();
        let foreground = if increased {
            if preset == DomainThemePreset::Paper {
                adaptive.light_high_contrast
            } else {
                adaptive.dark_high_contrast
            }
        } else {
            chip.foreground
        };
        chips.insert(
            kind,
            DomainMethodChip {
                background: chip.background,
                foreground,
            },
        );
    }
    let borders = resolve_borders(&d, &s, increased);
    ResolvedTokens {
        is_dark: preset != DomainThemePreset::Paper,
        statuses: resolved_statuses(&d, &chips, increased, true),
        primary: strengthen(d.primary, &s, 7.0),
        muted: strengthen(d.muted, &s, if increased { 7.0 } else { 4.5 }),
        hairline: borders.hairline,
        card_border: borders.card,
        focus_border: borders.focus,
        definition: d,
        chips,
    }
}
pub(super) struct ResolvedBorders {
    pub hairline: RgbColor,
    pub card: RgbColor,
    pub focus: RgbColor,
}
pub(super) fn resolve_borders(
    tokens: &ThemeTokens,
    surfaces: &[RgbColor],
    increased: bool,
) -> ResolvedBorders {
    ResolvedBorders {
        hairline: if increased {
            strengthen(tokens.hairline, surfaces, 3.0)
        } else {
            tokens.hairline
        },
        card: if increased {
            strengthen(tokens.card_border, surfaces, 3.0)
        } else {
            tokens.card_border
        },
        focus: strengthen(
            tokens.card_border,
            surfaces,
            if increased { 4.5 } else { 3.0 },
        ),
    }
}

/// Persisted strings are decoded once at the preference boundary, including rc.1 aliases.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThemeSelection {
    System,
    BuiltIn { preset: DomainThemePreset },
    Custom { id: String },
}
impl ThemeSelection {
    #[must_use]
    pub fn from_preference(value: &str) -> Self {
        match value {
            "light" => Self::BuiltIn {
                preset: DomainThemePreset::Paper,
            },
            "dark" => Self::BuiltIn {
                preset: DomainThemePreset::Black,
            },
            _ => DomainThemePreset::from_preference(value).map_or_else(
                || {
                    if value.starts_with("custom:") {
                        Self::Custom { id: value.into() }
                    } else {
                        Self::System
                    }
                },
                |preset| Self::BuiltIn { preset },
            ),
        }
    }
    #[must_use]
    pub fn preference_value(&self) -> String {
        match self {
            Self::System => "system".into(),
            Self::BuiltIn { preset } => preset.preference_value().into(),
            Self::Custom { id } => id.clone(),
        }
    }
    #[must_use]
    pub const fn preset(&self, dark: bool) -> DomainThemePreset {
        match self {
            Self::BuiltIn { preset } => *preset,
            _ if dark => DomainThemePreset::Black,
            _ => DomainThemePreset::Paper,
        }
    }
}

const BLACK: [u32; 36] = [
    0x0000_0000,
    0x0002_0203,
    0x0010_1010,
    0x0026_2626,
    0x001a_1a1a,
    0x000b_0b0b,
    0x0019_1919,
    0x0023_2323,
    0x00f1_f3f7,
    0x00cf_cfcf,
    0x00a3_a3a3,
    0x006b_6b6b,
    0x0044_4444,
    0x0036_3636,
    0x00ed_eded,
    0x001d_1e24,
    0x00a8_b4ff,
    0x0079_c0ff,
    0x00a5_d6ff,
    0x00ff_a657,
    0x00ff_7b72,
    0x00c9_d1d9,
    0x004a_de80,
    0x00ff_d426,
    0x00ff_7b7b,
    0x000f_2a1b,
    0x000f_2233,
    0x002e_1d0c,
    0x002e_1214,
    0x001e_1838,
    0x004a_de80,
    0x004a_a3ff,
    0x00ff_a24d,
    0x00ff_7b7b,
    0x00a5_a0ff,
    0x0066_6666,
];
const PAPER: [u32; 36] = [
    0x00fc_fcfd,
    0x00f4_f4f5,
    0x00ff_ffff,
    0x00ff_ffff,
    0x00ed_edf0,
    0x00ff_ffff,
    0x00e4_e4e7,
    0x00e4_e4e7,
    0x0018_181b,
    0x003f_3f46,
    0x0071_717a,
    0x00a1_a1aa,
    0x00c4_c4cc,
    0x0018_181b,
    0x00ff_ffff,
    0x00ee_f0ff,
    0x0043_38ca,
    0x0005_50ae,
    0x000a_3069,
    0x0095_3800,
    0x00cf_222e,
    0x0024_292f,
    0x0015_803d,
    0x00a1_6207,
    0x00b9_1c1c,
    0x00dc_fce7,
    0x00db_eafe,
    0x00ff_edd5,
    0x00fe_e2e2,
    0x00ed_e9fe,
    0x0015_803d,
    0x001d_4ed8,
    0x00c2_410c,
    0x00b9_1c1c,
    0x006d_28d9,
    0x0052_525b,
];
const EMBER: [u32; 36] = [
    0x0012_0d0b,
    0x0015_0f0c,
    0x001c_1512,
    0x002e_221c,
    0x0026_1c17,
    0x001a_1310,
    0x002a_1f1a,
    0x0030_2722,
    0x00f6_ede7,
    0x00dc_cbc2,
    0x00b5_a299,
    0x007d_6b62,
    0x005a_4a42,
    0x00f9_7316,
    0x001a_0e07,
    0x002e_1e15,
    0x00fd_ba74,
    0x00fd_ba74,
    0x00fd_e2c4,
    0x00fc_a5a5,
    0x00f9_7316,
    0x00e7_d8cf,
    0x004a_de80,
    0x00fb_bf24,
    0x00ff_8a80,
    0x0013_2a18,
    0x0014_212e,
    0x0033_200e,
    0x0033_1517,
    0x0024_1c33,
    0x004a_de80,
    0x0060_b0ff,
    0x00ff_b066,
    0x00ff_8a80,
    0x00b8_b0ff,
    0x007d_6b62,
];
#[derive(Clone, Debug, PartialEq)]
pub struct GeometryTokens {
    pub chip: f64,
    pub tab: f64,
    pub row: f64,
    pub card: f64,
    pub window: f64,
}
#[must_use]
pub const fn geometry() -> GeometryTokens {
    GeometryTokens {
        chip: 4.0,
        tab: 8.0,
        row: 10.0,
        card: 16.0,
        window: 12.0,
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct DensityTokens {
    pub control_height: f64,
    pub row_height: f64,
    pub body_size: f64,
    pub detail_size: f64,
}
#[must_use]
pub const fn density(compact: bool) -> DensityTokens {
    DensityTokens {
        control_height: if compact { 28.0 } else { 32.0 },
        row_height: if compact { 28.0 } else { 64.0 },
        body_size: if compact { 13.0 } else { 16.0 },
        detail_size: if compact { 11.0 } else { 14.0 },
    }
}
#[must_use]
pub fn backgrounds(dark: bool) -> Vec<RgbColor> {
    if dark {
        vec![0x001e_1e1e, 0x002a_2a2a, 0x002c_2c2e]
    } else {
        vec![0x00ff_ffff, 0x00f6_f6f6, 0x00e8_e8e8]
    }
    .into_iter()
    .map(RgbColor::hex)
    .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DomainThemePreset {
    Black,
    Paper,
    Ember,
}
impl DomainThemePreset {
    #[must_use]
    pub const fn preference_value(self) -> &'static str {
        match self {
            Self::Black => "black",
            Self::Paper => "paper",
            Self::Ember => "ember",
        }
    }
    #[must_use]
    pub fn from_preference(value: &str) -> Option<Self> {
        [Self::Black, Self::Paper, Self::Ember]
            .into_iter()
            .find(|preset| preset.preference_value() == value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomainSemanticColor {
    Green,
    Blue,
    Orange,
    Purple,
    Red,
    Teal,
    Pink,
    Indigo,
    Yellow,
    Neutral,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DomainStatusClass {
    Informational,
    Success,
    Redirection,
    ClientError,
    ServerError,
}

pub(super) fn resolved_statuses(
    definition: &ThemeTokens,
    chips: &std::collections::HashMap<MethodChipKind, DomainMethodChip>,
    increased: bool,
    built_in: bool,
) -> std::collections::HashMap<DomainStatusClass, RgbColor> {
    [100, 200, 300, 400, 500]
        .into_iter()
        .map(|status| {
            let class = status_class(status);
            let color = if increased && built_in {
                let adaptive = status_color(status);
                if definition.preset == DomainThemePreset::Paper {
                    adaptive.light_high_contrast
                } else {
                    adaptive.dark_high_contrast
                }
            } else {
                match class {
                    DomainStatusClass::Informational => chips[&MethodChipKind::Post].foreground,
                    DomainStatusClass::Success => definition.success,
                    DomainStatusClass::Redirection => chips[&MethodChipKind::Put].foreground,
                    DomainStatusClass::ClientError => definition.warning,
                    DomainStatusClass::ServerError => definition.error,
                }
            };
            (class, color)
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedAppearance {
    pub tokens: ResolvedTokens,
    pub custom: Option<library::CustomTheme>,
    pub follows_system: bool,
}
#[must_use]
pub fn resolve_selection(
    selection: &ThemeSelection,
    themes: &[library::CustomTheme],
    system_is_dark: bool,
    increased: bool,
) -> ResolvedAppearance {
    let custom = if let ThemeSelection::Custom { id } = selection {
        themes.iter().find(|theme| &theme.id == id).cloned()
    } else {
        None
    };
    let tokens = custom.as_ref().map_or_else(
        || resolve(selection.preset(system_is_dark), increased),
        |theme| custom::resolve(theme.tokens.clone(), increased),
    );
    let follows_system = !matches!(selection, ThemeSelection::BuiltIn { .. }) && custom.is_none();
    ResolvedAppearance {
        tokens,
        custom,
        follows_system,
    }
}

fn read_bounded(
    path: &std::path::Path,
    limit: usize,
    description: &str,
) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let regular_file = || format!("Choose a regular {description}.");
    if std::fs::metadata(path).is_ok_and(|m| !m.is_file()) {
        return Err(regular_file());
    }
    let file =
        std::fs::File::open(path).map_err(|e| format!("Could not open the {description}: {e}"))?;
    if !file.metadata().is_ok_and(|m| m.is_file()) {
        return Err(regular_file());
    }
    let mut bytes = Vec::new();
    file.take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Could not read the {description}: {e}"))?;
    if bytes.len() > limit {
        return Err(format!(
            "The {description} is too large (limit {} KiB).",
            limit / 1024
        ));
    }
    Ok(bytes)
}
