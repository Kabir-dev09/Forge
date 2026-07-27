use forge_core::cell::Cell;
use forge_core::color::Color;
use forge_core::config_registry::{StatusbarConfig, StatusbarItem};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct ClickRegion {
    pub start_col: usize,
    pub end_col: usize,
    pub action: String,
}

pub struct StatusBarState {
    pub cells: Vec<Cell>,
    pub click_regions: Vec<ClickRegion>,
    pub vars: HashMap<String, String>,
    pub generation: u64,
    pub hovered_action: Option<String>,
    pub hovered_region: Option<(usize, usize)>,
    pub hovered_is_square: bool,
    pub hover_opacity: f32,
    last_rebuild: Option<StatusbarRebuildState>,
}

#[derive(Clone)]
struct StatusbarRebuildState {
    cols: usize,
    active_tab: usize,
    tabs_signature: u64,
    generation: u64,
    hovered_action: Option<String>,
    hover_opacity_bits: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusbarTab {
    pub index: usize,
    pub title: String,
    pub is_zoomed: bool,
}

impl Default for StatusBarState {
    fn default() -> Self {
        Self {
            cells: Vec::new(),
            click_regions: Vec::new(),
            vars: HashMap::new(),
            generation: 1,
            hovered_action: None,
            hovered_region: None,
            hovered_is_square: false,
            hover_opacity: 0.0,
            last_rebuild: None,
        }
    }
}

#[cfg(test)]
// Tests are kept near the state definitions they exercise; rendering methods follow below.
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use forge_core::config_registry::{StatusbarConfig, StatusbarItem};

    fn statusbar_text(state: &StatusBarState) -> String {
        state.cells.iter().map(|cell| cell.c).collect()
    }

    #[test]
    fn tabs_format_renders_zoom_indicator_only_for_zoomed_tabs() {
        let config = StatusbarConfig {
            left: vec![StatusbarItem::Tabs {
                format: " {index}{zoom} ".to_string(),
                zoom_indicator: "(Z)".to_string(),
                left_edge: String::new(),
                right_edge: String::new(),
                separator: String::new(),
                active: Box::new(None),
                inactive: Box::new(None),
            }],
            ..StatusbarConfig::default()
        };
        let tabs = vec![
            StatusbarTab {
                index: 0,
                title: "One".to_string(),
                is_zoomed: false,
            },
            StatusbarTab {
                index: 1,
                title: "Two".to_string(),
                is_zoomed: true,
            },
        ];

        let mut state = StatusBarState::default();
        state.rebuild(&config, 24, &tabs, 0);

        let text = statusbar_text(&state);
        assert!(text.contains(" 1 "));
        assert!(text.contains(" 2(Z) "));
    }

    #[test]
    fn zoom_indicator_keeps_tab_click_region_target() {
        let config = StatusbarConfig {
            left: vec![StatusbarItem::Tabs {
                format: " {index}{zoom} ".to_string(),
                zoom_indicator: "(Z)".to_string(),
                left_edge: String::new(),
                right_edge: String::new(),
                separator: String::new(),
                active: Box::new(None),
                inactive: Box::new(None),
            }],
            ..StatusbarConfig::default()
        };
        let tabs = vec![StatusbarTab {
            index: 0,
            title: "One".to_string(),
            is_zoomed: true,
        }];

        let mut state = StatusBarState::default();
        state.rebuild(&config, 16, &tabs, 0);

        assert_eq!(state.click_regions.len(), 1);
        assert_eq!(state.click_regions[0].action, "SwitchTab1");
        assert_eq!(state.click_regions[0].start_col, 0);
        assert_eq!(state.click_regions[0].end_col, " 1(Z) ".chars().count());
    }

