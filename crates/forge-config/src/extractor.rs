use forge_core::bindings::{Action, KeyStroke};
use forge_core::color::Color;
use forge_core::config_registry::{
    BlurMethod, BrailleStyle, ConfirmCloseBackgroundMode, CursorStyle, ForgeConfig, LigatureConfig,
    LigatureMode, PaddingBalance, PaneAnimationMode, PaneManagerMode, StatusbarItem,
    StatusbarPlacement, StatusbarPosition, StatusbarStyle, TabsConfig,
};
use toml::{map::Map, Value};

fn table<'a>(root: &'a Value, key: &str) -> Option<&'a Map<String, Value>> {
    root.get(key).and_then(Value::as_table)
}

fn table_alias<'a>(root: &'a Value, keys: &[&str]) -> Option<&'a Map<String, Value>> {
    keys.iter().find_map(|key| table(root, key))
}

fn string(t: &Map<String, Value>, key: &str) -> Option<String> {
    t.get(key).and_then(Value::as_str).map(str::to_string)
}

fn bool_value(t: &Map<String, Value>, key: &str) -> Option<bool> {
    t.get(key).and_then(Value::as_bool)
}

fn f32_value(t: &Map<String, Value>, key: &str) -> Option<f32> {
    t.get(key).and_then(|value| match value {
        Value::Integer(i) => Some(*i as f32),
        Value::Float(f) => Some(*f as f32),
        _ => None,
    })
}

fn u32_value(t: &Map<String, Value>, key: &str) -> Option<u32> {
    t.get(key).and_then(|value| match value {
        Value::Integer(i) if *i >= 0 => Some(*i as u32),
        Value::Float(f) if *f >= 0.0 => Some(*f as u32),
        _ => None,
    })
}

fn usize_value(t: &Map<String, Value>, key: &str) -> Option<usize> {
    t.get(key).and_then(|value| match value {
        Value::Integer(i) if *i >= 0 => Some(*i as usize),
        Value::Float(f) if *f >= 0.0 => Some(*f as usize),
        _ => None,
    })
}

fn string_array(t: &Map<String, Value>, key: &str) -> Option<Vec<String>> {
    t.get(key).and_then(Value::as_array).map(|values| {
        values
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect()
    })
}

fn parse_hex_color(hex: &str) -> Option<Color> {
    if hex.eq_ignore_ascii_case("transparent") {
        return Some(Color::TRANSPARENT);
    }
    let hex = hex.trim_start_matches('#');
    if hex.len() == 6 || hex.len() == 8 {
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        let a = if hex.len() == 8 {
            u8::from_str_radix(&hex[6..8], 16).ok()?
        } else {
            255
        };
        Some(Color { r, g, b, a })
    } else {
        None
    }
}

fn color(t: &Map<String, Value>, key: &str) -> Option<Color> {
    string(t, key).and_then(|s| {
        let parsed = parse_hex_color(&s);
        if parsed.is_none() {
            tracing::warn!("Invalid color for '{}': '{}'", key, s);
        }
        parsed
    })
}

fn parse_padding(value: &Value, target: &mut forge_core::config_registry::PaddingConfig) {
    match value {
        Value::Integer(p) if *p >= 0 => {
            let p = *p as u32;
            target.top = p;
            target.bottom = p;
            target.left = p;
            target.right = p;
        }
        Value::Float(p) if *p >= 0.0 => {
            let p = *p as u32;
            target.top = p;
            target.bottom = p;
            target.left = p;
            target.right = p;
        }
        Value::Table(pad) => {
            if let Some(p) = u32_value(pad, "x_axis").or_else(|| u32_value(pad, "x")) {
                target.left = p;
                target.right = p;
            }
            if let Some(p) = u32_value(pad, "y_axis").or_else(|| u32_value(pad, "y")) {
                target.top = p;
                target.bottom = p;
            }
            if let Some(p) = u32_value(pad, "top") {
                target.top = p;
            }
            if let Some(p) = u32_value(pad, "bottom") {
                target.bottom = p;
            }
            if let Some(p) = u32_value(pad, "left") {
                target.left = p;
            }
            if let Some(p) = u32_value(pad, "right") {
                target.right = p;
            }
        }
        _ => {}
    }
}

