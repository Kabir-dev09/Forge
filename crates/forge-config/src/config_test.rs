#[test]
fn test_config_colors_load() {
    let source = include_str!("default_config.toml");
    let config = crate::extractor::parse_config_str(source).unwrap();

    assert_eq!(config.theme.background.r, 26); // #1a = 26
    assert_eq!(config.theme.foreground.r, 192); // #c0 = 192
    assert_eq!(config.theme.ansi_colors[0].r, 65); // #41 = 65
    assert_eq!(config.theme.ansi_colors[1].g, 118); // #76 = 118
}
