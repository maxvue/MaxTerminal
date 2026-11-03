//! Geração determinística de escalas cromáticas (`ColorScale11`: `50..=950` + `content`)
//! e rampas de superfície (`BackgroundRamp`: `--background-0..=950` em passos de 25).

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::color::HexColor;
use crate::contrast::pick_content_color;
use crate::ids::ThemePolarity;
use crate::oklch::Oklch;

/// Escala cromática de 11 passos (`50..=950`) mais token `content` com contraste garantido.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ColorScale11 {
    /// Passo 50 (mais claro).
    pub p50: HexColor,
    /// Passo 100.
    pub p100: HexColor,
    /// Passo 200.
    pub p200: HexColor,
    /// Passo 300.
    pub p300: HexColor,
    /// Passo 400.
    pub p400: HexColor,
    /// Passo 500 (cor base da família).
    pub p500: HexColor,
    /// Passo 600.
    pub p600: HexColor,
    /// Passo 700.
    pub p700: HexColor,
    /// Passo 800.
    pub p800: HexColor,
    /// Passo 900.
    pub p900: HexColor,
    /// Passo 950 (mais escuro).
    pub p950: HexColor,
    /// Cor de texto sobre `p500` com contraste `>= 4.5:1`.
    pub content: HexColor,
}

impl ColorScale11 {
    /// Retorna os 11 passos na ordem `[50, 100, 200, 300, 400, 500, 600, 700, 800, 900, 950]`.
    #[must_use]
    pub const fn steps(&self) -> [(&'static str, HexColor); 11] {
        [
            ("50", self.p50),
            ("100", self.p100),
            ("200", self.p200),
            ("300", self.p300),
            ("400", self.p400),
            ("500", self.p500),
            ("600", self.p600),
            ("700", self.p700),
            ("800", self.p800),
            ("900", self.p900),
            ("950", self.p950),
        ]
    }
}

/// Rampa de 39 passos de superfície (`--background-0` a `--background-950` de 25 em 25)
/// compatível com o contrato do `@maxvue/max-components-ui`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundRamp {
    /// `--background-0`
    pub b0: HexColor,
    /// `--background-25`
    pub b25: HexColor,
    /// `--background-50`
    pub b50: HexColor,
    /// `--background-75`
    pub b75: HexColor,
    /// `--background-100`
    pub b100: HexColor,
    /// `--background-125`
    pub b125: HexColor,
    /// `--background-150`
    pub b150: HexColor,
    /// `--background-175`
    pub b175: HexColor,
    /// `--background-200`
    pub b200: HexColor,
    /// `--background-225`
    pub b225: HexColor,
    /// `--background-250`
    pub b250: HexColor,
    /// `--background-275`
    pub b275: HexColor,
    /// `--background-300`
    pub b300: HexColor,
    /// `--background-325`
    pub b325: HexColor,
    /// `--background-350`
    pub b350: HexColor,
    /// `--background-375`
    pub b375: HexColor,
    /// `--background-400`
    pub b400: HexColor,
    /// `--background-425`
    pub b425: HexColor,
    /// `--background-450`
    pub b450: HexColor,
    /// `--background-475`
    pub b475: HexColor,
    /// `--background-500`
    pub b500: HexColor,
    /// `--background-525`
    pub b525: HexColor,
    /// `--background-550`
    pub b550: HexColor,
    /// `--background-575`
    pub b575: HexColor,
    /// `--background-600`
    pub b600: HexColor,
    /// `--background-625`
    pub b625: HexColor,
    /// `--background-650`
    pub b650: HexColor,
    /// `--background-675`
    pub b675: HexColor,
    /// `--background-700`
    pub b700: HexColor,
    /// `--background-725`
    pub b725: HexColor,
    /// `--background-750`
    pub b750: HexColor,
    /// `--background-775`
    pub b775: HexColor,
    /// `--background-800`
    pub b800: HexColor,
    /// `--background-825`
    pub b825: HexColor,
    /// `--background-850`
    pub b850: HexColor,
    /// `--background-875`
    pub b875: HexColor,
    /// `--background-900`
    pub b900: HexColor,
    /// `--background-925`
    pub b925: HexColor,
    /// `--background-950`
    pub b950: HexColor,
}