fn parse_ligature_mode(value: &str) -> Option<LigatureMode> {
    match value.to_ascii_lowercase().replace('_', "-").as_str() {
        "never" | "off" | "disabled" => Some(LigatureMode::Never),
        "always" | "on" | "enabled" => Some(LigatureMode::Always),
        "cursor-aware" | "cursor" => Some(LigatureMode::CursorAware),
        _ => None,
    }
}

fn parse_action(value: &str) -> Option<Action> {
    match value.to_ascii_lowercase().replace('-', "_").as_str() {
        "copy" => Some(Action::Copy),
        "paste" => Some(Action::Paste),
        "toggle_fullscreen" | "togglefullscreen" => Some(Action::ToggleFullscreen),
        "zoom_in" | "zoomin" => Some(Action::ZoomIn),
        "zoom_out" | "zoomout" => Some(Action::ZoomOut),
        "zoom_reset" | "zoomreset" => Some(Action::ZoomReset),
        "split_vertical" | "splitvertical" => Some(Action::SplitVertical),
        "split_horizontal" | "splithorizontal" => Some(Action::SplitHorizontal),
        "toggle_pane_zoom" | "togglepanezoom" => Some(Action::TogglePaneZoom),
        "toggle_sidebar" | "togglesidebar" => Some(Action::ToggleSidebar),
        "close_pane" | "closepane" => Some(Action::ClosePane),
        "focus_pane_left" | "focuspaneleft" => Some(Action::FocusPaneLeft),
        "focus_pane_right" | "focuspaneright" => Some(Action::FocusPaneRight),
        "focus_pane_up" | "focuspaneup" => Some(Action::FocusPaneUp),
        "focus_pane_down" | "focuspanedown" => Some(Action::FocusPaneDown),
        "new_tab" | "newtab" => Some(Action::NewTab),
        "close_tab" | "closetab" => Some(Action::CloseTab),
        "next_tab" | "nexttab" => Some(Action::NextTab),
        "previous_tab" | "previoustab" => Some(Action::PreviousTab),
        "switch_tab_1" | "switchtab1" => Some(Action::SwitchTab1),
        "switch_tab_2" | "switchtab2" => Some(Action::SwitchTab2),
        "switch_tab_3" | "switchtab3" => Some(Action::SwitchTab3),
        "switch_tab_4" | "switchtab4" => Some(Action::SwitchTab4),
        "switch_tab_5" | "switchtab5" => Some(Action::SwitchTab5),
        "switch_tab_6" | "switchtab6" => Some(Action::SwitchTab6),
        "switch_tab_7" | "switchtab7" => Some(Action::SwitchTab7),
        "switch_tab_8" | "switchtab8" => Some(Action::SwitchTab8),
        "switch_tab_9" | "switchtab9" => Some(Action::SwitchTab9),
        "move_tab_left" | "movetableft" => Some(Action::MoveTabLeft),
        "move_tab_right" | "movetabright" => Some(Action::MoveTabRight),
        _ => None,
    }
}

fn parse_ligatures(font_t: &Map<String, Value>, config: &mut ForgeConfig) {
    match font_t.get("ligatures") {
        Some(Value::Boolean(enabled)) => {
            config.font.ligatures = LigatureConfig::with_enabled(*enabled);
        }
        Some(Value::Table(lig_t)) => {
            let mut ligatures = config.font.ligatures.clone();
            if let Some(enabled) = bool_value(lig_t, "enabled") {
                ligatures.enabled = enabled;
            }
            if let Some(mode) = string(lig_t, "mode").and_then(|mode| parse_ligature_mode(&mode)) {
                ligatures.mode = mode;
            }
            if let Some(features) = string_array(lig_t, "features") {
                ligatures.features = features;
            }
            if let Some(max_token_len) = usize_value(lig_t, "max_token_len") {
                ligatures.max_token_len = max_token_len;
            }
            if let Some(cache_entries) = usize_value(lig_t, "cache_entries") {
                ligatures.cache_entries = cache_entries;
            }
            ligatures.normalize();
            config.font.ligatures = ligatures;
        }
        _ => {}
    }
}

