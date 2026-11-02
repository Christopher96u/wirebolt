use std::{collections::HashSet, fs};
use wirebolt_app::domain::theme::{custom, library::ThemeLibrary, vscode, *};

fn assert_contrast(tokens: &ThemeTokens) {
    for increased in [false, true] {
        let t = custom::resolve(tokens.clone(), increased);
        let d = &t.definition;
        let ratio = if increased { 7.0 } else { 4.5 };
        for s in [
            d.window,
            d.sidebar,
            d.card,
            d.selected,
            d.tab_active,
            d.overlay,
            d.variable_background,
        ] {
            assert!(t.primary.contrast(s) >= 7.0);
            for c in [
                t.muted,
                d.secondary,
                d.dim,
                d.line_number,
                d.syntax_key,
                d.syntax_string,
                d.syntax_number,
                d.syntax_keyword,
                d.syntax_punctuation,
                d.success,
                d.warning,
                d.error,
                d.variable_foreground,
            ] {
                assert!(
                    c.contrast(s) >= ratio,
                    "{c:?} vs {s:?}, increased {increased}"
                );
            }
            assert!(t.focus_border.contrast(s) >= if increased { 4.5 } else { 3.0 });
            if increased {
                assert!(t.hairline.contrast(s) >= 3.0);
                assert!(t.card_border.contrast(s) >= 3.0);
            }
            assert!(d.control.contrast(s) >= 3.0);
        }
        assert!(RgbColor::hex(0x00ff_ffff).contrast(d.control) >= 4.5);
        assert!(d.send_foreground.contrast(d.send_background) >= ratio);
        assert!(d.variable_foreground.contrast(d.variable_background) >= ratio);
        for chip in t.chips.values() {
            assert!(chip.foreground.contrast(chip.background) >= ratio);
        }
        let colors: HashSet<_> = t.chips.values().map(|c| c.foreground).collect();
        assert_eq!(colors.len(), MethodChipKind::ALL.len());
        let statuses = [
            t.chips[&MethodChipKind::Post].foreground,
            d.success,
            t.chips[&MethodChipKind::Put].foreground,
            d.warning,
            d.error,
        ];
        assert_eq!(statuses.into_iter().collect::<HashSet<_>>().len(), 5);
        let colors: Vec<_> = MethodChipKind::ALL
            .map(|kind| d.methods.get(kind).foreground)
            .into();
        for (i, a) in colors.iter().enumerate() {
            for b in &colors[i + 1..] {
                let distance: i32 = [a.red, a.green, a.blue]
                    .into_iter()
                    .zip([b.red, b.green, b.blue])
                    .map(|(a, b)| (i32::from(a) - i32::from(b)).pow(2))
                    .sum();
                assert!(distance >= 24 * 24, "method colors too close: {a:?} {b:?}");
            }
        }
    }
}

#[test]
fn derived_palettes_meet_contrast_and_distinguish_methods_across_color_pairs() {
    let values = [
        0,
        0x00ff_ffff,
        0x0077_7777,
        0x0099_9999,
        0x00ff_0000,
        0x0000_ff00,
        0x0000_00ff,
        0x00ff_ff00,
        0x00ff_00ff,
        0x0000_ffff,
        0x0001_0203,
        0x00fc_fafa,
        0x003f_407f,
        0x007f_5a30,
    ];
    for background in values {
        for accent in values {
            assert_contrast(
                &custom::derive(RgbColor::hex(background), RgbColor::hex(accent)).tokens,
            );
        }
    }
    // Deterministic broad coverage, without a random dependency or machine-specific seed.
    let mut seed = 9_u32;
    for _ in 0..256 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let background = RgbColor::hex(seed);
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        assert_contrast(&custom::derive(background, RgbColor::hex(seed)).tokens);
    }
}

