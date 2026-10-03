use super::*;

/// Truecolor caps with the given Nerd availability.
fn caps(nerd_font: bool) -> TermCaps {
    TermCaps {
        truecolor: true,
        nerd_font,
        ..TermCaps::default()
    }
}

#[test]
fn partial_toml_overrides_only_given_keys() {
    let caps = caps(false);
    let src = r##"
            accent = "#ff0000"
            error = "red"
            [files]
            image = "#00ff00"
        "##;
    let (theme, icons) = load_from(src, &caps, None);
    assert_eq!(theme.accent, Color::Rgb(255, 0, 0));
    assert_eq!(theme.error, Color::Red);
    assert_eq!(theme.image, Color::Rgb(0, 255, 0));
    // Untouched key keeps the default.
    assert_eq!(theme.bg, Theme::default().bg);
    assert_eq!(icons, IconSet::Unicode);
}

#[test]
fn invalid_colors_and_bad_toml_are_ignored() {
    let caps = caps(false);
    let (theme, _) = load_from("accent = \"not-a-color\"\n", &caps, None);
    assert_eq!(theme.accent, Theme::default().accent);

    let (theme, _) = load_from("this is not toml {{{", &caps, None);
    assert_eq!(theme, Theme::default());
}

#[test]
fn adapt_removes_every_rgb_when_truecolor_is_off() {
    let mut theme = Theme::default();
    assert!(
        theme
            .all_colors()
            .iter()
            .any(|c| matches!(c, Color::Rgb(_, _, _))),
        "default palette is truecolor"
    );
    theme.adapt(&TermCaps {
        truecolor: false,
        ..TermCaps::default()
    });
    for c in theme.all_colors() {
        assert!(
            !matches!(c, Color::Rgb(_, _, _)),
            "RGB survived adapt(): {c:?}"
        );
    }
}

#[test]
fn adapt_is_a_noop_when_truecolor_is_on() {
    let mut theme = Theme::default();
    let original = theme;
    theme.adapt(&caps(true));
    assert_eq!(theme, original);
}

#[test]
fn ira_icons_env_beats_toml_and_auto() {
    let nerd = caps(true);
    let (_, icons) = load_from("icons = \"nerd\"\n", &nerd, Some("unicode"));
    assert_eq!(icons, IconSet::Unicode);

    let (_, icons) = load_from("icons = \"unicode\"\n", &nerd, None);
    assert_eq!(icons, IconSet::Unicode);

    // Auto follows the probe in both directions.
    let (_, icons) = load_from("", &nerd, None);
    assert_eq!(icons, IconSet::Nerd);
    let (_, icons) = load_from("", &caps(false), None);
    assert_eq!(icons, IconSet::Unicode);

    // toml forces Nerd even when no font was found.
    let (_, icons) = load_from("icons = \"nerd\"\n", &caps(false), None);
    assert_eq!(icons, IconSet::Nerd);
}

#[test]
fn auto_picks_emoji_when_nerd_is_missing_but_emoji_render_wide() {
    let emoji_host = TermCaps {
        wide_emoji: true,
        ..caps(false)
    };
    let (theme, icons) = load_from("", &emoji_host, None);
    assert_eq!(icons, IconSet::Emoji);
    assert_eq!(
        theme.chips,
        ChipStyle::Square,
        "no Powerline caps without a Nerd Font"
    );
    assert!(!theme.nerd_glyphs);

    // Nerd still wins when both are possible.
    let both = TermCaps {
        wide_emoji: true,
        ..caps(true)
    };
    assert_eq!(load_from("", &both, None).1, IconSet::Nerd);

    // Explicit choices override in either direction.
    assert_eq!(
        load_from("icons = \"unicode\"\n", &emoji_host, None).1,
        IconSet::Unicode
    );
    assert_eq!(load_from("", &caps(false), Some("emoji")).1, IconSet::Emoji);
    assert_eq!(load_from("", &both, Some("EMOJI")).1, IconSet::Emoji);
}

