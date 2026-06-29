use forge_core::config_registry::ForgeConfig;
use mlua::Lua;

#[test]
fn test_config_colors_load() {
    let lua = Lua::new();
    let source = include_str!("default_config.lua");
    let table = lua.load(source).eval::<mlua::Table>().unwrap();
    let mut config = ForgeConfig::default();
    crate::extractor::extract_config(table, &mut config);

    assert_eq!(config.theme.background.r, 26); // #1a = 26
    assert_eq!(config.theme.foreground.r, 192); // #c0 = 192
    assert_eq!(config.theme.ansi_colors[0].r, 65); // #41 = 65
    assert_eq!(config.theme.ansi_colors[1].g, 118); // #76 = 118
}