#[test]
fn vscode_jsonc_maps_colors_scopes_and_reports_repairs_and_parent() {
    let tidal = vscode::parse(include_bytes!("fixtures/themes/tidal.json")).unwrap();
    assert_eq!(tidal.name, "Tidal");
    assert_eq!(tidal.tokens.window, RgbColor::hex(0x0007_171f));
    assert_eq!(tidal.tokens.sidebar, RgbColor::hex(0x0006_1319));
    assert_eq!(tidal.tokens.syntax_key, RgbColor::hex(0x008d_cfff));
    assert_eq!(tidal.tokens.syntax_string, RgbColor::hex(0x00a8_e0b4));
    assert_eq!(tidal.tokens.syntax_number, RgbColor::hex(0x00ef_bc85));
    assert!(tidal.notices.iter().any(|n| n.contains("Comment color")));
    assert_contrast(&tidal.tokens);
    let linen = vscode::parse(include_bytes!("fixtures/themes/linen.json")).unwrap();
    assert_eq!(linen.tokens.preset, DomainThemePreset::Paper);
    assert!(
        linen
            .notices
            .iter()
            .any(|n| n.contains("Parent files are not resolved"))
    );
    assert_contrast(&linen.tokens);
    let minimal = vscode::parse(br##"{"colors":{"editor.background":"#123"},"tokenColors":[{"settings":{"foreground":"#fff"}}]}"##).unwrap();
    assert_eq!(minimal.tokens.primary, RgbColor::hex(0x00ff_ffff));
}

#[test]
fn vscode_rejects_malformed_oversized_and_non_theme_inputs_without_panicking() {
    for bytes in [
        include_bytes!("fixtures/themes/invalid.json").as_slice(),
        b"[]",
        b"{}",
        b"{",
        b"/* unfinished",
        b"{\"colors\":{,,}}",
        b"{\"colors\":[],}",
        b"{\"tokenColors\":\"file.tmTheme\"}",
        b"\xff\xfe",
    ] {
        assert!(vscode::parse(bytes).is_err(), "{bytes:?}");
    }
    assert!(
        vscode::parse(&vec![b' '; vscode::MAX_IMPORT_BYTES + 1])
            .unwrap_err()
            .contains("too large")
    );
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("large.json");
    fs::write(&path, vec![b' '; vscode::MAX_IMPORT_BYTES + 1]).unwrap();
    assert!(vscode::read(&path).unwrap_err().contains("too large"));
    assert!(
        vscode::read(&root.path().join("missing.json"))
            .unwrap_err()
            .contains("Could not open")
    );
    fs::write(root.path().join("theme.txt"), b"{}").unwrap();
    assert!(
        vscode::read(&root.path().join("theme.txt"))
            .unwrap_err()
            .contains(".json")
    );
}

#[test]
fn jsonc_preserves_quoted_comments_escapes_bom_and_rejects_extra_commas() {
    let source = "\u{feff}{\"name\":\"https://x/*x*/\\\"end\",/* 🌊 */\"colors\":{},// hi\n\"tokenColors\":[],}";
    assert_eq!(
        vscode::parse(source.as_bytes()).unwrap().name,
        "https://x/*x*/\"end"
    );
    for source in [
        "{\"colors\":{,}}",
        "{\"colors\":{},,}",
        "{\"tokenColors\":[,]}",
        "{\"colors\":{},} trailing",
    ] {
        assert!(vscode::parse(source.as_bytes()).is_err());
    }
}

#[test]
fn library_round_trips_renames_deletes_and_falls_back_only_for_active_theme() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("themes.json");
    let mut library = ThemeLibrary::open(&path);
    let theme = library
        .add(
            " Ocean ",
            custom::derive(RgbColor::hex(0x0012_1314), RgbColor::hex(0x0077_ccff)).tokens,
        )
        .unwrap();
    assert_eq!(theme.name, "Ocean");
    assert_eq!(
        ThemeLibrary::open(&path).snapshot().themes.as_slice(),
        std::slice::from_ref(&theme)
    );
    library.rename(&theme.id, "Deep sea").unwrap();
    assert_eq!(
        ThemeLibrary::open(&path).snapshot().themes[0].name,
        "Deep sea"
    );
    assert!(library.rename(&theme.id, " ").is_err());
    assert!(library.rename(&theme.id, &"x".repeat(65)).is_err());
    assert_eq!(
        library
            .delete(
                &theme.id,
                &ThemeSelection::Custom {
                    id: theme.id.clone()
                }
            )
            .unwrap(),
        ThemeSelection::System
    );
    assert!(ThemeLibrary::open(&path).snapshot().themes.is_empty());
    let other = library.add("Other", theme.tokens).unwrap();
    assert_eq!(
        library
            .delete(
                &other.id,
                &ThemeSelection::BuiltIn {
                    preset: DomainThemePreset::Paper
                }
            )
            .unwrap(),
        ThemeSelection::BuiltIn {
            preset: DomainThemePreset::Paper
        }
    );
}

