#[test]
fn test_config_colors_load() {
    let source = include_str!("default_config.toml");
    let mut config: forge_core::config_registry::ForgeConfig = toml::from_str(source).unwrap();
    assert!(config.validate().is_empty());

    assert_eq!(config.theme.parsed_background.r, 26); // #1a = 26
    assert_eq!(config.theme.parsed_foreground.r, 192); // #c0 = 192
    assert_eq!(config.theme.parsed_ansi_colors[0].r, 65); // #41 = 65
    assert_eq!(config.theme.parsed_ansi_colors[1].g, 118); // #76 = 118
    assert_eq!(config.theme.parsed_ansi_colors[8].r, 65); // bright black
    assert_eq!(config.theme.parsed_ansi_colors[15].b, 245); // bright white
}

#[test]
fn test_all_sixteen_ansi_array_entries_load_in_order() {
    let source = r##"
        [theme]
        ansi_colors = [
            "#000000", "#010101", "#020202", "#030303",
            "#040404", "#050505", "#060606", "#070707",
            "#080808", "#090909", "#0a0a0a", "#0b0b0b",
            "#0c0c0c", "#0d0d0d", "#0e0e0e", "#0f0f0f",
        ]
    "##;
    let mut config: forge_core::config_registry::ForgeConfig = toml::from_str(source).unwrap();

    assert!(config.validate().is_empty());
    for (index, color) in config.theme.parsed_ansi_colors.iter().enumerate() {
        assert_eq!(
            (color.r, color.g, color.b),
            (index as u8, index as u8, index as u8)
        );
    }
}

#[test]
fn window_center_on_launch_defaults_off_and_parses_boolean() {
    let default_config: forge_core::config_registry::ForgeConfig =
        toml::from_str(include_str!("default_config.toml")).unwrap();
    assert!(!default_config.window.center_on_launch);

    let enabled: forge_core::config_registry::ForgeConfig =
        toml::from_str("[window]\ncenter_on_launch = true\n").unwrap();
    assert!(enabled.window.center_on_launch);
}

#[test]
fn shell_integration_is_enabled_by_default_and_can_be_disabled() {
    let default_config: forge_core::config_registry::ForgeConfig =
        toml::from_str(include_str!("default_config.toml")).unwrap();
    assert!(default_config.shell.integration_enabled);

    let disabled: forge_core::config_registry::ForgeConfig =
        toml::from_str("[shell]\nintegration_enabled = false\n").unwrap();
    assert!(!disabled.shell.integration_enabled);
}
