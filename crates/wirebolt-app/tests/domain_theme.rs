use std::collections::HashSet;
use wirebolt_app::domain::theme::*;
const METHODS: [&str; 8] = [
    "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "QUERY",
];
#[test]
fn text_contrast_is_preserved_on_every_surface() {
    for preset in [
        DomainThemePreset::Black,
        DomainThemePreset::Paper,
        DomainThemePreset::Ember,
    ] {
        for increased in [false, true] {
            let t = resolve(preset, increased);
            for surface in [t.definition.window, t.definition.sidebar, t.definition.card] {
                assert!(t.primary.contrast(surface) >= 7.0);
                assert!(t.muted.contrast(surface) >= if increased { 7.0 } else { 4.5 });
                assert!(t.focus_border.contrast(surface) >= if increased { 4.5 } else { 3.0 });
                if increased {
                    assert!(t.hairline.contrast(surface) >= 3.0);
                }
            }
        }
    }
}
#[test]
fn native_controls_are_neutral_and_keep_white_glyphs_legible() {
    for preset in [
        DomainThemePreset::Black,
        DomainThemePreset::Paper,
        DomainThemePreset::Ember,
    ] {
        let d = theme(preset);
        for surface in [d.window, d.sidebar, d.card] {
            assert!(d.control.contrast(surface) >= 3.0);
        }
        assert!(RgbColor::hex(0x00ff_ffff).contrast(d.control) >= 4.5);
        let channels = [d.control.red, d.control.green, d.control.blue];
        assert!(channels.iter().max().unwrap() - channels.iter().min().unwrap() <= 32);
    }
}
#[test]
fn theme_selection_and_density_keep_existing_values() {
    assert_eq!(select_preset("system", false), DomainThemePreset::Paper);
    assert_eq!(select_preset("ember", false), DomainThemePreset::Ember);
    assert_eq!(select_preset("system", true), DomainThemePreset::Black);
    assert_eq!(select_preset("light", true), DomainThemePreset::Paper);
    assert_eq!(select_preset("dark", false), DomainThemePreset::Black);
    assert!((typography(false).body - 16.0).abs() < f64::EPSILON);
    assert!((typography(true).field - 11.0).abs() < f64::EPSILON);
}
#[test]
fn methods_have_distinct_hues_and_readable_chips() {
    for preset in [
        DomainThemePreset::Black,
        DomainThemePreset::Paper,
        DomainThemePreset::Ember,
    ] {
        let t = resolve(preset, false);
        let colors: HashSet<_> = METHODS.iter().map(|m| t.chips[*m].foreground).collect();
        assert_eq!(colors.len(), METHODS.len());
        assert_ne!(t.chips["WS"].foreground, t.chips["GET"].foreground);
        for method in METHODS {
            let c = &t.chips[method];
            assert!(c.foreground.contrast(c.background) >= 4.5);
        }
    }
}
#[test]
fn semantic_text_meets_aa_and_high_contrast_aaa() {
    let colors: Vec<_> = METHODS
        .iter()
        .map(|m| method_color(m, false))
        .chain([method_color("GET", true), method_color("PROPFIND", false)])
        .chain([101, 200, 302, 404, 503].map(status_color))
        .collect();
    for c in colors {
        for background in [0x00ff_ffff, 0x00f6_f6f6, 0x00e8_e8e8].map(RgbColor::hex) {
            assert!(c.light.contrast(background) >= 4.5);
            assert!(c.light_high_contrast.contrast(background) >= 7.0);
        }
        for background in [0x001e_1e1e, 0x002a_2a2a, 0x002c_2c2e].map(RgbColor::hex) {
            assert!(c.dark.contrast(background) >= 4.5);
            assert!(c.dark_high_contrast.contrast(background) >= 7.0);
        }
    }
}
#[test]
fn every_method_and_websocket_has_its_own_semantic_color() {
    let methods: HashSet<_> = METHODS
        .iter()
        .map(|m| method_color(m, false).light)
        .chain([method_color("GET", true).light])
        .collect();
    assert_eq!(methods.len(), METHODS.len() + 1);
    assert_eq!(method_color("POST", true), method_color("GET", true));
}
#[test]
fn status_classes_map_to_distinct_colors() {
    assert_eq!(
        [101, 204, 308, 422, 500].map(status_class),
        [
            DomainStatusClass::Informational,
            DomainStatusClass::Success,
            DomainStatusClass::Redirection,
            DomainStatusClass::ClientError,
            DomainStatusClass::ServerError
        ]
    );
    let colors: HashSet<_> = [101, 200, 301, 404, 500]
        .map(|s| status_color(s).light)
        .into_iter()
        .collect();
    assert_eq!(colors.len(), 5);
}
#[test]
fn contrast_math_matches_wcag_reference_values() {
    let black = RgbColor::hex(0);
    let white = RgbColor::hex(0x00ff_ffff);
    assert!((black.contrast(white) - 21.0).abs() < 0.001);
    assert!((RgbColor::hex(0x0077_7777).contrast(white) - 4.48).abs() < 0.01);
}
