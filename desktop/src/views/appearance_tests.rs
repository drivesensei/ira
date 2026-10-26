use super::*;
use ira_core::theme::{Loader, ThemeCapabilities};

#[test]
fn desktop_appearance_maps_all_variants_and_preserves_default() {
    let mut palette = Palette::default();
    let configured = palette.configured;
    let light = Loader::from_toml(
        "desktop_appearance = \"system\"",
        ThemeCapabilities::default(),
    )
    .desktop_light_theme_for(ThemePreset::Mocha)
    .unwrap();
    for appearance in [
        WindowAppearance::Light,
        WindowAppearance::VibrantLight,
        WindowAppearance::Dark,
        WindowAppearance::VibrantDark,
    ] {
        palette.appearance = appearance;
        assert_eq!(palette.selected(), configured);
        palette.light = Some(light);
        assert_eq!(
            palette.selected(),
            match appearance {
                WindowAppearance::Light | WindowAppearance::VibrantLight => light,
                WindowAppearance::Dark | WindowAppearance::VibrantDark => configured,
            }
        );
        palette.light = None;
    }
}

#[test]
fn desktop_appearance_latest_publication_wins_without_an_os_event() {
    let loader = Loader::from_toml(
        "desktop_appearance = \"system\"",
        ThemeCapabilities::default(),
    );
    let mut palette = Palette {
        appearance: WindowAppearance::Light,
        ..Palette::default()
    };
    for preset in ThemePreset::ALL {
        palette.configured = loader.theme_for(*preset);
        palette.light = loader.desktop_light_theme_for(*preset);
        assert_eq!(palette.selected(), palette.light.unwrap());
        palette.appearance = WindowAppearance::Dark;
        assert_eq!(palette.selected(), palette.configured);
        palette.appearance = WindowAppearance::Light;
    }
}
