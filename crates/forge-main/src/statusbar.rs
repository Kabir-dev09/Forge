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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_core::config_registry::{StatusbarConfig, StatusbarItem, TabsConfig};

    fn statusbar_text(state: &StatusBarState) -> String {
        state.cells.iter().map(|cell| cell.c).collect()
    }

    #[test]
    fn tabs_format_renders_zoom_indicator_only_for_zoomed_tabs() {
        let mut config = StatusbarConfig::default();
        config.left = vec![StatusbarItem::Tabs {
            tabs: TabsConfig {
                format: " {index}{zoom} ".to_string(),
                zoom_indicator: "(Z)".to_string(),
                left_edge: String::new(),
                right_edge: String::new(),
                active: None,
                inactive: None,
            },
        }];
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
        let mut config = StatusbarConfig::default();
        config.left = vec![StatusbarItem::Tabs {
            tabs: TabsConfig {
                format: " {index}{zoom} ".to_string(),
                zoom_indicator: "(Z)".to_string(),
                left_edge: String::new(),
                right_edge: String::new(),
                active: None,
                inactive: None,
            },
        }];
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
        let mut config = StatusbarConfig::default();
        config.bg_color = "transparent".to_string();
        config.left = vec![StatusbarItem::Tabs {
            tabs: TabsConfig {
                format: " {index} ".to_string(),
                zoom_indicator: String::new(),
                left_edge: "".to_string(),
                right_edge: "".to_string(),
                active: Some(forge_core::config_registry::StatusbarStyle {
                    fg: Some("#111111".to_string()),
                    bg: Some("#89B4FA".to_string()),
                }),
                inactive: None,
            },
        }];
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
}

fn parse_hex_color(hex: &str) -> Option<Color> {
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

        let mut process_items = |items: &Vec<StatusbarItem>| -> Vec<(Cell, Option<String>)> {
            let mut out = Vec::new();
            for item in items {
                match item {
                    StatusbarItem::String(s) => {
                        let mut resolved = s.clone();
                        for (k, v) in &self.vars {
                            resolved = resolved.replace(&format!("{{{}}}", k), v);
                        }
                        for ch in resolved.chars() {
                            out.push((
                                Cell {
                                    c: ch,
                                    fg: fg_color,
                                    bg: bg_color,
                                    flags: 0,
                                },
                                None,
                            ));
                        }
                    }
                    StatusbarItem::Tabs { tabs: tabs_cfg } => {
                        for tab in tabs.iter() {
                            let is_active = tab.index == active_tab;
                            let style = if is_active {
                                &tabs_cfg.active
                            } else {
                                &tabs_cfg.inactive
                            };

                            let mut c_bg = bg_color;
                            let mut c_fg = fg_color;
                            if let Some(st) = style {
                                if let Some(bg) = &st.bg {
                                    c_bg = parse_hex_color(bg).unwrap_or(bg_color);
                                }
                                if let Some(fg) = &st.fg {
                                    c_fg = parse_hex_color(fg).unwrap_or(fg_color);
                                }
                            }

                            let zoom = if tab.is_zoomed {
                                tabs_cfg.zoom_indicator.as_str()
                            } else {
                                ""
                            };
                            let index = (tab.index + 1).to_string();
                            let text = tabs_cfg
                                .format
                                .replace("{index}", &index)
                                .replace("{title}", &tab.title)
                                .replace("{zoom}", zoom);
                            let action = Some(format!("SwitchTab{}", tab.index + 1));
                            for ch in tabs_cfg.left_edge.chars() {
                                out.push((
                                    Cell {
                                        c: ch,
                                        fg: c_bg,
                                        bg: bg_color,
                                        flags: 0,
                                    },
                                    action.clone(),
                                ));
                            }
                            for ch in text.chars() {
                                out.push((
                                    Cell {
                                        c: ch,
                                        fg: c_fg,
                                        bg: c_bg,
                                        flags: 0,
                                    },
                                    action.clone(),
                                ));
                            }
                            for ch in tabs_cfg.right_edge.chars() {
                                out.push((
                                    Cell {
                                        c: ch,
                                        fg: c_bg,
                                        bg: bg_color,
                                        flags: 0,
                                    },
                                    action.clone(),
                                ));
                            }
                        }
                    }
                    StatusbarItem::Table {
                        text,
                        fg,
                        bg,
                        action,
                        bold,
                    } => {
                        let mut resolved = text.clone();
                        for (k, v) in &self.vars {
                            resolved = resolved.replace(&format!("{{{}}}", k), v);
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
                        for ch in resolved.chars() {
                            out.push((
                                Cell {
                                    c: ch,
                                    fg: c_fg,
                                    bg: c_bg,
                                    flags,
                                },
                                action.clone(),
                            ));
                        }
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

        let mut place_cell = |c: Cell,
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