impl BackgroundRamp {
    /// Retorna os 39 pares `(step_number, HexColor)` na ordem `0, 25, 50, ..., 950`.
    #[must_use]
    pub const fn steps(&self) -> [(u16, HexColor); 39] {
        [
            (0, self.b0),
            (25, self.b25),
            (50, self.b50),
            (75, self.b75),
            (100, self.b100),
            (125, self.b125),
            (150, self.b150),
            (175, self.b175),
            (200, self.b200),
            (225, self.b225),
            (250, self.b250),
            (275, self.b275),
            (300, self.b300),
            (325, self.b325),
            (350, self.b350),
            (375, self.b375),
            (400, self.b400),
            (425, self.b425),
            (450, self.b450),
            (475, self.b475),
            (500, self.b500),
            (525, self.b525),
            (550, self.b550),
            (575, self.b575),
            (600, self.b600),
            (625, self.b625),
            (650, self.b650),
            (675, self.b675),
            (700, self.b700),
            (725, self.b725),
            (750, self.b750),
            (775, self.b775),
            (800, self.b800),
            (825, self.b825),
            (850, self.b850),
            (875, self.b875),
            (900, self.b900),
            (925, self.b925),
            (950, self.b950),
        ]
    }

    /// Constrói um `BackgroundRamp` a partir de um array de 39 cores na ordem `0, 25, ..., 950`.
    #[must_use]
    pub const fn from_array(c: [HexColor; 39]) -> Self {
        Self {
            b0: c[0],
            b25: c[1],
            b50: c[2],
            b75: c[3],
            b100: c[4],
            b125: c[5],
            b150: c[6],
            b175: c[7],
            b200: c[8],
            b225: c[9],
            b250: c[10],
            b275: c[11],
            b300: c[12],
            b325: c[13],
            b350: c[14],
            b375: c[15],
            b400: c[16],
            b425: c[17],
            b450: c[18],
            b475: c[19],
            b500: c[20],
            b525: c[21],
            b550: c[22],
            b575: c[23],
            b600: c[24],
            b625: c[25],
            b650: c[26],
            b675: c[27],
            b700: c[28],
            b725: c[29],
            b750: c[30],
            b775: c[31],
            b800: c[32],
            b825: c[33],
            b850: c[34],
            b875: c[35],
            b900: c[36],
            b925: c[37],
            b950: c[38],
        }
    }
}

/// Gera uma escala cromática de 11 passos (`50..=950`) monotônica em luminosidade OKLCH (`50` mais claro → `950` mais escuro)
/// a partir de uma cor semente `seed`.
#[must_use]
#[allow(clippy::suboptimal_flops)]
pub fn generate_scale(seed: HexColor, _polarity: ThemePolarity) -> ColorScale11 {
    let base_oklch = Oklch::from_srgb(seed);
    let clamped_l500 = base_oklch.l.clamp(0.42, 0.80);
    let p500 = if (clamped_l500 - base_oklch.l).abs() < 1e-4 {
        seed
    } else {
        base_oklch.with_l(clamped_l500).to_srgb_gamut_mapped()
    };

    let l500 = Oklch::from_srgb(p500).l;
    let c500 = base_oklch.c;
    let h = base_oklch.h;

    // Interpola monotonicamente entre l_top (0.95) -> l500 -> l_bottom (0.18)
    let l_top = 0.95_f32.max(l500 + 0.14).min(0.98);
    let l_bottom = 0.18_f32.min(l500 - 0.22).max(0.14);

    let light_steps: [(f32, f32); 5] = [
        (0.00, 0.28), // 50
        (0.20, 0.45), // 100
        (0.42, 0.65), // 200
        (0.65, 0.82), // 300
        (0.84, 0.93), // 400
    ];

    let dark_steps: [(f32, f32); 5] = [
        (0.20, 0.95), // 600
        (0.42, 0.86), // 700
        (0.64, 0.74), // 800
        (0.84, 0.60), // 900
        (1.00, 0.48), // 950
    ];

    let make_light = |(t, c_factor): (f32, f32)| -> HexColor {
        let l = l_top + (l500 - l_top) * t;
        Oklch::new(l, c500 * c_factor, h).to_srgb_gamut_mapped()
    };

    let make_dark = |(t, c_factor): (f32, f32)| -> HexColor {
        let l = l500 + (l_bottom - l500) * t;
        Oklch::new(l, c500 * c_factor, h).to_srgb_gamut_mapped()
    };

    let p050 = make_light(light_steps[0]);
    let p100 = make_light(light_steps[1]);
    let p200 = make_light(light_steps[2]);
    let p300 = make_light(light_steps[3]);
    let p400 = make_light(light_steps[4]);
    let p600 = make_dark(dark_steps[0]);
    let p700 = make_dark(dark_steps[1]);
    let p800 = make_dark(dark_steps[2]);
    let p900 = make_dark(dark_steps[3]);
    let p950 = make_dark(dark_steps[4]);
    let content = pick_content_color(p500);

    ColorScale11 {
        p50: p050,
        p100,
        p200,
        p300,
        p400,
        p500,
        p600,
        p700,
        p800,
        p900,
        p950,
        content,
    }
}