fn parse_ansi_colors(t: &Map<String, Value>, config: &mut ForgeConfig) {
    let names = [
        ("black", 0),
        ("red", 1),
        ("green", 2),
        ("yellow", 3),
        ("blue", 4),
        ("magenta", 5),
        ("cyan", 6),
        ("white", 7),
        ("bright_black", 8),
        ("bright_red", 9),
        ("bright_green", 10),
        ("bright_yellow", 11),
        ("bright_blue", 12),
        ("bright_magenta", 13),
        ("bright_cyan", 14),
        ("bright_white", 15),
    ];

    for (name, idx) in names {
        if let Some(c) = color(t, name) {
            config.theme.ansi_colors[idx] = c;
        }
        let color_name = format!("color{}", idx);
        if let Some(c) = color(t, &color_name) {
            config.theme.ansi_colors[idx] = c;
        }
    }

    if let Some(array) = t.get("palette").and_then(Value::as_array) {
        for (idx, value) in array.iter().take(16).enumerate() {
            if let Some(c) = value.as_str().and_then(parse_hex_color) {
                config.theme.ansi_colors[idx] = c;
            }
        }
    }
}

fn parse_theme(root: &Value, config: &mut ForgeConfig) {
    if let Some(theme_t) = table_alias(root, &["theme", "colors"]) {
        if let Some(c) = color(theme_t, "background") {
            config.theme.background = c;
        }
        if let Some(c) = color(theme_t, "foreground") {
            config.theme.foreground = c;
        }
        if let Some(c) = color(theme_t, "cursor_color").or_else(|| color(theme_t, "cursor")) {
            config.theme.cursor_color = c;
        }
        if let Some(c) = color(theme_t, "selection_bg").or_else(|| color(theme_t, "selection")) {
            config.theme.selection_bg = c;
        }
        if let Some(c) = color(theme_t, "pane_outline_active") {
            config.theme.pane_outline_active = c;
        }
        if let Some(c) = color(theme_t, "pane_outline_inactive") {
            config.theme.pane_outline_inactive = c;
        }
        if let Some(ansi_t) = theme_t.get("ansi").and_then(Value::as_table) {
            parse_ansi_colors(ansi_t, config);
        }
        if let Some(normal_t) = theme_t.get("normal").and_then(Value::as_table) {
            parse_ansi_colors(normal_t, config);
        }
        if let Some(bright_t) = theme_t.get("bright").and_then(Value::as_table) {
            let map = [
                ("black", 8),
                ("red", 9),
                ("green", 10),
                ("yellow", 11),
                ("blue", 12),
                ("magenta", 13),
                ("cyan", 14),
                ("white", 15),
            ];
            for (name, idx) in map {
                if let Some(c) = color(bright_t, name) {
                    config.theme.ansi_colors[idx] = c;
                }
            }
        }
    }
}

fn parse_statusbar_style(t: &Map<String, Value>) -> StatusbarStyle {
    StatusbarStyle {
        fg: string(t, "fg"),
        bg: string(t, "bg"),
    }
}

fn parse_statusbar_item(value: &Value) -> Option<StatusbarItem> {
    match value {
        Value::String(s) => Some(StatusbarItem::String(s.clone())),
        Value::Table(item) => {
            if let Some(tabs_t) = item.get("tabs").and_then(Value::as_table) {
                let format = string(tabs_t, "format").unwrap_or_else(|| " {index} ".to_string());
                let zoom_indicator =
                    string(tabs_t, "zoom_indicator").unwrap_or_else(|| "()".to_string());
                let left_edge = string(tabs_t, "left_edge").unwrap_or_default();
                let right_edge = string(tabs_t, "right_edge").unwrap_or_default();
                let active = tabs_t
                    .get("active")
                    .and_then(Value::as_table)
                    .map(parse_statusbar_style);
                let inactive = tabs_t
                    .get("inactive")
                    .and_then(Value::as_table)
                    .map(parse_statusbar_style);
                return Some(StatusbarItem::Tabs {
                    tabs: TabsConfig {
                        format,
                        zoom_indicator,
                        left_edge,
                        right_edge,
                        active,
                        inactive,
                    },
                });
            }

            let item = item.get("table").and_then(Value::as_table).unwrap_or(item);
            string(item, "text").map(|text| StatusbarItem::Table {
                text,
                fg: string(item, "fg"),
                bg: string(item, "bg"),
                action: string(item, "action"),
                bold: bool_value(item, "bold"),
            })
        }
        _ => None,
    }
}