#[test]
fn corrupt_library_reports_damage_and_unsupported_versions_are_read_only() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("themes.json");
    {
        let original = "broken";
        fs::write(&path, original).unwrap();
        let mut library = ThemeLibrary::open(&path);
        assert!(library.snapshot().themes.is_empty());
        assert!(!library.snapshot().notices.is_empty());
        library
            .add(
                "Recovered",
                custom::derive(RgbColor::hex(0), RgbColor::hex(0x0077_ccff)).tokens,
            )
            .unwrap();
    }
    let future = "{\"version\":99,\"themes\":[],\"future\":true}";
    fs::write(&path, future).unwrap();
    let mut library = ThemeLibrary::open(&path);
    assert!(!library.snapshot().writable);
    assert!(
        library
            .add(
                "No overwrite",
                custom::derive(RgbColor::hex(0), RgbColor::hex(0)).tokens
            )
            .is_err()
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), future);
}

#[test]
fn failed_library_write_keeps_snapshot_and_invalid_tokens_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let parent = root.path().join("not-a-directory");
    fs::write(&parent, "x").unwrap();
    let mut library = ThemeLibrary::open(&parent.join("themes.json"));
    let tokens = custom::derive(RgbColor::hex(0), RgbColor::hex(0)).tokens;
    assert!(
        library
            .add("Failure", tokens.clone())
            .unwrap_err()
            .contains("Could not save")
    );
    assert!(library.snapshot().themes.is_empty());
    let mut incomplete = serde_json::to_value(&tokens).unwrap();
    incomplete["methods"].as_object_mut().unwrap().remove("get");
    assert!(serde_json::from_value::<ThemeTokens>(incomplete).is_err());
}

