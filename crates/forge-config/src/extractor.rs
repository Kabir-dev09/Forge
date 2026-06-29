use forge_core::color::Color;
use forge_core::config_registry::{BlurMethod, CursorStyle, ForgeConfig};
use mlua::{Table, Value};

/// Helper to get a string from a table.
fn get_string(table: &Table, key: &str) -> Option<String> {
    match table.get::<_, Value>(key) {
        Ok(Value::String(s)) => s.to_str().ok().map(|s| s.to_string()),
        _ => None,
    }
}

/// Helper to get an f32. Handles both integer and float Lua types.
fn get_f32(table: &Table, key: &str) -> Option<f32> {
    match table.get::<_, Value>(key) {
        Ok(Value::Number(n)) => Some(n as f32),
        Ok(Value::Integer(i)) => Some(i as f32),
        _ => None,
    }
}

/// Helper to get a u32.
fn get_u32(table: &Table, key: &str) -> Option<u32> {
    match table.get::<_, Value>(key) {
        Ok(Value::Integer(i)) if i >= 0 => Some(i as u32),
        Ok(Value::Number(n)) if n >= 0.0 => Some(n as u32),
        _ => None,
    }
}

/// Helper to get a usize.
fn get_usize(table: &Table, key: &str) -> Option<usize> {
    match table.get::<_, Value>(key) {
        Ok(Value::Integer(i)) if i >= 0 => Some(i as usize),
        Ok(Value::Number(n)) if n >= 0.0 => Some(n as usize),
        _ => None,
    }
}

/// Helper to get a boolean.
fn get_bool(table: &Table, key: &str) -> Option<bool> {
    match table.get::<_, Value>(key) {
        Ok(Value::Boolean(b)) => Some(b),
        _ => None,
    }
}