fn parse_statusbar_items(t: &Map<String, Value>, key: &str) -> Option<Vec<StatusbarItem>> {
    t.get(key).and_then(Value::as_array).map(|items| {
        items
            .iter()
            .filter_map(parse_statusbar_item)
            .collect::<Vec<_>>()
    })
}

fn apply_action_binding(config: &mut ForgeConfig, action_name: &str, key: &str) {
    let Some(action) = parse_action(action_name) else {
        tracing::warn!("Unknown keybinding action '{}'", action_name);
        return;
    };
    let Some(keystroke) = KeyStroke::parse(key) else {
        tracing::warn!("Invalid keybinding '{}' for action '{}'", key, action_name);
        return;
    };
    config.keybindings.insert(keystroke, action);
}

fn apply_legacy_binding(config: &mut ForgeConfig, key: &str, action_name: &str) {
    let Some(keystroke) = KeyStroke::parse(key) else {
        tracing::warn!("Invalid keybinding '{}'", key);
        return;
    };
    let Some(action) = parse_action(action_name) else {
        tracing::warn!("Unknown action '{}' for keybinding '{}'", action_name, key);
        return;
    };
    config.keybindings.insert(keystroke, action);
}

pub fn extract_config(root: &Value, config: &mut ForgeConfig) {
    if let Some(font_t) = table(root, "font") {
        if let Some(family) = string(font_t, "family") {
            config.font.family = family;
        }
        if let Some(size) = f32_value(font_t, "size") {
            config.font.size = size;
        }
        if let Some(family) = string(font_t, "bold_family") {
            config.font.bold_family = Some(family);
        }
        if let Some(family) = string(font_t, "italic_family") {
            config.font.italic_family = Some(family);
        }
        if let Some(nerd_fonts) = bool_value(font_t, "nerd_fonts") {
            config.font.nerd_fonts = nerd_fonts;
        }
        parse_ligatures(font_t, config);
    }

    if let Some(win_t) = table(root, "window") {
        if let Some(width) = u32_value(win_t, "width") {
            config.window.width = width;
        }
        if let Some(height) = u32_value(win_t, "height") {
            config.window.height = height;
        }
        if let Some(opacity) = f32_value(win_t, "opacity") {
            config.window.opacity = opacity;
        }
        if let Some(title) = string(win_t, "title") {
            config.window.title = title;
        }
        if let Some(decorations) = bool_value(win_t, "decorations") {
            config.window.decorations = decorations;
        }
        if let Some(gap) = u32_value(win_t, "gap") {
            config.window.gap = gap;
        }
        if let Some(width) = f32_value(win_t, "pane_outline_width") {
            config.window.pane_outline_width = width;
        }
        if let Some(balance) = string(win_t, "padding_balance") {
            match balance.to_ascii_lowercase().as_str() {
                "fill" => config.window.padding_balance = PaddingBalance::Fill,
                "center" => config.window.padding_balance = PaddingBalance::Center,
                _ => tracing::warn!("Unknown padding_balance '{}'", balance),
            }
        }
        if let Some(padding) = win_t.get("padding") {
            parse_padding(padding, &mut config.window.padding);
        }
        if let Some(padding) = win_t.get("pane_padding") {
            parse_padding(padding, &mut config.window.pane_padding);
        }
    }

    if let Some(blur_t) = table(root, "blur") {
        if let Some(enabled) = bool_value(blur_t, "enabled") {
            config.blur.enabled = enabled;
        }
        if let Some(radius) = u32_value(blur_t, "radius") {
            config.blur.radius = radius;
        }
        if let Some(method) = string(blur_t, "method") {
            match method.to_ascii_lowercase().as_str() {
                "auto" => config.blur.method = BlurMethod::Auto,
                "kde" | "kwin" => config.blur.method = BlurMethod::Kde,
                "external" | "compositor" => config.blur.method = BlurMethod::External,
                "off" | "disabled" | "none" => config.blur.method = BlurMethod::Off,
                _ => tracing::warn!("Unknown blur method '{}'", method),
            }
        }
    }

    parse_theme(root, config);

    if let Some(shell_t) = table(root, "shell") {
        if let Some(program) = string(shell_t, "program") {
            config.shell.program = program;
        }
        if let Some(args) = string_array(shell_t, "args") {
            config.shell.args = args;
        }
        if let Some(env_t) = shell_t.get("env").and_then(Value::as_table) {
            config.shell.env = env_t
                .iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|value| (key.clone(), value.to_string()))
                })
                .collect();
        }
    }

    if let Some(cursor_t) = table(root, "cursor") {
        if let Some(style) = string(cursor_t, "style") {
            match style.to_ascii_lowercase().as_str() {
                "block" => config.cursor.style = CursorStyle::Block,
                "underline" => config.cursor.style = CursorStyle::Underline,
                "beam" => config.cursor.style = CursorStyle::Beam,
                _ => tracing::warn!("Unknown cursor style '{}'", style),
            }
        }
        if let Some(blink) = bool_value(cursor_t, "blink") {
            config.cursor.blink = blink;
        }
        if let Some(rate) = u32_value(cursor_t, "blink_rate_ms") {
            config.cursor.blink_rate_ms = rate;
        }
    }

    if let Some(scrollback_t) = table(root, "scrollback") {
        if let Some(lines) = usize_value(scrollback_t, "lines") {
            config.scrollback.lines = lines;
        }
        if let Some(smooth) = bool_value(scrollback_t, "smooth_scroll") {
            config.scrollback.smooth_scroll = smooth;
        }
        if let Some(multiplier) = f32_value(scrollback_t, "scroll_multiplier") {
            config.scrollback.scroll_multiplier = multiplier;
        }
    }

    if let Some(behavior_t) = table(root, "behavior") {
        if let Some(copy_on_select) = bool_value(behavior_t, "copy_on_select") {
            config.behavior.copy_on_select = copy_on_select;
        }
        if let Some(disable) = bool_value(behavior_t, "disable_default_keybindings") {
            config.behavior.disable_default_keybindings = disable;
        }
        if let Some(hide) = bool_value(behavior_t, "hide_mouse_when_typing") {
            config.behavior.hide_mouse_when_typing = hide;
        }
    }

    if let Some(panes_t) = table(root, "panes") {
        if let Some(mode) = string(panes_t, "mode") {
            match mode.to_ascii_lowercase().replace('-', "_").as_str() {
                "tiling" => config.panes.mode = PaneManagerMode::Tiling,
                "scrolling" => config.panes.mode = PaneManagerMode::Scrolling,
                _ => tracing::warn!(
                    "Unknown panes.mode '{}'; falling back to tiling pane manager",
                    mode
                ),
            }
        }
    }

    if let Some(render_t) = table_alias(root, &["render", "renderer"]) {
        if let Some(style) = string(render_t, "braille_style") {
            match style.to_ascii_lowercase().as_str() {
                "solid" => config.render.braille_style = BrailleStyle::Solid,
                "dots" => config.render.braille_style = BrailleStyle::Dots,
                _ => tracing::warn!("Unknown braille_style '{}'", style),
            }
        }
        if let Some(transparent) = bool_value(render_t, "context_menu_transparent") {
            config.render.context_menu_transparent = transparent;
        }
        if let Some(mode) = string(render_t, "pane_animation") {
            match mode.to_ascii_lowercase().as_str() {
                "expand" => config.render.pane_animation = PaneAnimationMode::Expand,
                "fade" => config.render.pane_animation = PaneAnimationMode::Fade,
                "none" | "off" | "disabled" => {
                    config.render.pane_animation = PaneAnimationMode::None
                }
                _ => tracing::warn!("Unknown pane_animation '{}'. Expected expand, fade, or none.", mode),
            }
        }
        if let Some(dur) = u32_value(render_t, "pane_animation_duration_ms") {
            config.render.pane_animation_duration_ms = dur.clamp(50, 2000);
        }
    }

    if let Some(confirm_t) = table(root, "confirm_close") {
        if let Some(mode) = string(confirm_t, "background_mode") {
            match mode.to_ascii_lowercase().replace('-', "_").as_str() {
                "opaque_panel" | "opaque" | "panel" => {
                    config.confirm_close.background_mode = ConfirmCloseBackgroundMode::OpaquePanel
                }
                "blank_screen" | "blank" | "empty" => {
                    config.confirm_close.background_mode = ConfirmCloseBackgroundMode::BlankScreen
                }
                _ => tracing::warn!("Unknown confirm_close.background_mode '{}'", mode),
            }
        }
        if let Some(color) = color(confirm_t, "panel_color") {
            config.confirm_close.panel_color = color;
        }
        if let Some(color) = color(confirm_t, "selected_color") {
            config.confirm_close.selected_color = color;
        }
    }

    if config.behavior.disable_default_keybindings {
        config.keybindings.clear();
    }

    if let Some(status_t) = table_alias(root, &["statusbar", "status_bar"]) {
        if let Some(enabled) = bool_value(status_t, "enabled") {
            config.statusbar.enabled = enabled;
        }
        if let Some(position) = string(status_t, "position") {
            match position.to_ascii_lowercase().as_str() {
                "top" => config.statusbar.position = StatusbarPosition::Top,
                "bottom" => config.statusbar.position = StatusbarPosition::Bottom,
                _ => tracing::warn!("Unknown statusbar.position '{}'", position),
            }
        }
        if let Some(placement) = string(status_t, "placement") {
            match placement.to_ascii_lowercase().as_str() {
                "absolute" => config.statusbar.placement = StatusbarPlacement::Absolute,
                "inside" => config.statusbar.placement = StatusbarPlacement::Inside,
                _ => tracing::warn!("Unknown statusbar.placement '{}'", placement),
            }
        }
        if let Some(bg) = string(status_t, "bg_color").or_else(|| string(status_t, "background")) {
            config.statusbar.bg_color = bg;
        }
        if let Some(fg) = string(status_t, "fg_color").or_else(|| string(status_t, "foreground")) {
            config.statusbar.fg_color = fg;
        }
        if let Some(items) = parse_statusbar_items(status_t, "left") {
            config.statusbar.left = items;
        }
        if let Some(items) = parse_statusbar_items(status_t, "center") {
            config.statusbar.center = items;
        }
        if let Some(items) = parse_statusbar_items(status_t, "right") {
            config.statusbar.right = items;
        }
    }

    if let Some(bindings_t) = table_alias(root, &["keybindings", "key_bindings"]) {
        for (action_name, key_value) in bindings_t {
            if let Some(key) = key_value.as_str() {
                apply_action_binding(config, action_name, key);
            }
        }
    }

    if let Some(bindings_t) = table(root, "bindings") {
        for (key, action_value) in bindings_t {
            if let Some(action) = action_value.as_str() {
                apply_legacy_binding(config, key, action);
            }
        }
    }
}

