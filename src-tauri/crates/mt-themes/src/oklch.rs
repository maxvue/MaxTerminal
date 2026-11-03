//! Conversões matemáticas entre sRGB linear, Oklab, OKLCH e HSL com mapeamento de gamut por bisseção em croma.

use crate::color::HexColor;

/// Representação perceptual de cor no espaço OKLCH.
///
/// - `l`: luminosidade perceptual (`0.0..=1.0`)
/// - `c`: croma (`>= 0.0`, tipicamente `0.0..=0.4`)
/// - `h`: ângulo de matiz em graus (`0.0..360.0`)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Oklch {
    /// Luminosidade perceptual (`0.0..=1.0`).
    pub l: f32,
    /// Croma perceptual (`>= 0.0`).
    pub c: f32,
    /// Matiz em graus (`0.0..360.0`).
    pub h: f32,
}

impl Oklch {
    /// Constrói uma cor OKLCH normalizando o matiz para `[0.0, 360.0)`.
    #[must_use]
    pub fn new(l: f32, c: f32, h: f32) -> Self {
        Self {
            l: l.clamp(0.0, 1.0),
            c: c.max(0.0),
            h: h.rem_euclid(360.0),
        }
    }

    /// Converte uma cor `HexColor` (sRGB) para OKLCH segundo as matrizes de Björn Ottosson.
    #[must_use]
    #[allow(clippy::many_single_char_names, clippy::suboptimal_flops)]
    pub fn from_srgb(color: HexColor) -> Self {
        let r = srgb_u8_to_linear(color.r);
        let g = srgb_u8_to_linear(color.g);
        let b = srgb_u8_to_linear(color.b);

        let l_lms = 0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b;
        let m_lms = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
        let s_lms = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;

        let l_ = l_lms.cbrt();
        let m_ = m_lms.cbrt();
        let s_ = s_lms.cbrt();

        let big_l = 0.210_454_26 * l_ + 0.793_617_8 * m_ - 0.004_072_047 * s_;
        let a = 1.977_998_5 * l_ - 2.428_592_2 * m_ + 0.450_593_7 * s_;
        let b_ok = 0.025_904_037 * l_ + 0.782_771_77 * m_ - 0.808_675_77 * s_;

        let c = a.hypot(b_ok);
        let h = if c < 1e-6 {
            0.0
        } else {
            b_ok.atan2(a).to_degrees().rem_euclid(360.0)
        };

        Self {
            l: big_l.clamp(0.0, 1.0),
            c,
            h,
        }
    }

    /// Converte OKLCH para sRGB linear `(r, g, b)` sem clamp.
    #[must_use]
    #[allow(clippy::many_single_char_names, clippy::suboptimal_flops)]
    pub fn to_linear_srgb_unclamped(self) -> (f32, f32, f32) {
        let rad = self.h.to_radians();
        let a = self.c * rad.cos();
        let b_ok = self.c * rad.sin();

        let l_ = self.l + 0.396_337_78 * a + 0.215_803_76 * b_ok;
        let m_ = self.l - 0.105_561_346 * a - 0.063_854_17 * b_ok;
        let s_ = self.l - 0.089_484_18 * a - 1.291_485_5 * b_ok;

        let l = l_ * l_ * l_;
        let m = m_ * m_ * m_;
        let s = s_ * s_ * s_;

        let r = 4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s;
        let g = -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s;
        let b = -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s;

        (r, g, b)
    }

    /// Verifica se a cor está dentro do gamut sRGB `[0.0, 1.0]` com tolerância numérica.
    #[must_use]
    pub fn in_srgb_gamut(self) -> bool {
        const EPS: f32 = 1e-4;
        let (r, g, b) = self.to_linear_srgb_unclamped();
        (-EPS..=1.0 + EPS).contains(&r)
            && (-EPS..=1.0 + EPS).contains(&g)
            && (-EPS..=1.0 + EPS).contains(&b)
    }