/// Parses a hex color string (e.g., "#RRGGBB" or "#RRGGBBAA") into a Color.
fn parse_hex_color(hex: &str) -> Option<Color> {
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

fn get_color(table: &Table, key: &str) -> Option<Color> {
    get_string(table, key).and_then(|s| {
        let parsed = parse_hex_color(&s);
        if parsed.is_none() {
            tracing::warn!(
                "Failed to parse color '{}' for key '{}'. Expected hex format (e.g., #RRGGBB).",
                s,
                key
            );
        }
        parsed
    })
}

/// The main extraction function. Modifies `config` in place.
pub fn extract_config(root: Table, config: &mut ForgeConfig) {
    // Font
    if let Ok(Value::Table(font_t)) = root.get::<_, Value>("font") {
        if let Some(f) = get_string(&font_t, "family") {
            config.font.family = f;
        }
        if let Some(s) = get_f32(&font_t, "size") {
            config.font.size = s;
        }
        if let Some(f) = get_string(&font_t, "bold_family") {
            config.font.bold_family = Some(f);
        }
        if let Some(f) = get_string(&font_t, "italic_family") {
            config.font.italic_family = Some(f);
        }
        if let Some(l) = get_bool(&font_t, "ligatures") {
            config.font.ligatures = l;
        }
        if let Some(n) = get_bool(&font_t, "nerd_fonts") {
            config.font.nerd_fonts = n;
        }
    }

    // Window
    if let Ok(Value::Table(win_t)) = root.get::<_, Value>("window") {
        if let Some(w) = get_u32(&win_t, "width") {
            config.window.width = w;
        }
        if let Some(h) = get_u32(&win_t, "height") {
            config.window.height = h;
        }
        if let Some(o) = get_f32(&win_t, "opacity") {
            config.window.opacity = o;
        }

        match win_t.get::<_, Value>("padding") {
            Ok(Value::Integer(p)) if p >= 0 => {
                let p = p as u32;
                config.window.padding.top = p;
                config.window.padding.bottom = p;
                config.window.padding.left = p;
                config.window.padding.right = p;
            }
            Ok(Value::Number(p)) if p >= 0.0 => {
                let p = p as u32;
                config.window.padding.top = p;
                config.window.padding.bottom = p;
                config.window.padding.left = p;
                config.window.padding.right = p;
            }
            Ok(Value::Table(pad_t)) => {
                if let Some(p) = get_u32(&pad_t, "x_axis") {
                    config.window.padding.left = p;
                    config.window.padding.right = p;
                }
                if let Some(p) = get_u32(&pad_t, "y_axis") {
                    config.window.padding.top = p;
                    config.window.padding.bottom = p;
                }
                if let Some(p) = get_u32(&pad_t, "top") {
                    config.window.padding.top = p;
                }
                if let Some(p) = get_u32(&pad_t, "bottom") {
                    config.window.padding.bottom = p;
                }
                if let Some(p) = get_u32(&pad_t, "left") {
                    config.window.padding.left = p;
                }
                if let Some(p) = get_u32(&pad_t, "right") {
                    config.window.padding.right = p;
                }
            }
            _ => {}
        }

        if let Some(t) = get_string(&win_t, "title") {
            config.window.title = t;
        }
        if let Some(d) = get_bool(&win_t, "decorations") {
            config.window.decorations = d;
        }
        if let Some(s) = get_string(&win_t, "padding_balance") {
            match s.to_lowercase().as_str() {
                "fill" => {
                    config.window.padding_balance =
                        forge_core::config_registry::PaddingBalance::Fill
                }
                "center" => {
                    config.window.padding_balance =
                        forge_core::config_registry::PaddingBalance::Center
                }
                _ => {}
            }
        }
    }

    // Blur
    if let Ok(Value::Table(blur_t)) = root.get::<_, Value>("blur") {
        if let Some(enabled) = get_bool(&blur_t, "enabled") {
            config.blur.enabled = enabled;
        }
        if let Some(radius) = get_u32(&blur_t, "radius") {
            config.blur.radius = radius;
        }
        if let Some(method) = get_string(&blur_t, "method") {
            match method.to_lowercase().as_str() {
                "auto" => config.blur.method = BlurMethod::Auto,
                "kde" | "kwin" => config.blur.method = BlurMethod::Kde,
                "external" | "compositor" => config.blur.method = BlurMethod::External,
                "off" | "disabled" | "none" => config.blur.method = BlurMethod::Off,
                other => {
                    tracing::warn!(
                        "Unknown blur method '{}'. Expected auto, kde, external, or off.",
                        other
                    );
                }
            }
        }
    }

    // Theme
    if let Ok(Value::Table(theme_t)) = root.get::<_, Value>("theme") {
        if let Some(c) = get_color(&theme_t, "background") {
            config.theme.background = c;
        }
        if let Some(c) = get_color(&theme_t, "foreground") {
            config.theme.foreground = c;
        }
        if let Some(c) = get_color(&theme_t, "cursor_color") {
            config.theme.cursor_color = c;
        }
        if let Some(c) = get_color(&theme_t, "selection_bg") {
            config.theme.selection_bg = c;
        }

        if let Ok(Value::Table(ansi_t)) = theme_t.get::<_, Value>("ansi") {
            let color_names = [
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
            for (name, idx) in color_names {
                // Try named colors (e.g., "red")
                if let Some(c) = get_color(&ansi_t, name) {
                    config.theme.ansi_colors[idx] = c;
                }
                // Try numbered colors (e.g., "color1")
                let color_name = format!("color{}", idx);
                if let Some(c) = get_color(&ansi_t, &color_name) {
                    config.theme.ansi_colors[idx] = c;
                }
                // Support array fallback for backwards compatibility
                if let Ok(Value::String(s)) = ansi_t.get::<_, Value>(idx as i32 + 1) {
                    if let Ok(s_str) = s.to_str() {
                        if let Some(c) = parse_hex_color(s_str) {
                            config.theme.ansi_colors[idx] = c;
                        }
                    }
                }
            }
        }
    }

    // Shell
    if let Ok(Value::Table(shell_t)) = root.get::<_, Value>("shell") {
        if let Some(p) = get_string(&shell_t, "program") {
            config.shell.program = p;
        }

        // Extract args (Lua array to Rust Vec)
        if let Ok(Value::Table(args_t)) = shell_t.get::<_, Value>("args") {
            let mut args = Vec::new();
            for i in 1.. {
                match args_t.get::<_, Value>(i) {
                    Ok(Value::String(s)) => {
                        if let Ok(s_str) = s.to_str() {
                            args.push(s_str.to_string());
                        }
                    }
                    _ => break,
                }
            }
            if !args.is_empty() {
                config.shell.args = args;
            }
        }

        // Extract env (Lua table to Rust Vec of tuples)
        if let Ok(Value::Table(env_t)) = shell_t.get::<_, Value>("env") {
            let mut env = Vec::new();
            for (k, v) in env_t.pairs::<String, String>().flatten() {
                env.push((k, v));
            }
            if !env.is_empty() {
                config.shell.env = env;
            }
        }
    }

    // Cursor
    if let Ok(Value::Table(cursor_t)) = root.get::<_, Value>("cursor") {
        if let Some(s) = get_string(&cursor_t, "style") {
            match s.to_lowercase().as_str() {
                "block" => config.cursor.style = CursorStyle::Block,
                "underline" => config.cursor.style = CursorStyle::Underline,
                "beam" => config.cursor.style = CursorStyle::Beam,
                _ => {}
            }
        }
        if let Some(b) = get_bool(&cursor_t, "blink") {
            config.cursor.blink = b;
        }
        if let Some(r) = get_u32(&cursor_t, "blink_rate_ms") {
            config.cursor.blink_rate_ms = r;
        }
    }

    // Scrollback
    if let Ok(Value::Table(scroll_t)) = root.get::<_, Value>("scrollback") {
        if let Some(l) = get_usize(&scroll_t, "lines") {
            config.scrollback.lines = l;
        }
        if let Some(s) = get_bool(&scroll_t, "smooth_scroll") {
            config.scrollback.smooth_scroll = s;
        }
        if let Some(m) = get_f32(&scroll_t, "scroll_multiplier") {
            config.scrollback.scroll_multiplier = m;
        }
    }

    // Behavior
    if let Ok(Value::Table(behavior_t)) = root.get::<_, Value>("behavior") {
        if let Some(c) = get_bool(&behavior_t, "copy_on_select") {
            config.behavior.copy_on_select = c;
        }
        if let Some(d) = get_bool(&behavior_t, "disable_default_keybindings") {
            config.behavior.disable_default_keybindings = d;
        }
        if let Some(h) = get_bool(&behavior_t, "hide_mouse_when_typing") {
            config.behavior.hide_mouse_when_typing = h;
        }
    }

    // Render
    if let Ok(Value::Table(render_t)) = root.get::<_, Value>("render") {
        if let Some(s) = get_string(&render_t, "braille_style") {
            match s.to_lowercase().as_str() {
                "solid" => {
                    config.render.braille_style = forge_core::config_registry::BrailleStyle::Solid
                }
                "dots" => {
                    config.render.braille_style = forge_core::config_registry::BrailleStyle::Dots
                }
                _ => {}
            }
        }
    }

    if config.behavior.disable_default_keybindings {
        config.keybindings.clear();
    }

    // Keybindings
    if let Ok(Value::Table(bindings_t)) = root.get::<_, Value>("bindings") {
        for (key_str, action_str) in bindings_t.pairs::<String, String>().flatten() {
            if let Some(keystroke) = forge_core::bindings::KeyStroke::parse(&key_str) {
                let action = match action_str.to_lowercase().as_str() {
                    "copy" => Some(forge_core::bindings::Action::Copy),
                    "paste" => Some(forge_core::bindings::Action::Paste),
                    "toggle_fullscreen" | "togglefullscreen" => {
                        Some(forge_core::bindings::Action::ToggleFullscreen)
                    }
                    _ => {
                        tracing::warn!("Unknown action '{}' for keybind '{}'", action_str, key_str);
                        None
                    }
                };
                if let Some(a) = action {
                    config.keybindings.insert(keystroke, a);
                }
            } else {
                tracing::warn!("Failed to parse keybind '{}'", key_str);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::Lua;

    #[test]
    fn test_extract_config() {
        let lua = Lua::new();
        let source = r##"
            return {
                font = {
                    family = "Fira Code",
                    size = 16.5,
                    ligatures = false,
                    invalid_field = "ignore me",
                },
                window = {
                    width = 1024,
                    height = 768,
                    opacity = 0.85,
                    title = "My Custom Terminal",
                },
                blur = {
                    enabled = true,
                    method = "kde",
                    radius = 24,
                },
                theme = {
                    background = "#1a1b26",
                    foreground = "#c0caf5aa",
                    cursor_color = "red", -- invalid color, should be ignored
                    ansi = {
                        black = "#111111",
                        red = "#222222",
                    }
                },
                shell = {
                    program = "/bin/zsh",
                    args = { "-l", "-i" },
                    env = {
                        FOO = "bar"
                    }
                },
                cursor = {
                    style = "beam",
                    blink = false,
                },
                scrollback = {
                    lines = 5000,
                    scroll_multiplier = 1.5,
                }
            }
        "##;

        let root: Table = lua.load(source).eval().unwrap();
        let mut config = ForgeConfig::default();
        extract_config(root, &mut config);

        assert_eq!(config.font.family, "Fira Code");
        assert_eq!(config.font.size, 16.5);
        assert!(!config.font.ligatures);

        assert_eq!(config.window.width, 1024);
        assert_eq!(config.window.height, 768);
        assert_eq!(config.window.opacity, 0.85);
        assert_eq!(config.window.title, "My Custom Terminal");
        assert!(config.blur.enabled);
        assert_eq!(config.blur.method, BlurMethod::Kde);
        assert_eq!(config.blur.radius, 24);

        assert_eq!(
            config.theme.background,
            Color {
                r: 26,
                g: 27,
                b: 38,
                a: 255
            }
        );
        assert_eq!(
            config.theme.foreground,
            Color {
                r: 192,
                g: 202,
                b: 245,
                a: 170
            }
        );
        assert_eq!(
            config.theme.ansi_colors[0],
            Color {
                r: 17,
                g: 17,
                b: 17,
                a: 255
            }
        );
        assert_eq!(
            config.theme.ansi_colors[1],
            Color {
                r: 34,
                g: 34,
                b: 34,
                a: 255
            }
        );
        // `cursor_color` should be the default, not "red"
        assert_eq!(
            config.theme.cursor_color,
            ForgeConfig::default().theme.cursor_color
        );

        assert_eq!(config.shell.program, "/bin/zsh");
        assert_eq!(config.shell.args, vec!["-l".to_string(), "-i".to_string()]);
        assert_eq!(
            config.shell.env,
            vec![("FOO".to_string(), "bar".to_string())]
        );

        assert_eq!(config.cursor.style, CursorStyle::Beam);
        assert!(!config.cursor.blink);

        assert_eq!(config.scrollback.lines, 5000);
        assert_eq!(config.scrollback.scroll_multiplier, 1.5);
    }

    #[test]
    fn test_integer_to_float_coercion() {
        let lua = Lua::new();
        let source = r##"
            return {
                font = {
                    size = 14,
                },
            }
        "##;
        let root: Table = lua.load(source).eval().unwrap();
        let mut config = ForgeConfig::default();
        extract_config(root, &mut config);

        // The integer 14 should have been coerced to 14.0
        assert_eq!(config.font.size, 14.0);
    }
}
