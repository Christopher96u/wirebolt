//! Bounded VS Code JSONC import. Includes are reported, never read from disk/network.
use super::{
    RgbColor, ThemeTokens,
    custom::{self, ThemePreview},
};
use serde::Deserialize;
use std::{collections::BTreeMap, fs::File, io::Read, path::Path};

pub const MAX_IMPORT_BYTES: usize = 1_048_576;

#[derive(Debug, Deserialize)]
struct Document {
    name: Option<String>,
    include: Option<String>,
    colors: Option<BTreeMap<String, Option<String>>>,
    #[serde(rename = "tokenColors")]
    token_colors: Option<Vec<TokenRule>>,
}
#[derive(Debug, Deserialize)]
struct TokenRule {
    scope: Option<Scopes>,
    settings: Settings,
}
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Scopes {
    One(String),
    Many(Vec<String>),
}
#[derive(Debug, Deserialize)]
struct Settings {
    foreground: Option<String>,
}

/// # Errors
/// Returns an error for non-JSON, unreadable, oversized or invalid theme files.
pub fn read(path: &Path) -> Result<ThemePreview, String> {
    if !path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("json"))
    {
        return Err("Choose a VS Code colour theme with a .json extension.".into());
    }
    let bytes = read_bounded(path, MAX_IMPORT_BYTES)?;
    parse(&bytes)
}

pub(super) fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    if std::fs::metadata(path).is_ok_and(|m| !m.is_file()) {
        return Err("Choose a regular theme file.".into());
    }
    let file = File::open(path).map_err(|e| format!("Could not open the theme file: {e}"))?;
    if !file.metadata().is_ok_and(|m| m.is_file()) {
        return Err("Choose a regular theme file.".into());
    }
    let mut bytes = Vec::new();
    file.take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Could not read the theme file: {e}"))?;
    if bytes.len() > limit {
        return Err(format!(
            "The theme file is too large (limit {} KiB).",
            limit / 1024
        ));
    }
    Ok(bytes)
}

/// # Errors
/// Returns an error for oversized, malformed or unsupported VS Code theme input.
pub fn parse(bytes: &[u8]) -> Result<ThemePreview, String> {
    if bytes.len() > MAX_IMPORT_BYTES {
        return Err("The theme file is too large (limit 1024 KiB).".into());
    }
    let source = std::str::from_utf8(bytes)
        .map_err(|_| "The theme file must use UTF-8 text.")?
        .trim_start_matches('\u{feff}');
    let uncommented = without_comments(source)?;
    let uncommented = std::str::from_utf8(&uncommented).map_err(|_| "Invalid theme text.")?;
    let json = crate::domain::body::without_trailing_commas(uncommented);
    let doc: Document =
        serde_json::from_slice(&json).map_err(|e| format!("Invalid VS Code theme: {e}"))?;
    if doc.colors.is_none() && doc.token_colors.is_none() {
        return Err("A VS Code theme must contain a colors object or tokenColors array.".into());
    }
    let colors = doc.colors.unwrap_or_default();
    let background = color(&colors, "editor.background", RgbColor::hex(0))?
        .unwrap_or(RgbColor::hex(0x001e_1e1e));
    let accent = color(&colors, "focusBorder", background)?
        .or(color(&colors, "button.background", background)?)
        .unwrap_or(RgbColor::hex(0x0079_c0ff));
    let mut d = custom::derive(background, accent).tokens;
    // Reapply source surfaces so every repair is included in the import report.
    d.window = background;
    map_colors(&mut d, &colors)?;
    for rule in doc.token_colors.unwrap_or_default() {
        apply_rule(&mut d, rule)?;
    }
    let (tokens, mut notices) = custom::checked(d, false);
    if doc.include.is_some() {
        notices.insert(0, "This theme includes a parent theme. Parent files are not resolved; missing colours were derived.".into());
    }
    let name = doc
        .name
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "Imported theme".into());
    Ok(ThemePreview {
        name,
        tokens,
        notices,
    })
}

