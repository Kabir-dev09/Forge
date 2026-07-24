use crate::config_registry::{ConfigError, ForgeConfig, LigatureConfig};

impl ForgeConfig {
    pub fn strict_validation_errors(&self) -> Vec<ConfigError> {
        let mut errors = Vec::new();
        let mut range = |valid: bool, path: &str, value: String, expected: &str| {
            if !valid {
                errors.push(ConfigError::OutOfRange {
                    path: path.to_string(),
                    value,
                    expected: expected.to_string(),
                });
            }
        };

        range(
            (6.0..=72.0).contains(&self.font.size),
            "font.size",
            self.font.size.to_string(),
            "between 6 and 72",
        );
        range(
            (LigatureConfig::MIN_TOKEN_LEN..=LigatureConfig::MAX_TOKEN_LEN)
                .contains(&self.font.ligatures.max_token_len),
            "font.ligatures.max_token_len",
            self.font.ligatures.max_token_len.to_string(),
            "between 2 and 256",
        );
        range(
            (LigatureConfig::MIN_CACHE_ENTRIES..=LigatureConfig::MAX_CACHE_ENTRIES)
                .contains(&self.font.ligatures.cache_entries),
            "font.ligatures.cache_entries",
            self.font.ligatures.cache_entries.to_string(),
            "between 64 and 65536",
        );
        range(
            (200..=8000).contains(&self.window.width),
            "window.width",
            self.window.width.to_string(),
            "between 200 and 8000",
        );
        range(
            (100..=6000).contains(&self.window.height),
            "window.height",
            self.window.height.to_string(),
            "between 100 and 6000",
        );
        range(
            (0.0..=1.0).contains(&self.window.opacity),
            "window.opacity",
            self.window.opacity.to_string(),
            "between 0 and 1",
        );
        range(
            (100..=2000).contains(&self.cursor.blink_rate_ms),
            "cursor.blink_rate_ms",
            self.cursor.blink_rate_ms.to_string(),
            "between 100 and 2000",
        );
        range(
            (1..=2000).contains(&self.cursor.trail.fast_decay_ms),
            "cursor.trail.fast_decay_ms",
            self.cursor.trail.fast_decay_ms.to_string(),
            "between 1 and 2000",
        );
        range(
            self.cursor.trail.slow_decay_ms >= self.cursor.trail.fast_decay_ms
                && self.cursor.trail.slow_decay_ms <= 4000,
            "cursor.trail.slow_decay_ms",
            self.cursor.trail.slow_decay_ms.to_string(),
            "between fast_decay_ms and 4000",
        );
        range(
            self.cursor.trail.minimum_distance_x <= 1000,
            "cursor.trail.minimum_distance_x",
            self.cursor.trail.minimum_distance_x.to_string(),
            "at most 1000",
        );
        range(
            self.cursor.trail.minimum_distance_y <= 1000,
            "cursor.trail.minimum_distance_y",
            self.cursor.trail.minimum_distance_y.to_string(),
            "at most 1000",
        );
        range(
            self.cursor.trail.trigger_delay_ms <= 1000,
            "cursor.trail.trigger_delay_ms",
            self.cursor.trail.trigger_delay_ms.to_string(),
            "at most 1000",
        );
        if let Some(lines) = self.scrollback.lines {
            range(
                lines >= 100,
                "scrollback.lines",
                lines.to_string(),
                "at least 100",
            );
        }
        range(
            (0.5..=10.0).contains(&self.scrollback.scroll_multiplier),
            "scrollback.scroll_multiplier",
            self.scrollback.scroll_multiplier.to_string(),
            "between 0.5 and 10",
        );
        range(
            (250..=60_000).contains(&self.command_completion_indicator.display_duration_ms),
            "command_completion_indicator.display_duration_ms",
            self.command_completion_indicator
                .display_duration_ms
                .to_string(),
            "between 250 and 60000",
        );
        for (path, value) in [
            ("window.padding.top", self.window.padding.top),
            ("window.padding.bottom", self.window.padding.bottom),
            ("window.padding.left", self.window.padding.left),
            ("window.padding.right", self.window.padding.right),
        ] {
            range(value <= 100, path, value.to_string(), "at most 100");
        }
        for feature in &self.font.ligatures.features {
            let trimmed = feature.trim();
            if trimmed.is_empty() || trimmed.len() > 32 {
                errors.push(ConfigError::InvalidValue {
                    path: "font.ligatures.features".to_string(),
                    value: feature.clone(),
                    reason: "feature names must contain 1 to 32 bytes".to_string(),
                });
            }
        }
        errors
    }
}