    #[test]
    fn tab_edges_use_tab_background_as_foreground() {
        let config = StatusbarConfig {
            bg_color: "transparent".to_string(),
            left: vec![StatusbarItem::Tabs {
                format: " {index} ".to_string(),
                zoom_indicator: String::new(),
                left_edge: "".to_string(),
                right_edge: "".to_string(),
                separator: String::new(),
                active: Box::new(Some(forge_core::config_registry::StatusbarStyle {
                    fg: Some("#111111".to_string()),
                    bg: Some("#89B4FA".to_string()),
                    ..Default::default()
                })),
                inactive: Box::new(None),
            }],
            ..StatusbarConfig::default()
        };
        let tabs = vec![StatusbarTab {
            index: 0,
            title: "One".to_string(),
            is_zoomed: false,
        }];

        let mut state = StatusBarState::default();
        state.rebuild(&config, 16, &tabs, 0);

        assert_eq!(state.cells[0].c, '');
        assert_eq!(state.cells[0].fg, parse_hex_color("#89B4FA").unwrap());
        assert_eq!(state.cells[0].bg, Color::TRANSPARENT);
        assert_eq!(state.cells[1].c, ' ');
        assert_eq!(state.cells[1].bg, parse_hex_color("#89B4FA").unwrap());
    }

    #[test]
    fn unchanged_statusbar_model_reuses_previous_build() {
        let config = StatusbarConfig::default();
        let tabs = vec![StatusbarTab {
            index: 0,
            title: "Tab 1".to_string(),
            is_zoomed: false,
        }];
        let mut state = StatusBarState::default();

        assert!(state.needs_rebuild(80, 0, 1));
        state.rebuild(&config, 80, &tabs, 0);
        state.record_rebuild(80, 0, 1);
        assert!(!state.needs_rebuild(80, 0, 1));

        state.set_var("dir", "/tmp");
        assert!(state.needs_rebuild(80, 0, 1));
    }
}