fn map_colors(
    d: &mut ThemeTokens,
    colors: &BTreeMap<String, Option<String>>,
) -> Result<(), String> {
    let canvas = d.window;
    // More specific editor/list/input tokens win over their general counterparts.
    for (key, destination) in [
        ("sideBar.background", &mut d.sidebar),
        ("input.background", &mut d.card),
        ("list.inactiveSelectionBackground", &mut d.selected),
        ("tab.activeBackground", &mut d.tab_active),
        ("editorWidget.background", &mut d.overlay),
        ("sideBar.border", &mut d.hairline),
        ("input.border", &mut d.card_border),
        ("editor.foreground", &mut d.primary),
        ("sideBar.foreground", &mut d.secondary),
        ("descriptionForeground", &mut d.muted),
        ("editorLineNumber.foreground", &mut d.line_number),
        ("button.background", &mut d.send_background),
        ("button.foreground", &mut d.send_foreground),
        ("editorWarning.foreground", &mut d.warning),
        ("editorError.foreground", &mut d.error),
        ("testing.iconPassed", &mut d.success),
        (
            "editorBracketHighlight.foreground1",
            &mut d.syntax_punctuation,
        ),
    ] {
        if let Some(c) = color(colors, key, canvas)? {
            *destination = c;
        }
    }
    if let Some(c) = color(colors, "list.activeSelectionBackground", canvas)? {
        d.selected = c;
    }
    if let Some(c) = color(colors, "focusBorder", canvas)? {
        d.card_border = c;
    }
    if let Some(c) = color(colors, "input.foreground", canvas)? {
        d.syntax_punctuation = c;
    }
    if let Some(c) = color(colors, "list.activeSelectionForeground", canvas)? {
        // All text must work on all selection and editor surfaces.
        d.primary = c;
    }
    Ok(())
}

fn apply_rule(d: &mut ThemeTokens, rule: TokenRule) -> Result<(), String> {
    let Some(fg) = rule.settings.foreground else {
        return Ok(());
    };
    let c = parse_color(&fg, d.window)
        .ok_or_else(|| format!("Invalid token foreground colour: {fg}"))?;
    let scopes = match rule.scope {
        None => {
            d.primary = c;
            d.syntax_punctuation = c;
            return Ok(());
        }
        Some(Scopes::One(s)) => vec![s],
        Some(Scopes::Many(s)) => s,
    };
    for selectors in scopes {
        for selector in selectors.split(',') {
            // The rightmost scope is the token; ancestors constrain its context.
            let scope = selector.split_whitespace().last().unwrap_or("");
            if scope.starts_with("comment") {
                d.dim = c;
                d.muted = c;
            } else if scope.contains("property") || scope.contains("support.type.property-name") {
                d.syntax_key = c;
            } else if scope.starts_with("string") {
                d.syntax_string = c;
            } else if scope.starts_with("constant.numeric") {
                d.syntax_number = c;
            } else if scope.starts_with("keyword")
                || scope.starts_with("storage")
                || scope.starts_with("constant.language")
            {
                d.syntax_keyword = c;
            } else if scope.starts_with("punctuation") {
                d.syntax_punctuation = c;
            } else if scope.starts_with("variable") {
                d.variable_foreground = c;
            }
        }
    }
    Ok(())
}

fn color(
    colors: &BTreeMap<String, Option<String>>,
    key: &str,
    background: RgbColor,
) -> Result<Option<RgbColor>, String> {
    colors
        .get(key)
        .and_then(Option::as_deref)
        .map(|s| parse_color(s, background).ok_or_else(|| format!("Invalid colour for {key}: {s}")))
        .transpose()
}

fn parse_color(value: &str, background: RgbColor) -> Option<RgbColor> {
    let hex = value.strip_prefix('#')?;
    if !hex.is_ascii() || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let expanded = match hex.len() {
        3 | 4 => hex.chars().flat_map(|c| [c, c]).collect::<String>(),
        6 | 8 => hex.to_owned(),
        _ => return None,
    };
    let n = u32::from_str_radix(&expanded, 16).ok()?;
    let (rgb, alpha) = if expanded.len() == 8 {
        (n >> 8, f64::from(n & 255) / 255.0)
    } else {
        (n, 1.0)
    };
    Some(custom::mix(background, RgbColor::hex(rgb), alpha))
}

fn without_comments(source: &str) -> Result<Vec<u8>, String> {
    let bytes = source.as_bytes();
    let mut output = bytes.to_vec();
    let (mut i, mut quoted, mut escaped) = (0, false, false);
    while i < bytes.len() {
        let b = bytes[i];
        if quoted {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                quoted = false;
            }
            i += 1;
        } else if b == b'"' {
            quoted = true;
            i += 1;
        } else if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && !b"\r\n".contains(&bytes[i]) {
                output[i] = b' ';
                i += 1;
            }
        } else if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            output[i] = b' ';
            output[i + 1] = b' ';
            i += 2;
            loop {
                if i + 1 >= bytes.len() {
                    return Err("Invalid VS Code theme: unterminated block comment.".into());
                }
                if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    output[i] = b' ';
                    output[i + 1] = b' ';
                    i += 2;
                    break;
                }
                if !b"\r\n".contains(&bytes[i]) {
                    output[i] = b' ';
                }
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    Ok(output)
}