#[test]
fn imported_unresolved_variable_errors_meet_contrast_on_variable_chips() {
    let preview = vscode::parse(br##"{"colors":{"editor.background":"#fff","focusBorder":"#000","list.activeSelectionBackground":"#fff","input.background":"#fff","editorError.foreground":"#777"}}"##).unwrap();
    for increased in [false, true] {
        let tokens = custom::resolve(preview.tokens.clone(), increased).definition;
        assert!(
            tokens.error.contrast(tokens.variable_background) >= if increased { 7.0 } else { 4.5 }
        );
    }
    assert_contrast(&preview.tokens);
}

#[test]
fn vscode_include_only_themes_derive_gaps_and_show_parent_warning() {
    let preview = vscode::parse(br#"{"name":"Child","include":"./parent.json"}"#).unwrap();
    assert_eq!(preview.name, "Child");
    assert!(
        preview
            .notices
            .iter()
            .any(|n| n.contains("Parent files are not resolved"))
    );
    assert_contrast(&preview.tokens);
    assert!(vscode::parse(br#"{"include":"  "}"#).is_err());
}

#[test]
fn custom_borders_are_neutral_and_follow_final_surfaces() {
    let mut previews = vec![vscode::parse(include_bytes!("fixtures/themes/linen.json")).unwrap()];
    for background in [
        0x00ff_9900,
        0x0033_66ff,
        0,
        0x00ff_ffff,
        0x0077_7777,
        0x00ff_0000,
        0x0000_ff00,
        0x0000_ffff,
    ] {
        for accent in [0x00ff_00ff, 0x0000_ff00, 0x0077_7777] {
            previews.push(custom::derive(
                RgbColor::hex(background),
                RgbColor::hex(accent),
            ));
            let source = format!(
                r##"{{"colors":{{"editor.background":"#{background:06x}","focusBorder":"#{accent:06x}","input.border":"#{accent:06x}"}}}}"##
            );
            previews.push(vscode::parse(source.as_bytes()).unwrap());
        }
    }
    for preview in previews {
        for increased in [false, true] {
            let resolved = custom::resolve(preview.tokens.clone(), increased);
            for control in [
                resolved.definition.send_background,
                resolved.definition.send_foreground,
            ] {
                assert_eq!(control.red, control.green, "{control:?}");
                assert_eq!(control.green, control.blue, "{control:?}");
            }
            for border in [
                resolved.definition.hairline,
                resolved.definition.card_border,
                resolved.hairline,
                resolved.card_border,
                resolved.focus_border,
            ] {
                assert_eq!(border.red, border.green, "{border:?}");
                assert_eq!(border.green, border.blue, "{border:?}");
                assert_eq!(
                    border.luminance() > resolved.definition.window.luminance(),
                    resolved.definition.preset != DomainThemePreset::Paper
                );
            }
        }
    }
}

#[test]
fn adjustment_notices_use_human_color_names() {
    let preview = custom::derive(RgbColor::hex(0x00ff_9900), RgbColor::hex(0x00ff_cc00));
    assert!(
        preview
            .notices
            .iter()
            .any(|notice| notice.starts_with("Selection color "))
    );
    assert!(preview.notices.iter().all(|notice| !notice.contains('/')));
    let preview = vscode::parse(br##"{"colors":{"editor.background":"#ffffff"},"tokenColors":[{"scope":"comment","settings":{"foreground":"#eeeeee"}}]}"##).unwrap();
    assert!(
        preview
            .notices
            .iter()
            .any(|notice| notice.starts_with("Comment color "))
    );
}

#[test]
fn saved_selection_resolves_custom_builtin_system_and_missing_themes_in_rust() {
    let custom = wirebolt_app::domain::theme::library::CustomTheme {
        id: "custom:fixture".into(),
        name: "Linen".into(),
        tokens: vscode::parse(include_bytes!("fixtures/themes/linen.json"))
            .unwrap()
            .tokens,
    };
    let themes = [custom.clone()];
    for increased in [false, true] {
        let selected = resolve_selection(
            &ThemeSelection::Custom {
                id: custom.id.clone(),
            },
            &themes,
            true,
            increased,
        );
        assert_eq!(selected.custom, Some(custom.clone()));
        assert!(!selected.follows_system && !selected.tokens.is_dark);
        for (selection, dark, follows_system) in [
            ("paper", false, false),
            ("black", true, false),
            ("ember", true, false),
            ("system", true, true),
            ("custom:missing", true, true),
        ] {
            let resolved = resolve_selection(
                &ThemeSelection::from_preference(selection),
                &themes,
                true,
                increased,
            );
            assert!(resolved.custom.is_none());
            assert_eq!(resolved.tokens.is_dark, dark);
            assert_eq!(resolved.follows_system, follows_system);
            for status in [100, 200, 300, 400, 500] {
                let color = resolved.tokens.statuses[&status_class(status)];
                assert!(color.contrast(resolved.tokens.definition.window) >= 4.5);
            }
        }
    }
}

#[test]
fn theme_library_reader_names_the_library_and_never_calls_it_a_theme_file() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("themes.json");
    fs::write(&path, vec![b' '; 4 * 1_048_576 + 1]).unwrap();
    let notices = ThemeLibrary::open(&path).snapshot().notices.join(" ");
    assert!(notices.contains("custom theme library is too large"));
    assert!(!notices.contains("theme file"));
}