/// Gera a rampa `--background-0` a `--background-950` (39 passos de 25 em 25) para temas importados/customizados,
/// respeitando a semântica de inversão do `@maxvue/max-components-ui`:
/// - Modo Escuro (`Dark`): `0` = superfície mais escura, `850..=950` = branco (`#ffffff`).
/// - Modo Claro (`Light`): `0` = superfície mais clara (`#ffffff` ou `bg`), `950` = tinta mais escura.
#[must_use]
#[allow(clippy::cast_precision_loss, clippy::suboptimal_flops)]
pub fn generate_background_ramp(
    bg: HexColor,
    fg: HexColor,
    polarity: ThemePolarity,
) -> BackgroundRamp {
    let bg_ok = Oklch::from_srgb(bg);
    let fg_ok = Oklch::from_srgb(fg);
    let neutral_c = bg_ok.c.min(0.045);
    let h = bg_ok.h;

    let mut out = [bg; 39];

    match polarity {
        ThemePolarity::Dark => {
            let l_start = bg_ok.l.clamp(0.12, 0.30);
            let l_end = fg_ok.l.max(0.96);
            for (idx, slot) in out.iter_mut().enumerate() {
                let step = idx * 25;
                if step == 0 {
                    *slot = bg;
                } else if step >= 850 {
                    *slot = HexColor::new(0xff, 0xff, 0xff);
                } else {
                    let t = (step as f32) / 850.0;
                    // Curva suave que preserva passos intermediários de superfície (25..200) próximos a bg
                    let eased = t.powf(0.92);
                    let l = l_start + (l_end - l_start) * eased;
                    let c = neutral_c * (1.0 - t * 0.85);
                    *slot = Oklch::new(l, c, h).to_srgb_gamut_mapped();
                }
            }
        }
        ThemePolarity::Light => {
            let l_start = bg_ok.l.clamp(0.92, 1.0);
            let l_end = fg_ok.l.min(0.14);
            for (idx, slot) in out.iter_mut().enumerate() {
                let step = idx * 25;
                if step == 0 {
                    *slot = bg;
                } else {
                    let t = (step as f32) / 950.0;
                    let eased = t.powf(1.08);
                    let l = l_start + (l_end - l_start) * eased;
                    let c = neutral_c * (0.35 + 0.65 * (4.0 * t * (1.0 - t)));
                    *slot = Oklch::new(l, c, h).to_srgb_gamut_mapped();
                }
            }
        }
    }

    BackgroundRamp::from_array(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contrast::contrast_ratio;

    #[test]
    fn generated_scale_is_monotonic_in_oklch_lightness_and_valid_content() {
        let seeds = [
            HexColor::new(0x00, 0x76, 0x8e),
            HexColor::new(0xbd, 0x93, 0xf9),
            HexColor::new(0x88, 0xc0, 0xd0),
            HexColor::new(0x26, 0x8b, 0xd2),
        ];

        for seed in seeds {
            let scale = generate_scale(seed, ThemePolarity::Dark);
            let steps = scale.steps();
            for pair in steps.windows(2) {
                let l_prev = Oklch::from_srgb(pair[0].1).l;
                let l_next = Oklch::from_srgb(pair[1].1).l;
                assert!(
                    l_prev > l_next,
                    "Escala não monotônica entre {} ({l_prev}) e {} ({l_next})",
                    pair[0].0,
                    pair[1].0
                );
            }
            assert!(contrast_ratio(scale.content, scale.p500) >= 4.5);
        }
    }
}
