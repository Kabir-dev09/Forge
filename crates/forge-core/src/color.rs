use bytemuck::{Pod, Zeroable};
use serde::{Deserialize, Serialize};

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ColorF32 {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const BLACK: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    pub const WHITE: Color = Color {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    pub const TRANSPARENT: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };

    pub fn to_srgb_linear(self) -> ColorF32 {
        static SRGB_LUT: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
        let lut = SRGB_LUT.get_or_init(|| {
            let mut table = [0.0; 256];
            for i in 0..=255 {
                let f = i as f32 / 255.0;
                table[i] = if f <= 0.04045 {
                    f / 12.92
                } else {
                    ((f + 0.055) / 1.055).powf(2.4)
                }
                .clamp(0.0, 1.0);
            }
            table
        });

        ColorF32 {
            r: lut[self.r as usize],
            g: lut[self.g as usize],
            b: lut[self.b as usize],
            a: (self.a as f32 / 255.0).clamp(0.0, 1.0),
        }
    }
}

impl From<Color> for ColorF32 {
    fn from(color: Color) -> Self {
        ColorF32 {
            r: color.r as f32 / 255.0,
            g: color.g as f32 / 255.0,
            b: color.b as f32 / 255.0,
            a: color.a as f32 / 255.0,
        }
    }
}

pub fn ansi_256_color(index: u8, palette: &[Color; 16]) -> Color {
    match index {
        0..=15 => palette[index as usize],
        16..=231 => {
            let i = index - 16;
            let b = i % 6;
            let g = (i / 6) % 6;
            let r = i / 36;
            let scale = |v: u8| if v == 0 { 0 } else { v * 40 + 55 };
            Color {
                r: scale(r),
                g: scale(g),
                b: scale(b),
                a: 255,
            }
        }
        232..=255 => {
            let v = (index - 232) * 10 + 8;
            Color {
                r: v,
                g: v,
                b: v,
                a: 255,
            }
        }
    }
}

pub const ANSI_16: [Color; 16] = [
    Color {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    },
    Color {
        r: 194,
        g: 54,
        b: 33,
        a: 255,
    },
    Color {
        r: 37,
        g: 188,
        b: 36,
        a: 255,
    },
    Color {
        r: 173,
        g: 173,
        b: 39,
        a: 255,
    },
    Color {
        r: 73,
        g: 46,
        b: 225,
        a: 255,
    },
    Color {
        r: 211,
        g: 56,
        b: 211,
        a: 255,
    },
    Color {
        r: 51,
        g: 187,
        b: 200,
        a: 255,
    },
    Color {
        r: 203,
        g: 204,
        b: 205,
        a: 255,
    },
    Color {
        r: 129,
        g: 131,
        b: 131,
        a: 255,
    },
    Color {
        r: 252,
        g: 57,
        b: 31,
        a: 255,
    },
    Color {
        r: 49,
        g: 231,
        b: 34,
        a: 255,
    },
    Color {
        r: 234,
        g: 236,
        b: 35,
        a: 255,
    },
    Color {
        r: 88,
        g: 51,
        b: 255,
        a: 255,
    },
    Color {
        r: 249,
        g: 53,
        b: 248,
        a: 255,
    },
    Color {
        r: 20,
        g: 240,
        b: 240,
        a: 255,
    },
    Color {
        r: 233,
        g: 235,
        b: 235,
        a: 255,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_srgb_linear() {
        let linear = Color {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        }
        .to_srgb_linear();
        assert!((linear.r - 1.0).abs() < 1e-5);
        assert!((linear.g - 0.0).abs() < 1e-5);
        assert!((linear.b - 0.0).abs() < 1e-5);
        assert!((linear.a - 1.0).abs() < 1e-5);
    }
}