    /// Converte OKLCH para `HexColor` aplicando redução de croma por bisseção quando fora do gamut sRGB,
    /// preservando luminosidade `l` e matiz `h`.
    #[must_use]
    pub fn to_srgb_gamut_mapped(self) -> HexColor {
        if self.l <= 0.0001 {
            return HexColor::new(0, 0, 0);
        }
        if self.l >= 0.9999 {
            return HexColor::new(255, 255, 255);
        }

        let mapped = if self.in_srgb_gamut() {
            self
        } else {
            let mut low = 0.0_f32;
            let mut high = self.c;
            for _ in 0..16 {
                let mid = f32::midpoint(low, high);
                let candidate = Self {
                    l: self.l,
                    c: mid,
                    h: self.h,
                };
                if candidate.in_srgb_gamut() {
                    low = mid;
                } else {
                    high = mid;
                }
            }
            Self {
                l: self.l,
                c: low,
                h: self.h,
            }
        };

        let (r, g, b) = mapped.to_linear_srgb_unclamped();
        HexColor::new(
            linear_to_srgb_u8(r),
            linear_to_srgb_u8(g),
            linear_to_srgb_u8(b),
        )
    }

    /// Retorna uma cópia com luminosidade `l` ajustada.
    #[must_use]
    pub fn with_l(self, l: f32) -> Self {
        Self {
            l: l.clamp(0.0, 1.0),
            ..self
        }
    }

    /// Retorna uma cópia com croma `c` ajustado.
    #[must_use]
    pub fn with_c(self, c: f32) -> Self {
        Self {
            c: c.max(0.0),
            ..self
        }
    }

    /// Interpola entre `self` e `other` no espaço OKLCH usando o menor arco de matiz.
    #[must_use]
    #[allow(clippy::suboptimal_flops)]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        let l = self.l + (other.l - self.l) * t;
        let c = self.c + (other.c - self.c) * t;

        let h = if self.c < 1e-4 {
            other.h
        } else if other.c < 1e-4 {
            self.h
        } else {
            let mut delta = other.h - self.h;
            if delta > 180.0 {
                delta -= 360.0;
            } else if delta < -180.0 {
                delta += 360.0;
            }
            (self.h + delta * t).rem_euclid(360.0)
        };

        Self::new(l, c, h)
    }
}

/// Converte um canal sRGB de 8 bits (`0..=255`) para luminância linear (`0.0..=1.0`).
#[must_use]
pub fn srgb_u8_to_linear(channel: u8) -> f32 {
    let c = f32::from(channel) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Converte um canal sRGB linear (`0.0..=1.0`) para canal sRGB de 8 bits (`0..=255`).
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn linear_to_srgb_u8(linear: f32) -> u8 {
    let clamped = linear.clamp(0.0, 1.0);
    let encoded = if clamped <= 0.003_130_8 {
        clamped * 12.92
    } else {
        1.055 * clamped.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round().clamp(0.0, 255.0) as u8
}

/// Converte HSL (`h` em graus, `s` e `l` em `0.0..=1.0`) para `HexColor`.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::many_single_char_names,
    clippy::suboptimal_flops
)]
pub fn hsl_to_srgb(h: f32, s: f32, l: f32) -> HexColor {
    let hue = h.rem_euclid(360.0);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;

    let (r1, g1, b1) = if hue < 60.0 {
        (c, x, 0.0)
    } else if hue < 120.0 {
        (x, c, 0.0)
    } else if hue < 180.0 {
        (0.0, c, x)
    } else if hue < 240.0 {
        (0.0, x, c)
    } else if hue < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    HexColor::new(
        ((r1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((g1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((b1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn srgb_oklch_srgb_roundtrip_within_1_lsb(r in 0u8..=255, g in 0u8..=255, b in 0u8..=255) {
            let orig = HexColor::new(r, g, b);
            let round = Oklch::from_srgb(orig).to_srgb_gamut_mapped();
            prop_assert!(orig.r.abs_diff(round.r) <= 1, "r diff: {} vs {}", orig.r, round.r);
            prop_assert!(orig.g.abs_diff(round.g) <= 1, "g diff: {} vs {}", orig.g, round.g);
            prop_assert!(orig.b.abs_diff(round.b) <= 1, "b diff: {} vs {}", orig.b, round.b);
        }
    }
}