pub fn parse_config_str(source: &str) -> Result<ForgeConfig, toml::de::Error> {
    let root = source.parse::<Value>()?;
    let mut config = ForgeConfig::default();
    extract_config(&root, &mut config);
    config.validate();
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_toml_config() {
        let source = r##"
            [font]
            family = "Fira Code"
            size = 16.5
            nerd_fonts = false

            [font.ligatures]
            enabled = true
            mode = "cursor-aware"
            features = ["liga", "clig"]
            max_token_len = 999

            [window]
            width = 1024
            height = 768
            opacity = 0.85
            title = "My Custom Terminal"
            padding_balance = "fill"
            gap = 8
            pane_outline_width = 2.0

            [window.padding]
            x = 10
            y = 6

            [blur]
            enabled = true
            method = "kde"
            radius = 24

            [colors]
            background = "#1a1b26"
            foreground = "#c0caf5aa"
            pane_outline_active = "#89b4fa"

            [colors.normal]
            black = "#111111"
            red = "#222222"

            [shell]
            program = "/bin/zsh"
            args = ["-l", "-i"]

            [shell.env]
            FOO = "bar"

            [cursor]
            style = "beam"
            blink = false

            [scrollback]
            lines = 5000
            scroll_multiplier = 1.5

            [panes]
            mode = "scrolling"

            [confirm_close]
            background_mode = "blank_screen"
            panel_color = "#0b1020"
            selected_color = "#ff8800"

            [status_bar]
            enabled = true
            position = "top"
            placement = "absolute"

            [[status_bar.left]]
            [status_bar.left.tabs]
            format = " {index}{zoom} "
            zoom_indicator = "(Z)"

            [[status_bar.left]]
            text = " + "
            action = "NewTab"

            [keybindings]
            toggle_sidebar = "ctrl+shift+b"
        "##;

        let config = parse_config_str(source).unwrap();

        assert_eq!(config.font.family, "Fira Code");
        assert_eq!(config.font.size, 16.5);
        assert!(config.font.ligatures.enabled);
        assert_eq!(
            config.font.ligatures.max_token_len,
            LigatureConfig::MAX_TOKEN_LEN
        );
        assert_eq!(config.window.width, 1024);
        assert_eq!(config.window.padding.left, 10);
        assert_eq!(config.window.padding.top, 6);
        assert_eq!(config.blur.method, BlurMethod::Kde);
        assert_eq!(config.theme.background.r, 26);
        assert_eq!(config.theme.foreground.a, 170);
        assert_eq!(config.theme.ansi_colors[0].r, 17);
        assert_eq!(config.theme.ansi_colors[1].g, 34);
        assert_eq!(config.shell.program, "/bin/zsh");
        assert_eq!(config.shell.args, vec!["-l".to_string(), "-i".to_string()]);
        assert_eq!(
            config.shell.env,
            vec![("FOO".to_string(), "bar".to_string())]
        );
        assert_eq!(config.cursor.style, CursorStyle::Beam);
        assert!(!config.cursor.blink);
        assert_eq!(config.scrollback.lines, 5000);
        assert_eq!(config.panes.mode, PaneManagerMode::Scrolling);
        assert_eq!(
            config.confirm_close.background_mode,
            ConfirmCloseBackgroundMode::BlankScreen
        );
        assert_eq!(config.confirm_close.selected_color.r, 255);

        let Some(StatusbarItem::Tabs { tabs }) = config.statusbar.left.first() else {
            panic!("expected tabs item");
        };
        assert_eq!(tabs.zoom_indicator, "(Z)");

        let key = KeyStroke::parse("ctrl+shift+b").unwrap();
        assert_eq!(config.keybindings.get(&key), Some(&Action::ToggleSidebar));
    }

    #[test]
    fn supports_legacy_key_to_action_bindings_table() {
        let config = parse_config_str(
            r#"
            [bindings]
            "ctrl+shift+t" = "new_tab"
            "#,
        )
        .unwrap();
        let key = KeyStroke::parse("ctrl+shift+t").unwrap();
        assert_eq!(config.keybindings.get(&key), Some(&Action::NewTab));
    }

    #[test]
    fn invalid_values_fall_back_or_validate() {
        let config = parse_config_str(
            r##"
            [window]
            width = 1
            opacity = 12.0

            [cursor]
            style = "giant"

            [confirm_close]
            panel_color = "bad"
            "##,
        )
        .unwrap();
        assert_eq!(config.window.width, 200);
        assert_eq!(config.window.opacity, 1.0);
        assert_eq!(config.cursor.style, CursorStyle::Block);
        assert_eq!(
            config.confirm_close.panel_color,
            forge_core::config_registry::ConfirmCloseConfig::default().panel_color
        );
    }

    #[test]
    fn panes_mode_defaults_to_tiling() {
        let config = parse_config_str("").unwrap();
        assert_eq!(config.panes.mode, PaneManagerMode::Tiling);
    }

    #[test]
    fn parses_explicit_tiling_panes_mode() {
        let config = parse_config_str(
            r#"
            [panes]
            mode = "tiling"
            "#,
        )
        .unwrap();
        assert_eq!(config.panes.mode, PaneManagerMode::Tiling);
    }

    #[test]
    fn parses_explicit_scrolling_panes_mode() {
        let config = parse_config_str(
            r#"
            [panes]
            mode = "scrolling"
            "#,
        )
        .unwrap();
        assert_eq!(config.panes.mode, PaneManagerMode::Scrolling);
    }

    #[test]
    fn invalid_panes_mode_falls_back_to_tiling() {
        let config = parse_config_str(
            r#"
            [panes]
            mode = "floating"
            "#,
        )
        .unwrap();
        assert_eq!(config.panes.mode, PaneManagerMode::Tiling);
    }
}
