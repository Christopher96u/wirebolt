//! Authored opaque sRGB tokens and WCAG contrast resolution shared by native renderers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeTokens {
    pub preset: String,
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
    pub method_backgrounds: Vec<RgbColor>,
    pub method_foregrounds: Vec<RgbColor>,
    pub control: RgbColor,
}
#[must_use]
pub fn theme(preset: &str) -> ThemeTokens {
    let values: [u32; 36] = match preset {
        "black" => BLACK,
        "paper" => PAPER,
        "ember" => EMBER,
        _ => return theme("black"),
    };
    let c = values.map(RgbColor::hex);
    ThemeTokens {
        preset: preset.into(),
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
        method_backgrounds: c[25..30].to_vec(),
        method_foregrounds: c[30..35].to_vec(),
        control: c[35],
    }
}
#[must_use]
pub fn semantic(name: &str) -> AdaptiveColor {
    match name {
        "green" => AdaptiveColor::new(0x0015_742C, 0x004A_DE80, 0x000F_5621, 0x0086_EFAC),
        "blue" => AdaptiveColor::new(0x000A_62C4, 0x005A_B0FF, 0x0008_4892, 0x009C_CFFF),
        "orange" => AdaptiveColor::new(0x00A0_4D04, 0x00FF_A24D, 0x0077_3903, 0x00FF_C58A),
        "purple" => AdaptiveColor::new(0x007E_3FC6, 0x00C7_9BFF, 0x005F_2A9E, 0x00DD_C2FF),
        "red" => AdaptiveColor::new(0x00C2_222A, 0x00FF_7B7B, 0x0091_1A20, 0x00FF_ABAB),
        "teal" => AdaptiveColor::new(0x0009_6F73, 0x003D_D6D0, 0x0007_5355, 0x008A_EAE5),
        "pink" => AdaptiveColor::new(0x00B0_277A, 0x00FF_7AC6, 0x008A_1B5F, 0x00FF_B0DD),
        "indigo" => AdaptiveColor::new(0x004B_44D0, 0x00A5_A0FF, 0x003A_33A8, 0x00C9_C6FF),
        "yellow" => AdaptiveColor::new(0x007E_6003, 0x00FF_D426, 0x005E_4702, 0x00FF_E270),
        "neutral" => AdaptiveColor::new(0x005F_6368, 0x00A8_ABB0, 0x0044_474B, 0x00C9_CCD0),
        _ => semantic("neutral"),
    }
}
#[must_use]
pub fn method_color(method: &str, ws: bool) -> AdaptiveColor {
    semantic(if ws {
        "indigo"
    } else {
        match method {
            "GET" => "green",
            "POST" => "blue",
            "PUT" => "orange",
            "PATCH" => "purple",
            "DELETE" => "red",
            "HEAD" => "teal",
            "OPTIONS" => "pink",
            "QUERY" => "yellow",
            _ => "neutral",
        }
    })
}
#[must_use]
pub fn status_class(status: u16) -> String {
    match status {
        100..=199 => "informational",
        200..=299 => "success",
        300..=399 => "redirection",
        400..=499 => "clientError",
        _ => "serverError",
    }
    .into()
}
#[must_use]
pub fn status_color(status: u16) -> AdaptiveColor {
    semantic(match status {
        100..=199 => "blue",
        200..=299 => "green",
        300..=399 => "orange",
        400..=499 => "yellow",
        _ => "red",
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainMethodChip {
    pub background: RgbColor,
    pub foreground: RgbColor,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedTokens {
    pub definition: ThemeTokens,
    pub primary: RgbColor,
    pub muted: RgbColor,
    pub hairline: RgbColor,
    pub card_border: RgbColor,
    pub focus_border: RgbColor,
    pub chips: std::collections::HashMap<String, DomainMethodChip>,
}
#[must_use]
pub fn resolve(preset: &str, increased: bool) -> ResolvedTokens {
    let d = theme(preset);
    let s = [d.window, d.sidebar, d.card];
    let mut chips = std::collections::HashMap::new();
    for method in [
        "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "QUERY", "CUSTOM", "WS",
    ] {
        let index = match method {
            "GET" => Some(0),
            "POST" => Some(1),
            "PUT" => Some(2),
            "DELETE" => Some(3),
            "WS" => Some(4),
            _ => None,
        };
        let adaptive = method_color(method, method == "WS");
        let dark = preset != "paper";
        let foreground = if increased {
            if dark {
                adaptive.dark_high_contrast
            } else {
                adaptive.light_high_contrast
            }
        } else if let Some(i) = index {
            d.method_foregrounds[i]
        } else if dark {
            adaptive.dark
        } else {
            adaptive.light
        };
        chips.insert(
            method.into(),
            DomainMethodChip {
                background: index.map_or(d.tab_active, |i| d.method_backgrounds[i]),
                foreground,
            },
        );
    }
    ResolvedTokens {
        primary: strengthen(d.primary, &s, 7.0),
        muted: strengthen(d.muted, &s, if increased { 7.0 } else { 4.5 }),
        hairline: if increased {
            strengthen(d.hairline, &s, 3.0)
        } else {
            d.hairline
        },
        card_border: if increased {
            strengthen(d.card_border, &s, 3.0)
        } else {
            d.card_border
        },
        focus_border: strengthen(d.card_border, &s, if increased { 4.5 } else { 3.0 }),
        definition: d,
        chips,
    }
}
#[must_use]
pub fn select_preset(selection: &str, dark: bool) -> String {
    match selection {
        "black" | "paper" | "ember" => selection,
        "light" => "paper",
        "dark" => "black",
        _ => {
            if dark {
                "black"
            } else {
                "paper"
            }
        }
    }
    .into()
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
