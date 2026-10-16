//! Colour palettes and value → colour mapping.

pub type Rgb = [u8; 3];

pub const MISSING: Rgb = [128, 132, 140];

const CAT: [u32; 20] = [
    0x4e79a7, 0xf28e2b, 0xe15759, 0x76b7b2, 0x59a14f, 0xedc948, 0xb07aa1, 0xff9da7, 0x9c755f, 0xbab0ac,
    0x1f77b4, 0xff7f0e, 0x2ca02c, 0xd62728, 0x9467bd, 0x8c564b, 0xe377c2, 0x7f7f7f, 0xbcbd22, 0x17becf,
];

fn hex(c: u32) -> Rgb {
    [(c >> 16) as u8, (c >> 8) as u8, c as u8]
}

fn hsv(h: f32, s: f32, v: f32) -> Rgb {
    let h = (h.fract() + 1.0).fract() * 6.0;
    let i = h.floor();
    let f = h - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    let (r, g, b) = match i as i32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8]
}

/// Categorical colour for category index `i` (20 hand-picked, then golden-ratio hues).
pub fn cat_color(i: usize) -> Rgb {
    if i < CAT.len() {
        hex(CAT[i])
    } else {
        let k = (i - CAT.len()) as f32;
        hsv(0.13 + k * 0.618_034, 0.55 + 0.25 * ((k * 0.37).fract()), 0.75 + 0.2 * ((k * 0.53).fract()))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Palette {
    Viridis,
    Plasma,
    Magma,
    YlOrRd,
    Blues,
    CoolWarm,
}

impl Palette {
    pub const ALL: [Palette; 6] =
        [Palette::Viridis, Palette::Plasma, Palette::Magma, Palette::YlOrRd, Palette::Blues, Palette::CoolWarm];

    pub fn name(&self) -> &'static str {
        match self {
            Palette::Viridis => "Viridis",
            Palette::Plasma => "Plasma",
            Palette::Magma => "Magma",
            Palette::YlOrRd => "Yellow-Orange-Red",
            Palette::Blues => "Blues",
            Palette::CoolWarm => "Cool-Warm (diverging)",
        }
    }

    fn stops(&self) -> &'static [u32] {
        match self {
            Palette::Viridis => &[0x440154, 0x472d7b, 0x3b528b, 0x2c728e, 0x21918c, 0x28ae80, 0x5ec962, 0xaddc30, 0xfde725],
            Palette::Plasma => &[0x0d0887, 0x46039f, 0x7201a8, 0x9c179e, 0xbd3786, 0xd8576b, 0xed7953, 0xfb9f3a, 0xfdca26, 0xf0f921],
            Palette::Magma => &[0x000004, 0x140e36, 0x3b0f70, 0x641a80, 0x8c2981, 0xb73779, 0xde4968, 0xf7705c, 0xfe9f6d, 0xfecf92, 0xfcfdbf],
            Palette::YlOrRd => &[0xffffcc, 0xffeda0, 0xfed976, 0xfeb24c, 0xfd8d3c, 0xfc4e2a, 0xe31a1c, 0xbd0026, 0x800026],
            Palette::Blues => &[0xf7fbff, 0xdeebf7, 0xc6dbef, 0x9ecae1, 0x6baed6, 0x4292c6, 0x2171b5, 0x08519c, 0x08306b],
            Palette::CoolWarm => &[0x3b4cc0, 0x7b9ff9, 0xc0d4f5, 0xf2cbb7, 0xee8468, 0xb40426],
        }
    }

    pub fn color(&self, t: f32) -> Rgb {
        let s = self.stops();
        let t = t.clamp(0.0, 1.0) * (s.len() - 1) as f32;
        let i = (t.floor() as usize).min(s.len() - 2);
        let f = t - i as f32;
        let (a, b) = (hex(s[i]), hex(s[i + 1]));
        [
            (a[0] as f32 + (b[0] as f32 - a[0] as f32) * f) as u8,
            (a[1] as f32 + (b[1] as f32 - a[1] as f32) * f) as u8,
            (a[2] as f32 + (b[2] as f32 - a[2] as f32) * f) as u8,
        ]
    }
}

/// Compress skewed distributions: t in 0..1 → 0..1 with a log-like curve.
pub fn log_curve(t: f32) -> f32 {
    (1.0 + 50.0 * t.clamp(0.0, 1.0)).ln() / 51f32.ln()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_endpoints() {
        assert_eq!(Palette::Viridis.color(0.0), [0x44, 0x01, 0x54]);
        assert_eq!(Palette::Viridis.color(1.0), [0xfd, 0xe7, 0x25]);
        // out-of-range clamps
        assert_eq!(Palette::Viridis.color(7.0), Palette::Viridis.color(1.0));
    }

    #[test]
    fn many_categories_distinct_enough() {
        let mut seen = std::collections::HashSet::new();
        for i in 0..60 {
            seen.insert(cat_color(i));
        }
        assert!(seen.len() >= 55);
    }

    #[test]
    fn log_curve_monotone() {
        assert!(log_curve(0.0) < 1e-6);
        assert!((log_curve(1.0) - 1.0).abs() < 1e-6);
        assert!(log_curve(0.1) > 0.1);
    }
}