pub fn parse_hex_color(hex: &str) -> Option<Color> {
    if hex.to_lowercase() == "transparent" {
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

fn blend_hover_background(base: Color, opacity: f32) -> Color {
    let blend = opacity * 0.35;
    let r = (base.r as f32 * (1.0 - blend) + 255.0 * blend) as u8;
    let g = (base.g as f32 * (1.0 - blend) + 255.0 * blend) as u8;
    let b = (base.b as f32 * (1.0 - blend) + 255.0 * blend) as u8;
    let a = if base.a == 0 {
        (255.0 * blend) as u8
    } else {
        base.a
    };
    Color { r, g, b, a }
}

fn push_styled_text(
    text: &str,
    default_fg: Color,
    bg: Color,
    flags: u8,
    action: Option<String>,
    out: &mut Vec<(Cell, Option<String>)>,
) {
    let mut current_fg = default_fg;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '<' {
            let mut tag = String::new();
            let mut is_tag = false;
            let mut clone_chars = chars.clone();
            for tc in clone_chars.by_ref() {
                if tc == '>' {
                    is_tag = true;
                    break;
                }
                tag.push(tc);
                if tag.len() > 10 {
                    break;
                }
            }
            if is_tag {
                if tag == "/" || tag.to_lowercase() == "reset" {
                    current_fg = default_fg;
                    chars = clone_chars;
                    continue;
                } else if tag.starts_with('#') {
                    if let Some(color) = parse_hex_color(&tag) {
                        current_fg = color;
                        chars = clone_chars;
                        continue;
                    }
                }
            }
        }
        
        out.push((
            Cell {
                c,
                fg: current_fg,
                bg,
                flags,
            },
            action.clone(),
        ));
    }
}

impl StatusBarState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_var(&mut self, key: &str, value: &str) {
        if self.vars.get(key).map(|s| s.as_str()) != Some(value) {
            self.vars.insert(key.to_string(), value.to_string());
            self.generation = self.generation.wrapping_add(1);
        }
    }

    pub fn needs_rebuild(&self, cols: usize, active_tab: usize, tabs_signature: u64) -> bool {
        self.last_rebuild.as_ref().is_none_or(|last| {
            last.cols != cols
                || last.active_tab != active_tab
                || last.tabs_signature != tabs_signature
                || last.generation != self.generation
                || last.hovered_action != self.hovered_action
                || last.hover_opacity_bits != self.hover_opacity.to_bits()
        })
    }

    pub fn record_rebuild(&mut self, cols: usize, active_tab: usize, tabs_signature: u64) {
        if let Some(last) = self.last_rebuild.as_mut() {
            if last.hovered_action != self.hovered_action {
                last.hovered_action = self.hovered_action.clone();
            }
            last.cols = cols;
            last.active_tab = active_tab;
            last.tabs_signature = tabs_signature;
            last.generation = self.generation;
            last.hover_opacity_bits = self.hover_opacity.to_bits();
        } else {
            self.last_rebuild = Some(StatusbarRebuildState {
                cols,
                active_tab,
                tabs_signature,
                generation: self.generation,
                hovered_action: self.hovered_action.clone(),
                hover_opacity_bits: self.hover_opacity.to_bits(),
            });
        }
    }

    pub fn rebuild(
        &mut self,
        config: &StatusbarConfig,
        cols: usize,
        tabs: &[StatusbarTab],
        active_tab: usize,
    ) {
        let mut new_cells = Vec::new();
        let mut new_click_regions = Vec::new();

        let bg_color = parse_hex_color(&config.bg_color).unwrap_or(Color::TRANSPARENT);
        let fg_color = parse_hex_color(&config.fg_color).unwrap_or(Color::WHITE);

        let process_items = |items: &Vec<StatusbarItem>| -> Vec<(Cell, Option<String>)> {
            let mut out = Vec::new();
            for item in items {
                match item {
                    StatusbarItem::Tabs { format, zoom_indicator, left_edge, right_edge, separator, active, inactive } => {
                        for (i, tab) in tabs.iter().enumerate() {
                            let is_active = tab.index == active_tab;
                            let style = if is_active { active } else { inactive };
                            let action_str = std::format!("SwitchTab{}", tab.index + 1);
                            let is_hovered = self.hovered_action.as_ref() == Some(&action_str);
                            let mut current_format = format.as_str();
                            let mut current_zoom = zoom_indicator.as_str();
                            let mut current_left = left_edge.as_str();
                            let mut current_right = right_edge.as_str();
                            let mut current_separator = separator.as_str();
                            
                            let mut c_bg = bg_color;
                            let mut c_fg = fg_color;
                            if let Some(st) = &**style {
                                if let Some(bg) = &st.bg {
                                    c_bg = parse_hex_color(bg).unwrap_or(bg_color);
                                }
                                if let Some(fg) = &st.fg {
                                    c_fg = parse_hex_color(fg).unwrap_or(fg_color);
                                }
                                if let Some(f) = &st.format { current_format = f; }
                                if let Some(z) = &st.zoom_indicator { current_zoom = z; }
                                if let Some(l) = &st.left_edge { current_left = l; }
                                if let Some(r) = &st.right_edge { current_right = r; }
                                if let Some(s) = &st.separator { current_separator = s; }
                            }

                            if is_hovered && self.hover_opacity > 0.01 {
                                c_bg = blend_hover_background(c_bg, self.hover_opacity);
                            }

                            let zoom = if tab.is_zoomed {
                                current_zoom
                            } else {
                                ""
                            };
                            let index = (tab.index + 1).to_string();
                            let text_val = current_format
                                .replace("{index}", &index)
                                .replace("{title}", &tab.title)
                                .replace("{zoom}", zoom);
                            let action = Some(std::format!("SwitchTab{}", tab.index + 1));
                            push_styled_text(current_left, c_bg, bg_color, 0, action.clone(), &mut out);
                            push_styled_text(&text_val, c_fg, c_bg, 0, action.clone(), &mut out);
                            push_styled_text(current_right, c_bg, bg_color, 0, action.clone(), &mut out);
                            
                            if i < tabs.len() - 1 {
                                push_styled_text(current_separator, fg_color, bg_color, 0, None, &mut out);
                            }
                        }
                    }
                    StatusbarItem::Text {
                        text,
                        fg,
                        bg,
                        action,
                        bold,
                    } => {
                        let mut resolved = text.clone();
                        for (k, v) in &self.vars {
                            resolved = resolved.replace(&std::format!("{{{}}}", k), v);
                        }
                        let c_fg = fg
                            .as_ref()
                            .and_then(|c| parse_hex_color(c))
                            .unwrap_or(fg_color);
                        let c_bg = bg
                            .as_ref()
                            .and_then(|c| parse_hex_color(c))
                            .unwrap_or(bg_color);
                        let mut flags = 0;
                        if bold.unwrap_or(false) {
                            flags |= Cell::FLAG_BOLD;
                        }
                        push_styled_text(&resolved, c_fg, c_bg, flags, action.clone(), &mut out);
                    }
                }
            }
            out
        };

        let left_cells = process_items(&config.left);
        let center_cells = process_items(&config.center);
        let right_cells = process_items(&config.right);

        let total_w = cols;
        new_cells.resize(
            total_w,
            Cell {
                c: ' ',
                fg: fg_color,
                bg: bg_color,
                flags: 0,
            },
        );

        let mut current_col = 0;
        let mut active_action: Option<String> = None;
        let mut start_col = 0;

        let place_cell = |c: Cell,
                          action: Option<String>,
                          col: usize,
                          out_click: &mut Vec<ClickRegion>,
                          cells: &mut Vec<Cell>,
                          active_action: &mut Option<String>,
                          start_col: &mut usize| {
            if col < total_w {
                cells[col] = c;
                if action != *active_action {
                    if let Some(act) = active_action.take() {
                        out_click.push(ClickRegion {
                            start_col: *start_col,
                            end_col: col,
                            action: act,
                        });
                    }
                    if let Some(act) = action.clone() {
                        *active_action = Some(act);
                        *start_col = col;
                    }
                }
            }
        };

        // Place left
        for (c, act) in left_cells {
            place_cell(
                c,
                act,
                current_col,
                &mut new_click_regions,
                &mut new_cells,
                &mut active_action,
                &mut start_col,
            );
            current_col += 1;
        }

        // Place center
        if !center_cells.is_empty() {
            let mut center_start = total_w / 2 - center_cells.len() / 2;
            center_start = center_start.max(current_col);
            if active_action.is_some() && center_start > current_col {
                new_click_regions.push(ClickRegion {
                    start_col,
                    end_col: current_col,
                    action: active_action.take().unwrap(),
                });
            }
            current_col = center_start;
            for (c, act) in center_cells {
                place_cell(
                    c,
                    act,
                    current_col,
                    &mut new_click_regions,
                    &mut new_cells,
                    &mut active_action,
                    &mut start_col,
                );
                current_col += 1;
            }
        }

        // Place right
        if !right_cells.is_empty() {
            let mut right_start = total_w.saturating_sub(right_cells.len());
            right_start = right_start.max(current_col);
            if active_action.is_some() && right_start > current_col {
                new_click_regions.push(ClickRegion {
                    start_col,
                    end_col: current_col,
                    action: active_action.take().unwrap(),
                });
            }
            current_col = right_start;
            for (c, act) in right_cells {
                place_cell(
                    c,
                    act,
                    current_col,
                    &mut new_click_regions,
                    &mut new_cells,
                    &mut active_action,
                    &mut start_col,
                );
                current_col += 1;
            }
        }

        if let Some(act) = active_action.take() {
            new_click_regions.push(ClickRegion {
                start_col,
                end_col: current_col,
                action: act,
            });
        }

        if self.cells != new_cells || self.click_regions != new_click_regions {
            self.cells = new_cells;
            self.click_regions = new_click_regions;
            self.generation = self.generation.wrapping_add(1);
        }
    }
}