#[test]
fn cyberpunk_preset_applies_and_allows_overrides() {
    let caps = caps(false);
    let (theme, _) = load_from("preset = \"cyberpunk2077\"\n", &caps, None);
    assert_eq!(theme.accent, Theme::cyberpunk2077().accent);
    assert_eq!(theme.error, Theme::cyberpunk2077().error);
    assert_ne!(theme.bg, Theme::mocha().bg);

    let (theme, _) = load_from(
        "preset = \"cyberpunk2077\"\naccent = \"#ffffff\"\n",
        &caps,
        None,
    );
    assert_eq!(theme.accent, Color::Rgb(255, 255, 255));
    assert_eq!(theme.bg, Theme::cyberpunk2077().bg);
}

#[test]
fn preset_cycle_wraps_and_ids_roundtrip() {
    let mut p = ThemePreset::ALL[0];
    for _ in 0..ThemePreset::ALL.len() {
        p = p.next();
    }
    assert_eq!(p, ThemePreset::ALL[0], "full cycle returns to the start");

    for preset in ThemePreset::ALL {
        assert_eq!(ThemePreset::parse(preset.id()), Some(*preset), "{preset:?}");
        assert_eq!(
            ThemePreset::parse(&preset.id().to_uppercase()),
            Some(*preset)
        );
        assert!(!preset.label().is_empty());
    }
    assert_eq!(ThemePreset::parse("bogus"), None);
}

#[test]
fn chip_style_follows_icon_set_unless_toml_pins_it() {
    let caps = caps(true);
    // Auto: Nerd icons -> outline, Unicode -> square.
    let (theme, icons) = load_from("", &caps, None);
    assert_eq!(icons, IconSet::Nerd);
    assert_eq!(theme.chips, ChipStyle::Outline);
    let (theme, _) = load_from("", &caps, Some("unicode"));
    assert_eq!(theme.chips, ChipStyle::Square);

    // toml pins it regardless of icons; unknown values fall back to auto.
    let (theme, _) = load_from("chips = \"square\"\n", &caps, None);
    assert_eq!(theme.chips, ChipStyle::Square);
    let (theme, _) = load_from("chips = \"rounded\"\n", &caps, Some("unicode"));
    assert_eq!(theme.chips, ChipStyle::Rounded);
    let (theme, _) = load_from("chips = \"Outline\"\n", &caps, Some("unicode"));
    assert_eq!(theme.chips, ChipStyle::Outline);
    let (theme, _) = load_from("chips = \"blob\"\n", &caps, None);
    assert_eq!(theme.chips, ChipStyle::Outline);

    // Every preset keeps the style when cycling.
    let mut loader = Loader::from_toml("", caps);
    loader.set_icon_set(IconSet::Nerd);
    for p in ThemePreset::ALL {
        assert_eq!(loader.theme_for(*p).chips, ChipStyle::Outline, "{p:?}");
    }
}

#[test]
fn every_preset_is_readable() {
    for preset in ThemePreset::ALL {
        let t = preset.theme();
        assert_ne!(t.bg, t.text, "{preset:?}: text must contrast bg");
        assert_ne!(t.surface, t.text, "{preset:?}: text must contrast surface");
        assert_ne!(t.key_bg, t.key_fg, "{preset:?}: chip must be legible");
    }
}

#[test]
fn loader_applies_overrides_on_any_preset_and_resolves_precedence() {
    let caps = caps(false);
    let loader = Loader::from_toml("preset = \"nord\"\naccent = \"#123456\"\n", caps.clone());
    let t = loader.theme_for(ThemePreset::Dracula);
    assert_eq!(t.accent, Color::Rgb(0x12, 0x34, 0x56));
    assert_eq!(t.bg, Theme::dracula().bg);

    // Persisted state beats toml, toml beats default.
    assert_eq!(
        loader.resolve_preset(Some("tokyo-night")),
        (ThemePreset::TokyoNight, PresetSource::State)
    );
    assert_eq!(
        loader.resolve_preset(Some("junk")),
        (ThemePreset::Nord, PresetSource::Toml)
    );
    let plain = Loader::from_toml("", caps);
    assert_eq!(
        plain.resolve_preset(None),
        (ThemePreset::Mocha, PresetSource::Default)
    );
}

#[path = "palette_goldens.rs"]
mod goldens;
#[test]
fn all_168_preset_rgb_values_match_frozen_source_capture() {
    for (index, preset) in ThemePreset::ALL.iter().enumerate() {
        assert_eq!(
            preset.theme().all_colors(),
            goldens::PRESETS[index],
            "{}",
            preset.id()
        );
    }
}
#[test]
fn named_indexed_hex_and_unicode_color_parser_preserves_pinned_oracle_rules() {
    use std::str::FromStr;
    for (input, expected) in [
        ("bright-red", Color::LightRed),
        ("light black", Color::DarkGray),
        ("silver", Color::Gray),
        ("light_gray", Color::White),
        ("#Aa01fF", Color::Rgb(170, 1, 255)),
        ("255", Color::Indexed(255)),
        ("reset", Color::Reset),
    ] {
        assert_eq!(Color::from_str(input).unwrap(), expected);
    }
    for input in [
        "#fff",
        "256",
        "#ééé",
        "#xz0011",
        "rgb(1,2,3)",
        "not-a-color",
    ] {
        assert!(Color::from_str(input).is_err(), "{input}");
    }
}
#[test]
fn all_28_toml_color_slots_apply_and_unknown_fields_do_not_disable_valid_values() {
    let body="unknown = true\n".to_owned()+"bg = \"#123456\"\nsurface = \"#123456\"\nsurface_alt = \"#123456\"\nborder = \"#123456\"\nborder_active = \"#123456\"\ntext = \"#123456\"\ntext_muted = \"#123456\"\naccent = \"#123456\"\nkey_fg = \"#123456\"\nkey_bg = \"#123456\"\nsuccess = \"#123456\"\nwarning = \"#123456\"\nerror = \"#123456\"\ninfo = \"#123456\"\ncursor_bg = \"#123456\"\ncursor_fg = \"#123456\"\nselection = \"#123456\"\ndir = \"#123456\"\nhidden = \"#123456\"\nshadow = \"#123456\"\n";
    let body=body+"[files]\n"+"image = \"#123456\"\nvideo = \"#123456\"\naudio = \"#123456\"\narchive = \"#123456\"\ncode = \"#123456\"\ndocument = \"#123456\"\nexecutable = \"#123456\"\ndata = \"#123456\"\n";
    let loader = Loader::from_toml(&body, ThemeCapabilities::default());
    assert_eq!(
        loader.theme_for(ThemePreset::Mocha).all_colors(),
        [Color::Rgb(18, 52, 86); 28]
    );
}
#[test]
fn native_rgba_tokens_cover_named_cube_gray_and_explicit_reset() {
    let fallback = Rgba {
        r: 1,
        g: 2,
        b: 3,
        a: 4,
    };
    assert_eq!(Color::Reset.rgba(fallback), fallback);
    assert_eq!(
        Color::Rgb(10, 20, 30).rgba(fallback),
        Rgba {
            r: 10,
            g: 20,
            b: 30,
            a: 255
        }
    );
    assert_eq!(
        Color::Indexed(196).rgba(fallback),
        Color::LightRed.rgba(fallback)
    );
    assert_eq!(
        Color::Indexed(232).rgba(fallback),
        Rgba {
            r: 8,
            g: 8,
            b: 8,
            a: 255
        }
    );
    assert_eq!(
        Color::Indexed(255).rgba(fallback),
        Rgba {
            r: 238,
            g: 238,
            b: 238,
            a: 255
        }
    );
}
#[test]
fn caller_owned_theme_file_uses_toml_fallback_without_changing_config() {
    let path = std::env::temp_dir().join(format!("ira-theme-{}.toml", std::process::id()));
    std::fs::write(&path, "preset=\"dracula\"\naccent=\"#010203\"\n").unwrap();
    let loader = Loader::from_path(&path, ThemeCapabilities::default());
    assert_eq!(
        loader.resolve_preset(None),
        (ThemePreset::Dracula, PresetSource::Toml)
    );
    assert_eq!(
        loader.theme_for(ThemePreset::Dracula).accent,
        Color::Rgb(1, 2, 3)
    );
    std::fs::remove_file(path).unwrap();
}
